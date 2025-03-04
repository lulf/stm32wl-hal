// Blinks the 3 LEDs on the NUCLEO-WL55JC2 in a sequence.

#![no_std]
#![no_main]

use defmt_rtt as _; // global logger
use panic_probe as _; // panic handler
use stm32wl_hal::{
    self as hal,
    cortex_m::delay::Delay,
    gpio::{Level, Output, PortB},
    pac,
    util::new_delay,
};

#[hal::cortex_m_rt::entry]
fn main() -> ! {
    let mut dp: pac::Peripherals = defmt::unwrap!(pac::Peripherals::take());
    let cp: pac::CorePeripherals = defmt::unwrap!(pac::CorePeripherals::take());

    let gpiob: PortB = PortB::split(dp.GPIOB, &mut dp.RCC);
    let mut led1 = Output::default(gpiob.b9);
    let mut led2 = Output::default(gpiob.b15);
    let mut led3 = Output::default(gpiob.b11);

    let mut delay: Delay = new_delay(cp.SYST, &dp.RCC);

    defmt::info!("Starting blinky");

    loop {
        for &level in &[Level::High, Level::Low] {
            led1.set_level(level);
            delay.delay_ms(600);
            led2.set_level(level);
            delay.delay_ms(600);
            led3.set_level(level);
            delay.delay_ms(600);
        }
    }
}
