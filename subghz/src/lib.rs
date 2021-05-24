//! The sub-GHz radio is an ultra-low-power sub-GHz radio operating in the
//! 150 - 960 MHz ISM band.
#![cfg_attr(not(test), no_std)]

mod calibrate;
mod irq;
mod mod_params;
mod ocp;
mod op_error;
mod pa_config;
mod packet_params;
mod packet_status;
mod packet_type;
mod reg_mode;
mod rf_frequency;
mod rx_timeout_stop;
mod standby_clk;
mod stats;
mod status;
mod tcxo_mode;
mod timeout;
mod value_error;

pub use calibrate::{Calibrate, CalibrateImage};
pub use irq::{CfgDioIrq, Irq, IrqLine};
pub use mod_params::{CodingRate, LoRaBandwidth, LoRaModParams, SpreadingFactor};
pub use mod_params::{GfskBandwidth, GfskBitrate, GfskFdev, GfskModParams, GfskPulseShape};
pub use ocp::Ocp;
pub use op_error::OpError;
pub use pa_config::{PaConfig, PaSel};
pub use packet_params::{AddrComp, CrcType, GenericPacketParams, PayloadType, PreambleDetection};
pub use packet_status::GfskPacketStatus;
pub use packet_type::PacketType;
pub use reg_mode::RegMode;
pub use rf_frequency::RfFreq;
pub use rx_timeout_stop::RxTimeoutStop;
pub use standby_clk::StandbyClk;
pub use stats::{FskStats, LoRaStats, Stats};
pub use status::{CmdStatus, Status, StatusMode};
pub use tcxo_mode::{TcxoMode, TcxoTrim};
pub use timeout::Timeout;
pub use value_error::ValueError;

use core::{
    convert::Infallible,
    ptr::{read_volatile, write_volatile},
};

pub use num_rational;

use num_rational::Ratio;

cfg_if::cfg_if! {
    if #[cfg(feature = "stm32wl5x_cm0p")] {
        pub use stm32wl::stm32wl5x_cm0p as pac;
    } else if #[cfg(feature = "stm32wl5x_cm4")] {
        pub use stm32wl::stm32wl5x_cm4 as pac;
    } else if #[cfg(feature = "stm32wle5")] {
        pub use stm32wl::stm32wle5 as pac;
    } else {
        core::compile_error!("You must select your hardware with a feature flag");
    }
}

/// Errors?  What errors!  TODO.
pub type SubGhzError = Infallible;

/// sub-GHz radio peripheral.
pub struct SubGhz {
    spi: pac::SPI3,
}

impl SubGhz {
    /// Create a new sub-GHz radio driver from a peripheral.
    ///
    /// This will initialize the SPI bus, and bring the radio out of reset,
    /// but it will **not** enable the clocks on the radio.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use stm32wl_hal_subghz::{pac, SubGhz};
    ///
    /// let dp: pac::Peripherals = pac::Peripherals::take().unwrap();
    /// let mut rcc = dp.RCC;
    /// let pwr = dp.PWR;
    ///
    /// // ... setup the system clocks
    ///
    /// rcc.apb3enr.modify(|_, w| w.subghzspien().set_bit());
    /// rcc.csr.modify(|_, w| w.rfrst().clear_bit());
    /// pwr.subghzspicr.write(|w| w.nss().clear_bit());
    /// pwr.subghzspicr.write(|w| w.nss().set_bit());
    ///
    /// let sg = SubGhz::new(dp.SPI3, &mut rcc);
    /// ```
    pub fn new(spi: pac::SPI3, rcc: &mut pac::RCC) -> SubGhz {
        rcc.apb3rstr.write(|w| w.subghzspirst().set_bit());
        rcc.apb3rstr.write(|w| w.subghzspirst().clear_bit());

        #[rustfmt::skip]
        spi.cr2.write(|w| unsafe {
            w
                // 8-bit data size
                .ds().bits(0b111)
                // RXNE generated on 8-bits
                .frxth().set_bit()
        });

        #[rustfmt::skip]
        spi.cr1.write(|w| unsafe {
            w
                // SPI clock phase 0
                .cpha().clear_bit()
                // SPI clock polarity 0
                .cpol().clear_bit()
                // SPI is a master device
                .mstr().set_bit()
                // this is an internal device - no baud rate divisor necessary
                .br().bits(0b000)
                // LSB first
                .lsbfirst().clear_bit()
                // enable software slave management
                .ssm().set_bit()
                .ssi().set_bit()
                // Full duplex
                .rxonly().clear_bit()
                .bidioe().clear_bit()
                .bidimode().clear_bit()
                // Enable the SPI peripheral
                .spe().set_bit()
        });

        SubGhz { spi }
    }

    /// Magically creates a radio peripheral out of thin air.
    ///
    /// This will **not** initialize the SPI bus (unlike [`new`]).
    ///
    /// # Safety
    ///
    /// This will create a new `SPI3` peripheral, bypassing the singleton checks
    /// that normally occur.
    /// You are responsible for ensuring that the radio has exclusive access to
    /// these peripherals.
    ///
    /// # Example
    ///
    /// ```
    /// use stm32wl_hal_subghz::SubGhz;
    ///
    /// // ... setup happens here
    ///
    /// let sg = unsafe { SubGhz::conjure() };
    /// ```
    ///
    /// [`new`]: SubGhz::new
    pub unsafe fn conjure() -> SubGhz {
        let dp: pac::Peripherals = pac::Peripherals::steal();
        SubGhz { spi: dp.SPI3 }
    }

    /// Returns `true` if RFBUSYS is high.
    ///
    /// This indicates the radio is busy.
    ///
    /// See section 6.3 "Radio busy management" for more details.
    pub fn rfbusys(&self) -> bool {
        let dp = unsafe { pac::Peripherals::steal() };
        dp.PWR.sr2.read().rfbusys().bit_is_set()
    }

    fn poll_not_busy(&self) {
        let mut count: u32 = 100_000;
        while self.rfbusys() {
            count -= 1;
            if count == 0 {
                let dp = unsafe { pac::Peripherals::steal() };
                panic!(
                    "pwr.sr2=0x{:X} pwr.subghzspicr=0x{:X} pwr.cr1=0x{:X}",
                    dp.PWR.sr2.read().bits(),
                    dp.PWR.subghzspicr.read().bits(),
                    dp.PWR.cr1.read().bits(),
                );
            }
        }
    }

    /// Read from the sub-GHz radio.
    ///
    /// # Arguments
    ///
    /// * `opcode` - Opcode for the command.
    /// * `data` - Buffer to read data into. The number of bytes read is equal
    ///   to the length of this buffer.
    #[allow(clippy::unnecessary_wraps)]
    fn read(&self, opcode: OpCode, data: &mut [u8]) -> Result<(), SubGhzError> {
        let dp = unsafe { pac::Peripherals::steal() };
        let pwr = &dp.PWR;

        self.poll_not_busy();
        pwr.subghzspicr.write(|w| w.nss().clear_bit());

        self.write_byte_raw(opcode as u8);

        for byte in data.iter_mut() {
            *byte = self.read_byte_raw()
        }

        pwr.subghzspicr.write(|w| w.nss().set_bit());
        self.poll_not_busy();

        Ok(())
    }

    #[allow(clippy::unnecessary_wraps)]
    fn write(&self, data: &[u8]) -> Result<(), SubGhzError> {
        let dp = unsafe { pac::Peripherals::steal() };
        let pwr = &dp.PWR;
        self.poll_not_busy();

        pwr.subghzspicr.write(|w| w.nss().clear_bit());
        data.iter().for_each(|&b| self.write_byte_raw(b));
        pwr.subghzspicr.write(|w| w.nss().set_bit());

        self.poll_not_busy();
        Ok(())
    }

    fn write_byte_raw(&self, byte: u8) {
        while self.spi.sr.read().txe().bit_is_clear() {}
        unsafe { write_volatile(&self.spi.dr as *const _ as *mut u8, byte) };
        while self.spi.sr.read().rxne().bit_is_clear() {}
        unsafe { read_volatile(&self.spi.dr as *const _ as *const u8) };
    }

    fn read_byte_raw(&self) -> u8 {
        while self.spi.sr.read().txe().bit_is_clear() {}
        unsafe { write_volatile(&self.spi.dr as *const _ as *mut u8, 0xFF) };
        while self.spi.sr.read().rxne().bit_is_clear() {}
        unsafe { read_volatile(&self.spi.dr as *const _ as *const u8) }
    }

    /// Read one byte from the sub-Ghz radio.
    fn read_1(&self, opcode: OpCode) -> Result<u8, SubGhzError> {
        let mut buf: [u8; 1] = [0; 1];
        self.read(opcode, &mut buf)?;
        Ok(buf[0])
    }

    /// Read a fixed number of bytes from the sub-Ghz radio.
    fn read_n<const N: usize>(&self, opcode: OpCode) -> Result<[u8; N], SubGhzError> {
        let mut buf: [u8; N] = [0; N];
        self.read(opcode, &mut buf)?;
        Ok(buf)
    }

    // TODO: make a struct for the input value.
    pub fn set_hse_in_trim(&mut self, in_trimr: u8) -> Result<(), SubGhzError> {
        self.write_register(Register::HSEOUTTRIM, &[in_trimr])
    }

    /// Set the LoRa sync word.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # let mut sg = unsafe { stm32wl_hal_subghz::SubGhz::conjure() };
    /// use stm32wl_hal_subghz::PacketType;
    ///
    /// sg.set_packet_type(PacketType::LoRa)?;
    /// sg.set_lora_sync_word(0x1234)?;
    /// # Ok::<(), stm32wl_hal_subghz::SubGhzError>(())
    /// ```
    pub fn set_lora_sync_word(&mut self, sync_word: u16) -> Result<(), SubGhzError> {
        self.write_register(Register::LSYNCH, &sync_word.to_be_bytes())
    }

    /// Set the power amplifier over current protection.
    ///
    /// # Example
    ///
    /// Maximum 60mA for LP PA mode.
    ///
    /// ```no_run
    /// # let mut sg = unsafe { stm32wl_hal_subghz::SubGhz::conjure() };
    /// use stm32wl_hal_subghz::Ocp;
    ///
    /// sg.set_pa_ocp(Ocp::Max60m)?;
    /// # Ok::<(), stm32wl_hal_subghz::SubGhzError>(())
    /// ```
    ///
    /// Maximum 60mA for HP PA mode.
    ///
    /// ```no_run
    /// # let mut sg = unsafe { stm32wl_hal_subghz::SubGhz::conjure() };
    /// use stm32wl_hal_subghz::Ocp;
    ///
    /// sg.set_pa_ocp(Ocp::Max140m)?;
    /// # Ok::<(), stm32wl_hal_subghz::SubGhzError>(())
    /// ```
    pub fn set_pa_ocp(&mut self, ocp: Ocp) -> Result<(), SubGhzError> {
        self.write_register(Register::PAOCP, &[ocp as u8])
    }

    /// Set the synchronization word registers.
    pub fn set_sync_word(&mut self, sync_word: [u8; 8]) -> Result<(), SubGhzError> {
        self.write_register(Register::GSYNC7, &sync_word)
    }
}

// 5.8.2
/// Register and buffer access commands.
impl SubGhz {
    #[allow(dead_code)]
    #[allow(clippy::unnecessary_wraps)]
    fn read_register(&mut self, register: Register) -> Result<u8, SubGhzError> {
        let dp = unsafe { pac::Peripherals::steal() };
        let pwr = &dp.PWR;
        self.poll_not_busy();

        pwr.subghzspicr.write(|w| w.nss().clear_bit());
        self.write_byte_raw(0x1D);
        let addr: u16 = register.address();
        self.write_byte_raw(((addr >> 8) & 0xFF) as u8);
        self.write_byte_raw((addr & 0xFF) as u8);
        let ret: u8 = self.read_byte_raw();
        pwr.subghzspicr.write(|w| w.nss().set_bit());

        self.poll_not_busy();
        Ok(ret)
    }

    #[allow(clippy::unnecessary_wraps)]
    fn write_register(&mut self, register: Register, data: &[u8]) -> Result<(), SubGhzError> {
        let dp = unsafe { pac::Peripherals::steal() };
        let pwr = &dp.PWR;
        self.poll_not_busy();

        pwr.subghzspicr.write(|w| w.nss().clear_bit());
        self.write_byte_raw(OpCode::WriteRegister as u8);
        register
            .address()
            .to_be_bytes()
            .iter()
            .for_each(|&b| self.write_byte_raw(b));
        data.iter().for_each(|&b| self.write_byte_raw(b));
        pwr.subghzspicr.write(|w| w.nss().set_bit());

        self.poll_not_busy();
        Ok(())
    }

    pub fn write_buffer(&mut self, offset: u8, data: &[u8]) -> Result<(), SubGhzError> {
        let dp = unsafe { pac::Peripherals::steal() };
        let pwr = &dp.PWR;
        self.poll_not_busy();

        pwr.subghzspicr.write(|w| w.nss().clear_bit());
        self.write_byte_raw(OpCode::WriteBuffer as u8);
        self.write_byte_raw(offset);
        data.iter().for_each(|&b| self.write_byte_raw(b));
        pwr.subghzspicr.write(|w| w.nss().set_bit());

        self.poll_not_busy();
        Ok(())
    }

    pub fn read_buffer(&mut self, offset: u8, buf: &mut [u8]) -> Result<Status, SubGhzError> {
        let dp = unsafe { pac::Peripherals::steal() };
        let pwr = &dp.PWR;
        self.poll_not_busy();

        pwr.subghzspicr.write(|w| w.nss().clear_bit());
        self.write_byte_raw(OpCode::WriteBuffer as u8);
        self.write_byte_raw(offset);
        let status: Status = self.read_byte_raw().into();
        buf.iter_mut().for_each(|b| *b = self.read_byte_raw());
        pwr.subghzspicr.write(|w| w.nss().set_bit());

        self.poll_not_busy();
        Ok(status)
    }
}

// 5.8.3
/// Operating mode commands.
impl SubGhz {
    // TODO: set_sleep

    /// Put the radio into standby mode.
    ///
    /// # Examples
    ///
    /// Put the radio into standby mode using the RC 13MHz clock.
    ///
    /// ```no_run
    /// # let mut sg = unsafe { stm32wl_hal_subghz::SubGhz::conjure() };
    /// use stm32wl_hal_subghz::StandbyClk;
    ///
    /// sg.set_standby(StandbyClk::Rc)?;
    /// # Ok::<(), stm32wl_hal_subghz::SubGhzError>(())
    /// ```
    ///
    /// Put the radio into standby mode using the HSE32 clock.
    ///
    /// ```no_run
    /// # let mut sg = unsafe { stm32wl_hal_subghz::SubGhz::conjure() };
    /// use stm32wl_hal_subghz::StandbyClk;
    ///
    /// sg.set_standby(StandbyClk::Hse32)?;
    /// # Ok::<(), stm32wl_hal_subghz::SubGhzError>(())
    /// ```
    pub fn set_standby(&mut self, standby_clk: StandbyClk) -> Result<(), SubGhzError> {
        self.write(&[OpCode::SetStandby as u8, standby_clk as u8])
    }

    // TODO: set_fs

    /// Set the sub-GHz radio in TX mode.
    ///
    /// # Example
    ///
    /// Transmit with no timeout.
    ///
    /// ```no_run
    /// # let mut sg = unsafe { stm32wl_hal_subghz::SubGhz::conjure() };
    /// use stm32wl_hal_subghz::Timeout;
    ///
    /// sg.set_tx(Timeout::DISABLED)?;
    /// # Ok::<(), stm32wl_hal_subghz::SubGhzError>(())
    /// ```
    pub fn set_tx(&mut self, timeout: Timeout) -> Result<(), SubGhzError> {
        let tobits: u32 = timeout.into_bits();
        self.write(&[
            crate::OpCode::SetTx as u8,
            ((tobits >> 16) & 0xFF) as u8,
            ((tobits >> 8) & 0xFF) as u8,
            (tobits & 0xFF) as u8,
        ])
    }

    /// Set the sub-GHz radio in RX mode.
    ///
    /// # Example
    ///
    /// Receive with a 1 second timeout.
    ///
    /// ```no_run
    /// # let mut sg = unsafe { stm32wl_hal_subghz::SubGhz::conjure() };
    /// use core::time::Duration;
    /// use stm32wl_hal_subghz::Timeout;
    ///
    /// sg.set_rx(Timeout::from_duration_sat(Duration::from_secs(1)))?;
    /// # Ok::<(), stm32wl_hal_subghz::SubGhzError>(())
    /// ```
    pub fn set_rx(&mut self, timeout: Timeout) -> Result<(), SubGhzError> {
        let tobits: u32 = timeout.into_bits();
        self.write(&[
            crate::OpCode::SetRx as u8,
            ((tobits >> 16) & 0xFF) as u8,
            ((tobits >> 8) & 0xFF) as u8,
            (tobits & 0xFF) as u8,
        ])
    }

    /// Allows selection of the receiver event which stops the RX timeout timer.
    ///
    /// # Example
    ///
    /// Set the RX timeout timer to stop on preamble detection.
    ///
    /// ```no_run
    /// # let mut sg = unsafe { stm32wl_hal_subghz::SubGhz::conjure() };
    /// use stm32wl_hal_subghz::RxTimeoutStop;
    ///
    /// sg.set_rx_timeout_stop(RxTimeoutStop::Preamble)?;
    /// # Ok::<(), stm32wl_hal_subghz::SubGhzError>(())
    /// ```
    pub fn set_rx_timeout_stop(
        &mut self,
        rx_timeout_stop: RxTimeoutStop,
    ) -> Result<(), SubGhzError> {
        self.write(&[
            OpCode::SetStopRxTimerOnPreamble as u8,
            rx_timeout_stop as u8,
        ])
    }

    // TODO: set_rx_duty_cycle

    // TODO: set_cad

    pub fn set_tx_continuous_wave(&mut self) -> Result<(), SubGhzError> {
        self.write(&[OpCode::SetTxContinuousWave as u8])
    }

    // TODO: set_tx_continuous_preamble
}

// 5.8.4
/// Radio configuration commands.
impl SubGhz {
    /// Set the packet type (modulation scheme).
    ///
    /// # Examples
    ///
    /// FSK (frequency shift keying):
    ///
    /// ```no_run
    /// # let mut sg = unsafe { stm32wl_hal_subghz::SubGhz::conjure() };
    /// use stm32wl_hal_subghz::PacketType;
    ///
    /// sg.set_packet_type(PacketType::Fsk)?;
    /// # Ok::<(), stm32wl_hal_subghz::SubGhzError>(())
    /// ```
    ///
    /// LoRa (long range):
    ///
    /// ```no_run
    /// # let mut sg = unsafe { stm32wl_hal_subghz::SubGhz::conjure() };
    /// use stm32wl_hal_subghz::PacketType;
    ///
    /// sg.set_packet_type(PacketType::LoRa)?;
    /// # Ok::<(), stm32wl_hal_subghz::SubGhzError>(())
    /// ```
    ///
    /// BPSK (binary phase shift keying):
    ///
    /// ```no_run
    /// # let mut sg = unsafe { stm32wl_hal_subghz::SubGhz::conjure() };
    /// use stm32wl_hal_subghz::PacketType;
    ///
    /// sg.set_packet_type(PacketType::Bpsk)?;
    /// # Ok::<(), stm32wl_hal_subghz::SubGhzError>(())
    /// ```
    ///
    /// MSK (minimum shift keying):
    ///
    /// ```no_run
    /// # let mut sg = unsafe { stm32wl_hal_subghz::SubGhz::conjure() };
    /// use stm32wl_hal_subghz::PacketType;
    ///
    /// sg.set_packet_type(PacketType::Msk)?;
    /// # Ok::<(), stm32wl_hal_subghz::SubGhzError>(())
    /// ```
    pub fn set_packet_type(&self, packet_type: PacketType) -> Result<(), SubGhzError> {
        self.write(&[OpCode::SetPacketType as u8, packet_type as u8])
    }

    /// Get the packet type.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # let mut sg = unsafe { stm32wl_hal_subghz::SubGhz::conjure() };
    /// use stm32wl_hal_subghz::PacketType;
    ///
    /// sg.set_packet_type(PacketType::LoRa)?;
    /// assert_eq!(sg.packet_type()?, Ok(PacketType::LoRa));
    /// # Ok::<(), stm32wl_hal_subghz::SubGhzError>(())
    /// ```
    pub fn packet_type(&self) -> Result<Result<PacketType, u8>, SubGhzError> {
        let pkt_type: [u8; 2] = self.read_n(OpCode::GetPacketType)?;
        Ok(PacketType::from_bits(pkt_type[1]))
    }

    /// Set the radio carrier frequency.
    ///
    /// # Example
    ///
    /// Set the frequency to 915MHz (Australia and North America).
    ///
    /// ```no_run
    /// # let mut sg = unsafe { stm32wl_hal_subghz::SubGhz::conjure() };
    /// use stm32wl_hal_subghz::RfFreq;
    ///
    /// sg.set_rf_frequency(&RfFreq::F915)?;
    /// # Ok::<(), stm32wl_hal_subghz::SubGhzError>(())
    /// ```
    pub fn set_rf_frequency(&mut self, freq: &RfFreq) -> Result<(), SubGhzError> {
        self.write(freq.as_slice())
    }

    pub fn set_tx_params(&mut self, power: u8, ramp_time: RampTime) -> Result<(), SubGhzError> {
        self.write(&[OpCode::SetTxParams as u8, power, ramp_time.into()])
    }

    /// Power amplifier configuation, used to customize the maximum output power
    /// and efficiency.
    pub fn set_pa_config(&mut self, pa_config: &PaConfig) -> Result<(), SubGhzError> {
        self.write(pa_config.as_slice())
    }

    // TODO: Set_TxRxFallbackMode

    // TODO: Set_CadParams

    /// Set the data buffer base address for the packet handling in TX and RX.
    ///
    /// There is a 256B TX buffer and a 256B RX buffer.
    /// These buffers are not memory mapped, they are accessed via the
    /// [`read_buffer`] and [`write_buffer`] methods.
    ///
    /// # Example
    ///
    /// Set the TX and RX buffer base to the start.
    ///
    /// ```no_run
    /// # let mut sg = unsafe { stm32wl_hal_subghz::SubGhz::conjure() };
    /// sg.set_buffer_base_address(0, 0)?;
    /// # Ok::<(), stm32wl_hal_subghz::SubGhzError>(())
    /// ```
    ///
    /// [`read_buffer`]: SubGhz::read_buffer
    /// [`write_buffer`]: SubGhz::write_buffer
    pub fn set_buffer_base_address(&mut self, tx: u8, rx: u8) -> Result<(), SubGhzError> {
        self.write(&[OpCode::SetBufferBaseAddress as u8, tx, rx])
    }

    /// Set the (G)FSK modulation parameters.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # let mut sg = unsafe { stm32wl_hal_subghz::SubGhz::conjure() };
    /// use stm32wl_hal_subghz::{
    ///     GfskBandwidth, GfskBitrate, GfskFdev, GfskModParams, GfskPulseShape, PacketType,
    /// };
    ///
    /// const BITRATE: GfskBitrate = GfskBitrate::from_bps(32_000);
    /// const PULSE_SHAPE: GfskPulseShape = GfskPulseShape::Bt03;
    /// const BW: GfskBandwidth = GfskBandwidth::Bw9;
    /// const FDEV: GfskFdev = GfskFdev::from_hertz(31_250);
    ///
    /// const MOD_PARAMS: GfskModParams = GfskModParams::new()
    ///     .set_bitrate(BITRATE)
    ///     .set_pulse_shape(PULSE_SHAPE)
    ///     .set_bandwidth(BW)
    ///     .set_fdev(FDEV);
    ///
    /// sg.set_packet_type(PacketType::Fsk)?;
    /// sg.set_gfsk_mod_params(&MOD_PARAMS)?;
    /// # Ok::<(), stm32wl_hal_subghz::SubGhzError>(())
    /// ```
    pub fn set_gfsk_mod_params(&mut self, params: &GfskModParams) -> Result<(), SubGhzError> {
        self.write(params.as_slice())
    }

    /// Set the LoRa modulation parameters.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # let mut sg = unsafe { stm32wl_hal_subghz::SubGhz::conjure() };
    /// use stm32wl_hal_subghz::{
    ///     CodingRate, LoRaBandwidth, LoRaModParams, PacketType, SpreadingFactor,
    /// };
    ///
    /// const MOD_PARAMS: LoRaModParams = LoRaModParams::new()
    ///     .set_sf(SpreadingFactor::Sf7)
    ///     .set_bw(LoRaBandwidth::Bw125)
    ///     .set_cr(CodingRate::Cr45)
    ///     .set_ldro_en(false);
    ///
    /// sg.set_packet_type(PacketType::LoRa)?;
    /// sg.set_lora_mod_params(&MOD_PARAMS)?;
    /// # Ok::<(), stm32wl_hal_subghz::SubGhzError>(())
    /// ```
    pub fn set_lora_mod_params(&mut self, params: &LoRaModParams) -> Result<(), SubGhzError> {
        self.write(params.as_slice())
    }

    // TODO: BPSK `Set_ModulationParams`

    pub fn set_packet_params(&mut self, params: &GenericPacketParams) -> Result<(), SubGhzError> {
        self.write(params.as_slice())
    }

    // TODO: BPSK `Set_PacketParams`

    // TODO: LoRa `Set_PacketParams`

    // TODO: `Set_LoRaSymbTimeout`
}

// 5.8.5
/// Communication status and information commands
impl SubGhz {
    /// Get the radio status.
    ///
    /// The hardware appears to have many bugs where this will return reserved
    /// values.
    ///
    /// ```no_run
    /// # let mut sg = unsafe { stm32wl_hal_subghz::SubGhz::conjure() };
    /// use stm32wl_hal_subghz::Status;
    ///
    /// let status: Status = sg.status()?;
    /// # Ok::<(), stm32wl_hal_subghz::SubGhzError>(())
    /// ```
    pub fn status(&self) -> Result<Status, SubGhzError> {
        Ok(self.read_1(OpCode::GetStatus)?.into())
    }

    /// Get the RX buffer status.
    ///
    /// The return tuple is (status, payload_length, buffer_pointer).
    ///
    /// # Example
    ///
    /// ```no_run
    /// # let mut sg = unsafe { stm32wl_hal_subghz::SubGhz::conjure() };
    /// use stm32wl_hal_subghz::{CmdStatus, Timeout};
    ///
    /// sg.set_rx(Timeout::DISABLED)?;
    /// loop {
    ///     let (status, len, ptr) = sg.rx_buffer_status()?;
    ///
    ///     if status.cmd() == Ok(CmdStatus::Avaliable) {
    ///         let mut buf: [u8; 256] = [0; 256];
    ///         let data: &mut [u8] = &mut buf[..usize::from(len)];
    ///         sg.read_buffer(ptr, data)?;
    ///         // ... do things with the data
    ///         break;
    ///     }
    /// }
    /// # Ok::<(), stm32wl_hal_subghz::SubGhzError>(())
    /// ```
    pub fn rx_buffer_status(&self) -> Result<(Status, u8, u8), SubGhzError> {
        let data: [u8; 3] = self.read_n(OpCode::GetRxBufferStatus)?;
        Ok((data[0].into(), data[1], data[2]))
    }

    /// Returns information on the last received packet.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use std::fmt::Write;
    /// # let mut uart = String::new();
    /// # let mut sg = unsafe { stm32wl_hal_subghz::SubGhz::conjure() };
    /// use stm32wl_hal_subghz::{CmdStatus, Timeout};
    ///
    /// sg.set_rx(Timeout::DISABLED)?;
    /// loop {
    ///     let pkt_status = sg.gfsk_packet_status()?;
    ///
    ///     if pkt_status.status().cmd() == Ok(CmdStatus::Avaliable) {
    ///         let rssi = pkt_status.rssi_avg();
    ///         writeln!(&mut uart, "Avg RSSI: {}", rssi);
    ///         break;
    ///     }
    /// }
    /// # Ok::<(), stm32wl_hal_subghz::SubGhzError>(())
    /// ```
    pub fn gfsk_packet_status(&self) -> Result<GfskPacketStatus, SubGhzError> {
        Ok(GfskPacketStatus::from(
            self.read_n(OpCode::GetPacketStatus)?,
        ))
    }

    // TODO: LoRa Get_PacketStatus

    /// Get the instantaneous signal strength during packet reception.
    ///
    /// The units are in dbm.
    ///
    /// # Example
    ///
    /// Log the instantaneous signal strength to UART.
    ///
    /// ```no_run
    /// # use std::fmt::Write;
    /// # let mut uart = String::new();
    /// # let mut sg = unsafe { stm32wl_hal_subghz::SubGhz::conjure() };
    /// use stm32wl_hal_subghz::{CmdStatus, Timeout};
    ///
    /// sg.set_rx(Timeout::DISABLED)?;
    /// let (_, rssi) = sg.rssi_inst()?;
    /// writeln!(&mut uart, "RSSI: {} dBm", rssi);
    /// # Ok::<(), stm32wl_hal_subghz::SubGhzError>(())
    /// ```
    pub fn rssi_inst(&self) -> Result<(Status, Ratio<i16>), SubGhzError> {
        let data: [u8; 2] = self.read_n(OpCode::GetRssiInst)?;
        let status: Status = data[0].into();
        let rssi: Ratio<i16> = Ratio::new(i16::from(data[1]), -2);

        Ok((status, rssi))
    }

    /// (G)FSK packet stats.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # let mut sg = unsafe { stm32wl_hal_subghz::SubGhz::conjure() };
    /// use stm32wl_hal_subghz::{FskStats, Stats};
    ///
    /// let stats: Stats<FskStats> = sg.fsk_stats()?;
    /// // ... use stats
    /// sg.reset_stats();
    /// # Ok::<(), stm32wl_hal_subghz::SubGhzError>(())
    /// ```
    pub fn fsk_stats(&self) -> Result<Stats<FskStats>, SubGhzError> {
        let data: [u8; 7] = self.read_n(OpCode::GetStats)?;
        Ok(Stats::from_raw_fsk(data))
    }

    /// LoRa packet stats.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # let mut sg = unsafe { stm32wl_hal_subghz::SubGhz::conjure() };
    /// use stm32wl_hal_subghz::{LoRaStats, Stats};
    ///
    /// let stats: Stats<LoRaStats> = sg.lora_stats()?;
    /// // ... use stats
    /// sg.reset_stats();
    /// # Ok::<(), stm32wl_hal_subghz::SubGhzError>(())
    /// ```
    pub fn lora_stats(&self) -> Result<Stats<LoRaStats>, SubGhzError> {
        let data: [u8; 7] = self.read_n(OpCode::GetStats)?;
        Ok(Stats::from_raw_lora(data))
    }

    /// Reset the stats as reported in [`lora_stats`] and [`fsk_stats`].
    ///
    /// # Example
    ///
    /// ```no_run
    /// # let mut sg = unsafe { stm32wl_hal_subghz::SubGhz::conjure() };
    ///
    /// sg.reset_stats();
    /// # Ok::<(), stm32wl_hal_subghz::SubGhzError>(())
    /// ```
    ///
    /// [`lora_stats`]: crate::SubGhz::lora_stats
    /// [`fsk_stats`]: crate::SubGhz::fsk_stats
    pub fn reset_stats(&mut self) -> Result<(), SubGhzError> {
        const RESET_STATS: [u8; 7] = [0x00; 7];
        self.write(&RESET_STATS)
    }
}

// 5.8.6
/// IRQ commands.
impl SubGhz {
    /// Set the interrupt configuration.
    ///
    /// # Example
    ///
    /// Enable TX and timeout interrupts globally.
    ///
    /// ```no_run
    /// # let mut sg = unsafe { stm32wl_hal_subghz::SubGhz::conjure() };
    /// use stm32wl_hal_subghz::{CfgDioIrq, Irq, IrqLine};
    ///
    /// const IRQ_CFG: CfgDioIrq = CfgDioIrq::new()
    ///     .irq_enable(IrqLine::Global, Irq::TxDone)
    ///     .irq_enable(IrqLine::Global, Irq::Timeout);
    /// sg.set_irq_cfg(&IRQ_CFG)?;
    /// # Ok::<(), stm32wl_hal_subghz::SubGhzError>(())
    /// ```
    pub fn set_irq_cfg(&mut self, cfg: &CfgDioIrq) -> Result<(), SubGhzError> {
        self.write(cfg.as_slice())
    }

    /// Get the IRQ status.
    ///
    /// # Example
    ///
    /// Wait for TX to complete or time out.
    ///
    /// ```no_run
    /// # let mut sg = unsafe { stm32wl_hal_subghz::SubGhz::conjure() };
    /// use stm32wl_hal_subghz::Irq;
    ///
    /// loop {
    ///     let (_, irq_status) = sg.irq_status()?;
    ///     if irq_status & Irq::TxDone.mask() != 0 {
    ///         // handle TX done
    ///         break;
    ///     }
    ///     if irq_status & Irq::Timeout.mask() != 0 {
    ///         // handle timeout
    ///         break;
    ///     }
    /// }
    /// # Ok::<(), stm32wl_hal_subghz::SubGhzError>(())
    /// ```
    pub fn irq_status(&self) -> Result<(Status, u16), SubGhzError> {
        let data: [u8; 3] = self.read_n(OpCode::GetIrqStatus)?;
        let irq_status: u16 = u16::from_be_bytes([data[1], data[2]]);
        Ok((data[0].into(), irq_status))
    }

    /// Clear the IRQ status.
    ///
    /// # Example
    ///
    /// Clear the [`TxDone`] and [`RxDone`] interrupts.
    ///
    /// ```no_run
    /// # let mut sg = unsafe { stm32wl_hal_subghz::SubGhz::conjure() };
    /// use stm32wl_hal_subghz::Irq;
    ///
    /// sg.clear_irq_status(Irq::TxDone.mask() | Irq::RxDone.mask())?;
    /// # Ok::<(), stm32wl_hal_subghz::SubGhzError>(())
    /// ```
    ///
    /// [`TxDone`]: crate::Irq::TxDone
    /// [`RxDone`]: crate::Irq::RxDone
    pub fn clear_irq_status(&mut self, mask: u16) -> Result<(), SubGhzError> {
        self.write(&[
            crate::OpCode::ClrIrqStatus as u8,
            ((mask >> 8) & 0xFF) as u8,
            (mask & 0xFF) as u8,
        ])
    }
}

// 5.8.7
/// Miscellaneous commands
impl SubGhz {
    /// Calibrate one or several blocks at any time when in standby mode.
    ///
    /// The blocks to calibrate are defined by `cal` argument.
    /// When the calibration is ongoing, BUSY is set.
    /// A falling edge on BUSY indicates the end of all enabled calibrations.
    ///
    /// This function will not poll for BUSY.
    ///
    /// # Example
    ///
    /// Calibrate the RC 13 MHz and PLL.
    ///
    /// ```no_run
    /// # let mut sg = unsafe { stm32wl_hal_subghz::SubGhz::conjure() };
    /// use stm32wl_hal_subghz::{Calibrate, StandbyClk};
    ///
    /// sg.set_standby(StandbyClk::Rc)?;
    /// sg.calibrate(Calibrate::Rc13M.mask() | Calibrate::Pll.mask())?;
    /// while sg.rfbusys() {
    ///     // ... insert some timeout code here
    /// }
    /// # Ok::<(), stm32wl_hal_subghz::SubGhzError>(())
    /// ```
    pub fn calibrate(&mut self, cal: u8) -> Result<(), SubGhzError> {
        // bit 7 is reserved and must be kept at reset value.
        self.write(&[OpCode::Calibrate as u8, cal & 0x7F])
    }

    /// Calibrate the image at the given frequencies.
    ///
    /// Requires the radio to be in standby mode.
    ///
    /// # Example
    ///
    /// Calibrate the image for the 430 - 440 MHz ISM band.
    ///
    /// ```no_run
    /// # let mut sg = unsafe { stm32wl_hal_subghz::SubGhz::conjure() };
    /// use stm32wl_hal_subghz::{CalibrateImage, StandbyClk};
    ///
    /// sg.set_standby(StandbyClk::Rc)?;
    /// sg.calibrate_image(CalibrateImage::ISM_430_440)?;
    /// # Ok::<(), stm32wl_hal_subghz::SubGhzError>(())
    /// ```
    pub fn calibrate_image(&mut self, cal: CalibrateImage) -> Result<(), SubGhzError> {
        self.write(&[OpCode::CalibrateImage as u8, cal.0, cal.1])
    }

    /// Set the radio power supply.
    ///
    /// # Examples
    ///
    /// Use the linear dropout regulator (LDO):
    ///
    /// ```no_run
    /// # let mut sg = unsafe { stm32wl_hal_subghz::SubGhz::conjure() };
    /// use stm32wl_hal_subghz::RegMode;
    ///
    /// sg.set_regulator_mode(RegMode::Ldo)?;
    /// # Ok::<(), stm32wl_hal_subghz::SubGhzError>(())
    /// ```
    ///
    /// Use the switch mode power supply (SPMS):
    ///
    /// ```no_run
    /// # let mut sg = unsafe { stm32wl_hal_subghz::SubGhz::conjure() };
    /// use stm32wl_hal_subghz::RegMode;
    ///
    /// sg.set_regulator_mode(RegMode::Smps)?;
    /// # Ok::<(), stm32wl_hal_subghz::SubGhzError>(())
    /// ```
    pub fn set_regulator_mode(&mut self, reg_mode: RegMode) -> Result<(), SubGhzError> {
        self.write(&[OpCode::SetRegulatorMode as u8, reg_mode as u8])
    }

    /// Get the radio operational errors.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # let mut sg = unsafe { stm32wl_hal_subghz::SubGhz::conjure() };
    /// use stm32wl_hal_subghz::OpError;
    ///
    /// let (status, error_mask) = sg.op_error()?;
    /// if error_mask & OpError::PllLockError.mask() != 0 {
    ///     // ... handle PLL lock error
    /// }
    /// # Ok::<(), stm32wl_hal_subghz::SubGhzError>(())
    /// ```
    pub fn op_error(&self) -> Result<(Status, u16), SubGhzError> {
        let data: [u8; 3] = self.read_n(OpCode::GetError)?;
        Ok((data[0].into(), u16::from_le_bytes([data[1], data[2]])))
    }

    /// Clear all errors as reported by [`op_error`].
    ///
    /// # Example
    ///
    /// ```no_run
    /// # let mut sg = unsafe { stm32wl_hal_subghz::SubGhz::conjure() };
    /// use stm32wl_hal_subghz::OpError;
    ///
    /// let (status, error_mask) = sg.op_error()?;
    /// // ignore all errors
    /// if error_mask != 0 {
    ///     sg.clear_error()?;
    /// }
    /// # Ok::<(), stm32wl_hal_subghz::SubGhzError>(())
    /// ```
    ///
    /// [`op_error`]: crate::SubGhz::op_error
    pub fn clear_error(&mut self) -> Result<(), SubGhzError> {
        self.write(&[OpCode::ClrError as u8, 0x00])
    }
}

// 5.8.8
/// Set TCXO mode command
impl SubGhz {
    /// Set the TCXO trim and HSE32 ready timeout.
    ///
    /// # Example
    ///
    /// Setup the TCXO with 1.7V trim and a 10ms timeout.
    ///
    /// ```no_run
    /// # let mut sg = unsafe { stm32wl_hal_subghz::SubGhz::conjure() };
    /// use core::time::Duration;
    /// use stm32wl_hal_subghz::{TcxoMode, TcxoTrim, Timeout};
    ///
    /// const TCXO_MODE: TcxoMode = TcxoMode::new()
    ///     .set_txco_trim(TcxoTrim::Volts1pt7)
    ///     .set_timeout(Timeout::from_duration_sat(Duration::from_millis(10)));
    /// sg.set_tcxo_mode(&TCXO_MODE)?;
    /// # Ok::<(), stm32wl_hal_subghz::SubGhzError>(())
    /// ```
    pub fn set_tcxo_mode(&mut self, tcxo_mode: &TcxoMode) -> Result<(), SubGhzError> {
        self.write(tcxo_mode.as_slice())
    }
}

/// sub-GHz radio opcodes.
///
/// See Table 41 "Sub-GHz radio SPI commands overview"
#[repr(u8)]
#[allow(dead_code)]
pub(crate) enum OpCode {
    Calibrate = 0x89,
    CalibrateImage = 0x98,
    CfgDioIrq = 0x08,
    ClrError = 0x07,
    ClrIrqStatus = 0x02,
    GetError = 0x17,
    GetIrqStatus = 0x12,
    GetPacketStatus = 0x14,
    GetPacketType = 0x11,
    GetRssiInst = 0x15,
    GetRxBufferStatus = 0x13,
    GetStats = 0x10,
    GetStatus = 0xC0,
    ReadBuffer = 0x1E,
    RegRegister = 0x1D,
    ResetStats = 0x00,
    SetBufferBaseAddress = 0x8F,
    SetCad = 0xC5,
    SetCadParams = 0x88,
    SetFs = 0xC1,
    SetLoRaSymbTimeout = 0xA0,
    SetModulationParams = 0x8B,
    SetPacketParams = 0x8C,
    SetPacketType = 0x8A,
    SetPaConfig = 0x95,
    SetRegulatorMode = 0x96,
    SetRfFrequency = 0x86,
    SetRx = 0x82,
    SetRxDutyCycle = 0x94,
    SetSleep = 0x84,
    SetStandby = 0x80,
    SetStopRxTimerOnPreamble = 0x9F,
    SetTcxoMode = 0x97,
    SetTx = 0x83,
    SetTxContinuousPreamble = 0xD2,
    SetTxContinuousWave = 0xD1,
    SetTxParams = 0x8E,
    SetTxRxFallbackMode = 0x93,
    WriteBuffer = 0x0E,
    WriteRegister = 0x0D,
}

impl From<OpCode> for u8 {
    fn from(opcode: OpCode) -> Self {
        opcode as u8
    }
}

#[repr(u16)]
#[allow(dead_code)]
#[allow(clippy::upper_case_acronyms)]
pub(crate) enum Register {
    /// PA over current protection.
    PAOCP = 0x08E7,
    /// LoRa synchronization word MSB.
    LSYNCH = 0x0740,
    /// LoRa synchronization word LSB.
    LSYNCL = 0x0741,
    /// Generic synchronization word 7.
    GSYNC7 = 0x06C0,
    /// HSE32 OSC_IN capacitor trim.
    HSEINTRIM = 0x0911,
    /// HSE32 OSC_OUT capacitor trim.
    HSEOUTTRIM = 0x0912,
}

impl Register {
    pub const fn address(self) -> u16 {
        self as u16
    }
}

/// Power amplifier ramp time for FSK, MSK, and LoRa modulation.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum RampTime {
    /// 10µs
    Micros10 = 0x00,
    /// 20µs
    Micros20 = 0x01,
    /// 40µs
    Micros40 = 0x02,
    /// 80µs
    Micros80 = 0x03,
    /// 200µs
    Micros200 = 0x04,
    /// 800µs
    Micros800 = 0x05,
    /// 1.7ms
    Micros1700 = 0x06,
    /// 3.4ms
    Micros3400 = 0x07,
}

impl From<RampTime> for u8 {
    fn from(rt: RampTime) -> Self {
        rt as u8
    }
}

impl From<RampTime> for core::time::Duration {
    fn from(rt: RampTime) -> Self {
        match rt {
            RampTime::Micros10 => core::time::Duration::from_micros(10),
            RampTime::Micros20 => core::time::Duration::from_micros(20),
            RampTime::Micros40 => core::time::Duration::from_micros(40),
            RampTime::Micros80 => core::time::Duration::from_micros(80),
            RampTime::Micros200 => core::time::Duration::from_micros(200),
            RampTime::Micros800 => core::time::Duration::from_micros(800),
            RampTime::Micros1700 => core::time::Duration::from_micros(1700),
            RampTime::Micros3400 => core::time::Duration::from_micros(3400),
        }
    }
}
