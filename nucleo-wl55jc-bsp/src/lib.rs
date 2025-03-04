//! NUCLEO-WL55JC board support package.

#![cfg_attr(not(test), no_std)]
#![forbid(missing_docs)]

pub mod led;
pub mod pb;

pub use stm32wl_hal as hal;

use hal::gpio::{self, pins, Level, Output, OutputArgs};

/// RF switch.
#[derive(Debug)]
pub struct RfSwitch {
    fe_ctrl1: Output<pins::C4>,
    fe_ctrl2: Output<pins::C5>,
    fe_ctrl3: Output<pins::C3>,
}

impl RfSwitch {
    /// Create a new `RfSwitch` struct from GPIOs.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use nucleo_wl55jc_bsp::{
    ///     hal::{gpio::PortC, pac},
    ///     RfSwitch,
    /// };
    ///
    /// let mut dp: pac::Peripherals = pac::Peripherals::take().unwrap();
    ///
    /// let gpioc: PortC = PortC::split(dp.GPIOC, &mut dp.RCC);
    /// let rfs = RfSwitch::new(gpioc.c3, gpioc.c4, gpioc.c5);
    /// ```
    pub fn new(c3: pins::C3, c4: pins::C4, c5: pins::C5) -> RfSwitch {
        const ARGS: OutputArgs = OutputArgs {
            speed: gpio::Speed::Fast,
            level: gpio::Level::High,
            ot: gpio::OutputType::PushPull,
            pull: gpio::Pull::None,
        };
        RfSwitch {
            fe_ctrl1: Output::new(c4, &ARGS),
            fe_ctrl2: Output::new(c5, &ARGS),
            fe_ctrl3: Output::new(c3, &ARGS),
        }
    }

    /// Set the RF switch to receive.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use nucleo_wl55jc_bsp::{
    ///     hal::{gpio::PortC, pac},
    ///     RfSwitch,
    /// };
    ///
    /// let mut dp: pac::Peripherals = pac::Peripherals::take().unwrap();
    ///
    /// let gpioc: PortC = PortC::split(dp.GPIOC, &mut dp.RCC);
    /// let mut rfs = RfSwitch::new(gpioc.c3, gpioc.c4, gpioc.c5);
    /// rfs.set_rx()
    /// ```
    pub fn set_rx(&mut self) {
        self.fe_ctrl1.set_level(Level::High);
        self.fe_ctrl2.set_level(Level::Low);
        self.fe_ctrl3.set_level(Level::High);
    }

    /// Set the RF switch to low power transmit.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use nucleo_wl55jc_bsp::{
    ///     hal::{gpio::PortC, pac},
    ///     RfSwitch,
    /// };
    ///
    /// let mut dp: pac::Peripherals = pac::Peripherals::take().unwrap();
    ///
    /// let gpioc: PortC = PortC::split(dp.GPIOC, &mut dp.RCC);
    /// let mut rfs = RfSwitch::new(gpioc.c3, gpioc.c4, gpioc.c5);
    /// rfs.set_tx_lp()
    /// ```
    pub fn set_tx_lp(&mut self) {
        self.fe_ctrl1.set_level(Level::High);
        self.fe_ctrl2.set_level(Level::High);
        self.fe_ctrl3.set_level(Level::High);
    }

    /// Set the RF switch to high power transmit.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use nucleo_wl55jc_bsp::{
    ///     hal::{gpio::PortC, pac},
    ///     RfSwitch,
    /// };
    ///
    /// let mut dp: pac::Peripherals = pac::Peripherals::take().unwrap();
    ///
    /// let gpioc: PortC = PortC::split(dp.GPIOC, &mut dp.RCC);
    /// let mut rfs = RfSwitch::new(gpioc.c3, gpioc.c4, gpioc.c5);
    /// rfs.set_tx_hp()
    /// ```
    pub fn set_tx_hp(&mut self) {
        self.fe_ctrl2.set_level(Level::High);
        self.fe_ctrl1.set_level(Level::Low);
        self.fe_ctrl3.set_level(Level::High);
    }
}
