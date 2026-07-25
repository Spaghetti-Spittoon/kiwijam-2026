#![no_std]
#![no_main]

use arduino_hal::prelude::*;
use panic_halt as _;

#[arduino_hal::entry]
fn main() -> ! {
    let dp = arduino_hal::Peripherals::take().unwrap();
    let mut pins = arduino_hal::pins!(dp);

    let mut led = pins.d13.into_output();

    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);

    let mut uart = Uart::new(
        peripherals.UART0,
        Config::default(),
        peripherals.GPIO1,
        peripherals.GPIO3,
    )
    .unwrap();
    writeln!(uart, "Hello world").ok();

    loop {
        led.toggle();
        arduino_hal::delay_ms(1000);
    }
}
