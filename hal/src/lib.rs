//! STM32WL Hardware Abstraction Layer.
//!
//! This documentation was generated for the
#![cfg_attr(feature = "stm32wl5x_cm0p", doc = "STM32WL5X (CM0+ core).")]
#![cfg_attr(feature = "stm32wl5x_cm4", doc = "STM32WL5X (CM4 core).")]
#![cfg_attr(feature = "stm32wl5x_cm0p", doc = "STM32WLE5.")]
//!
#![cfg_attr(not(test), no_std)]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![warn(missing_docs)]

#[cfg(any(
    all(feature = "stm32wl5x_cm0p", feature = "stm32wl5x_cm4"),
    all(feature = "stm32wl5x_cm0p", feature = "stm32wle5"),
    all(feature = "stm32wl5x_cm4", feature = "stm32wle5"),
))]
compile_error!(
    "Multile chip features activated. \
    You must activate exactly one of the following features: \
    stm32wl5x_cm0p, stm32wl5x_cm4, stm32wle5"
);

cfg_if::cfg_if! {
    if #[cfg(feature = "stm32wl5x_cm0p")] {
        /// Peripheral access crate
        pub use stm32wl::stm32wl5x_cm0p as pac;
    } else if #[cfg(feature = "stm32wl5x_cm4")] {
        /// Peripheral access crate
        pub use stm32wl::stm32wl5x_cm4 as pac;
    } else if #[cfg(feature = "stm32wle5")] {
        /// Peripheral access crate
        pub use stm32wl::stm32wle5 as pac;
    } else {
        core::compile_error!("You must select your hardware with a feature flag");
    }
}

// This mod MUST go first, so that the others see its macros.
pub(crate) mod fmt;
pub(crate) mod macros;

pub mod adc;
pub mod aes;
pub mod dac;
pub mod dma;
pub mod gpio;
pub mod i2c;
pub mod info;
pub mod lptim;
pub mod pka;
pub mod pwr;
pub mod rcc;
pub mod rng;
pub mod rtc;
pub mod spi;
pub mod subghz;
pub mod uart;
pub mod util;

mod ratio;
pub use ratio::Ratio;

#[cfg(feature = "rt")]
#[cfg_attr(docsrs, doc(cfg(feature = "rt")))]
pub use cortex_m_rt;

pub use chrono;
pub use cortex_m;
pub use embedded_hal;
pub use embedded_time;
