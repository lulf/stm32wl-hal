//! Real-time clock.

use crate::{pac, rcc::lsi_hz};

use chrono::{Datelike, NaiveDate, NaiveDateTime, NaiveTime, Timelike};

use pac::rcc::{
    bdcr::RTCSEL_A,
    csr::LSIPRE_A::{DIV1, DIV128},
};

/// RTC clock selection
#[derive(Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[repr(u8)]
pub enum Clk {
    /// LSE oscillator clock selected.
    Lse = RTCSEL_A::LSE as u8,
    /// LSI oscillator clock selected.
    Lsi = RTCSEL_A::LSI as u8,
    /// HSE32 oscillator clock divided by 32 selected.
    Hse = RTCSEL_A::HSE32 as u8,
}

/// Real-time clock driver.
#[derive(Debug)]
pub struct Rtc {
    rtc: pac::RTC,
}

impl Rtc {
    /// Create a new real-time clock driver.
    ///
    /// # Safety
    ///
    /// 1. The RTC is in the backup domain, system resets will not reset the RTC.
    ///    You are responsible for resetting the backup domain if required.
    ///    This function does not perform the reset because resetting the backup
    ///    domain also resets the LSE clock.
    /// 2. You are responsible for setting up the source clock.
    ///
    /// # Panics
    ///
    /// * (debug) clock source is not ready.
    ///
    /// # Example
    ///
    /// LSE clock source (this depends on HW, example valid for NUCLEO board):
    ///
    /// ```no_run
    /// use stm32wl_hal::{
    ///     pac,
    ///     rcc::pulse_reset_backup_domain,
    ///     rtc::{Clk, Rtc},
    /// };
    ///
    /// let mut dp: pac::Peripherals = pac::Peripherals::take().unwrap();
    ///
    /// unsafe { pulse_reset_backup_domain(&mut dp.RCC, &mut dp.PWR) };
    /// dp.PWR.cr1.modify(|_, w| w.dbp().enabled());
    /// dp.RCC.bdcr.modify(|_, w| w.lseon().on());
    /// while dp.RCC.bdcr.read().lserdy().is_not_ready() {}
    ///
    /// let rtc: Rtc = unsafe { Rtc::new(dp.RTC, Clk::Lse, &mut dp.PWR, &mut dp.RCC) };
    /// ```
    ///
    /// LSI clock source:
    ///
    /// ```no_run
    /// use stm32wl_hal::{
    ///     pac,
    ///     rcc::{enable_lsi, pulse_reset_backup_domain},
    ///     rtc::{Clk, Rtc},
    /// };
    ///
    /// let mut dp: pac::Peripherals = pac::Peripherals::take().unwrap();
    ///
    /// unsafe { pulse_reset_backup_domain(&mut dp.RCC, &mut dp.PWR) };
    /// enable_lsi(&mut dp.RCC);
    ///
    /// let rtc: Rtc = unsafe { Rtc::new(dp.RTC, Clk::Lsi, &mut dp.PWR, &mut dp.RCC) };
    /// ```
    ///
    /// HSE clock source (this depends on HW, example valid for NUCLEO board):
    ///
    /// ```no_run
    /// use stm32wl_hal::{
    ///     pac,
    ///     rcc::pulse_reset_backup_domain,
    ///     rtc::{Clk, Rtc},
    /// };
    ///
    /// let mut dp: pac::Peripherals = pac::Peripherals::take().unwrap();
    ///
    /// unsafe { pulse_reset_backup_domain(&mut dp.RCC, &mut dp.PWR) };
    /// dp.RCC
    ///     .cr
    ///     .modify(|_, w| w.hseon().enabled().hsebyppwr().vddtcxo());
    /// while dp.RCC.cr.read().hserdy().is_not_ready() {}
    ///
    /// let rtc: Rtc = unsafe { Rtc::new(dp.RTC, Clk::Hse, &mut dp.PWR, &mut dp.RCC) };
    /// ```
    pub unsafe fn new(rtc: pac::RTC, clk: Clk, pwr: &mut pac::PWR, rcc: &mut pac::RCC) -> Rtc {
        pwr.cr1.modify(|_, w| w.dbp().enabled());

        match clk {
            Clk::Lse => {
                debug_assert!(rcc.bdcr.read().lserdy().is_ready());
                rcc.bdcr.modify(|_, w| w.rtcsel().lse().rtcen().enabled());
            }
            Clk::Lsi => {
                debug_assert!(rcc.csr.read().lsirdy().is_ready());
                rcc.bdcr.modify(|_, w| w.rtcsel().lsi().rtcen().enabled());
            }
            Clk::Hse => {
                debug_assert!(rcc.cr.read().hserdy().is_ready());
                rcc.bdcr.modify(|_, w| w.rtcsel().hse32().rtcen().enabled());
            }
        }

        #[cfg(not(feature = "stm32wl5x_cm0p"))]
        rcc.apb1enr1.modify(|_, w| w.rtcapben().set_bit());
        #[cfg(feature = "stm32wl5x_cm0p")]
        rcc.c2apb1enr1.modify(|_, w| w.rtcapben().set_bit());

        let mut rtc: Rtc = Rtc { rtc };
        rtc.disable_write_protect();
        rtc.configure_prescaler(rcc);

        rtc
    }

    /// Source clock frequency in hertz.
    #[inline]
    pub fn hz(rcc: &pac::RCC) -> u32 {
        match rcc.bdcr.read().rtcsel().variant() {
            RTCSEL_A::NOCLOCK => 0,
            RTCSEL_A::LSE => 32_768,
            RTCSEL_A::LSI => lsi_hz(rcc).into(),
            RTCSEL_A::HSE32 => 1_000_000,
        }
    }

    // configure prescaler for a 1Hz clock
    //
    // RM0453 Rev 2 page 996:
    // When both prescalers are used, it is recommended to configure the
    // asynchronous prescaler to a high value to minimize consumption.
    //
    // async is 7 bit (128 max)
    // sync is 15-bit (32_768 max)
    fn configure_prescaler(&mut self, rcc: &mut pac::RCC) {
        let (a_pre, s_pre): (u8, u16) = match rcc.bdcr.read().rtcsel().variant() {
            RTCSEL_A::NOCLOCK => unreachable!(),
            // (127 + 1) × (255 + 1) = 32_768 Hz
            RTCSEL_A::LSE => (127, 255),
            RTCSEL_A::LSI => match rcc.csr.read().lsipre().variant() {
                // (99 + 1) × (319 + 1) = 32_000 Hz
                DIV1 => (99, 319),
                // (124 + 1) × (1 + 1) = 250 Hz
                DIV128 => (124, 1),
            },
            // (99 + 1) × (9_999 + 1) = 1_000_000 Hz
            RTCSEL_A::HSE32 => (99, 9_999),
        };

        // enter initialization mode
        self.rtc.icsr.modify(|_, w| w.init().init_mode());
        while self.rtc.icsr.read().initf().is_not_allowed() {}

        self.rtc
            .prer
            .write(|w| w.prediv_s().bits(s_pre).prediv_a().bits(a_pre));

        // exit initialization mode
        self.rtc.icsr.modify(|_, w| w.init().free_running_mode())
    }

    /// Set the date and time.
    ///
    /// The value will take some duration to apply after this function returns:
    ///
    /// * LPCAL=0: the counting restarts after 4 RTCCLK clock cycles
    /// * LPCAL=1: the counting restarts after up to 2 RTCCLK + 1 ck_apre
    ///
    /// # Panics
    ///
    /// * Year is greater than or equal to 2100.
    /// * Year is less than 2000.
    /// * Backup domain write protection is enabled.
    pub fn set_date_time(&mut self, date_time: chrono::NaiveDateTime) {
        // safety: atomic read with no side effects
        assert!(unsafe { (*pac::PWR::ptr()).cr1.read().dbp().bit_is_set() });

        // enter initialization mode
        self.rtc.icsr.modify(|_, w| w.init().init_mode());
        while self.rtc.icsr.read().initf().is_not_allowed() {}

        let hour: u8 = date_time.hour() as u8;
        let ht: u8 = hour / 10;
        let hu: u8 = hour % 10;

        let minute: u8 = date_time.minute() as u8;
        let mnt: u8 = minute / 10;
        let mnu: u8 = minute % 10;

        let second: u8 = date_time.second() as u8;
        let st: u8 = second / 10;
        let su: u8 = second % 10;

        #[rustfmt::skip]
        self.rtc.tr.write(|w| {
            w
                .pm().clear_bit() // 24h format
                .ht().bits(ht)
                .hu().bits(hu)
                .mnt().bits(mnt)
                .mnu().bits(mnu)
                .st().bits(st)
                .su().bits(su)
        });

        let year: i32 = date_time.year();
        assert!((2000..2100).contains(&year));
        let yt: u8 = ((year - 2000) / 10) as u8;
        let yu: u8 = ((year - 2000) % 10) as u8;

        let wdu: u8 = date_time.weekday().number_from_monday() as u8;

        let month: u8 = date_time.month() as u8;
        let mt: bool = month > 9;
        let mu: u8 = month % 10;

        let day: u8 = date_time.day() as u8;
        let dt: u8 = day / 10;
        let du: u8 = day % 10;

        #[rustfmt::skip]
        self.rtc.dr.write(|w| unsafe {
            w
                .yt().bits(yt)
                .yu().bits(yu)
                .wdu().bits(wdu)
                .mt().bit(mt)
                .mu().bits(mu)
                .dt().bits(dt)
                .du().bits(du)
        });

        // exit initialization mode
        self.rtc.icsr.modify(|_, w| w.init().free_running_mode());
    }

    /// Returns `None` if the calendar is uninitialized
    fn calendar_initialized(&self) -> Option<()> {
        use pac::rtc::icsr::INITS_A;
        match self.rtc.icsr.read().inits().variant() {
            INITS_A::NOTINITALIZED => None,
            INITS_A::INITALIZED => Some(()),
        }
    }

    /// Calendar Date
    ///
    /// Returns `None` if the calendar has not been initialized.
    pub fn date(&self) -> Option<NaiveDate> {
        self.calendar_initialized()?;
        let data = self.rtc.dr.read();
        let year: i32 = 2000 + (data.yt().bits() as i32) * 10 + (data.yu().bits() as i32);
        let month: u8 = data.mt().bits() as u8 * 10 + data.mu().bits();
        let day: u8 = data.dt().bits() * 10 + data.du().bits();
        NaiveDate::from_ymd_opt(year, month.into(), day.into())
    }

    fn ss_to_us(&self, ss: u32) -> u32 {
        // running in BCD mode, only 15:0 are used
        let ss: u32 = ss & 0xFFFF;

        let pre_s: u32 = self.rtc.prer.read().prediv_s().bits().into();
        // RM0453 Rev 2 page 1012
        // SS can be larger than PREDIV_S only after a shift operation.
        // In that case, the correct time/date is one second less than as
        // indicated by RTC_TR/RTC_DR.
        debug_assert!(ss <= pre_s);

        // RM0453 Rev 2 page 1012
        // SS[15:0] is the value in the synchronous prescaler counter.
        // The fraction of a second is given by the formula below:
        // Second fraction = (PREDIV_S - SS) / (PREDIV_S + 1)
        (((pre_s - ss) * 100_000) / (pre_s + 1)) * 10
    }

    /// Current Time
    ///
    /// Returns `None` if the calendar has not been initialized.
    pub fn time(&self) -> Option<NaiveTime> {
        loop {
            self.calendar_initialized()?;
            let ss: u32 = self.rtc.ssr.read().ss().bits();
            let data = self.rtc.tr.read();

            // If an RTCCLK edge occurs during read we may see inconsistent values
            // so read ssr again and see if it has changed
            // see RM0453 Rev 2 32.3.10 page 1002 "Reading the calendar"
            let ss_after: u32 = self.rtc.ssr.read().ss().bits();
            if ss == ss_after {
                let mut hour: u8 = data.ht().bits() * 10 + data.hu().bits();
                if data.pm().is_pm() {
                    hour += 12;
                }
                let minute: u8 = data.mnt().bits() * 10 + data.mnu().bits();
                let second: u8 = data.st().bits() * 10 + data.su().bits();
                let micro: u32 = self.ss_to_us(ss);

                return NaiveTime::from_hms_micro_opt(
                    hour as u32,
                    minute as u32,
                    second as u32,
                    micro,
                );
            }
        }
    }

    /// Calendar Date and Time
    ///
    /// Returns `None` if the calendar has not been initialized.
    pub fn date_time(&self) -> Option<NaiveDateTime> {
        loop {
            self.calendar_initialized()?;
            let ss: u32 = self.rtc.ssr.read().ss().bits();
            let dr = self.rtc.dr.read();
            let tr = self.rtc.tr.read();

            // If an RTCCLK edge occurs during read we may see inconsistent values
            // so read ssr again and see if it has changed
            // see RM0453 Rev 2 32.3.10 page 1002 "Reading the calendar"
            let ss_after: u32 = self.rtc.ssr.read().ss().bits();
            if ss == ss_after {
                let year: i32 = 2000 + (dr.yt().bits() as i32) * 10 + (dr.yu().bits() as i32);
                let month: u8 = dr.mt().bits() as u8 * 10 + dr.mu().bits();
                let day: u8 = dr.dt().bits() * 10 + dr.du().bits();

                let date: NaiveDate = NaiveDate::from_ymd_opt(year, month as u32, day as u32)?;

                let mut hour: u8 = tr.ht().bits() * 10 + tr.hu().bits();
                if tr.pm().is_pm() {
                    hour += 12;
                }
                let minute: u8 = tr.mnt().bits() * 10 + tr.mnu().bits();
                let second: u8 = tr.st().bits() * 10 + tr.su().bits();
                let micro: u32 = self.ss_to_us(ss);

                let time = NaiveTime::from_hms_micro_opt(
                    hour as u32,
                    minute as u32,
                    second as u32,
                    micro,
                )?;

                return Some(date.and_time(time));
            }
        }
    }

    /// Disable the RTC write protection.
    #[inline]
    pub fn disable_write_protect(&mut self) {
        self.rtc.wpr.write(|w| w.key().deactivate1());
        self.rtc.wpr.write(|w| w.key().deactivate2());
    }

    /// Enable the RTC write protection.
    ///
    /// # Safety
    ///
    /// * You must call [`disable_write_protect`] before using any other
    ///   `&mut self` RTC method.
    ///
    /// [`disable_write_protect`]: Self::disable_write_protect
    #[inline]
    pub unsafe fn enable_write_protect(&mut self) {
        self.rtc.wpr.write(|w| w.key().activate());
    }
}
