//! Arduino Uno servo driver — genuine AVR firmware (runs in Wokwi and on a real Uno).
//!
//! Bit-bangs a standard 50 Hz hobby-servo signal on pin D9 and continuously
//! sweeps the horn from 0° to 180° and back.

#![no_std]
#![no_main]

use panic_halt as _;

/// Standard hobby servo timing: 1000 µs pulse = 0°, 2000 µs = 180°.
fn angle_to_us(angle: u8) -> u16 {
    // u32 intermediate: `angle * 1000` overflows u16 past ~65°.
    1000 + (angle as u32 * 1000 / 180) as u16
}

#[arduino_hal::entry]
fn main() -> ! {
    let dp = arduino_hal::Peripherals::take().unwrap();
    let pins = arduino_hal::pins!(dp);

    // Servo signal on D9.
    let mut servo = pins.d9.into_output();

    let mut angle: i16 = 0;
    let mut step: i16 = 5;

    loop {
        let pulse_us = angle_to_us(angle as u8) as u32;

        // One 20 ms servo frame: HIGH for the pulse width, LOW for the rest.
        servo.set_high();
        arduino_hal::delay_us(pulse_us);
        servo.set_low();
        arduino_hal::delay_us(20_000u32 - pulse_us);

        // Advance the sweep, bouncing between the end stops.
        angle += step;
        if angle >= 180 {
            angle = 180;
            step = -5;
        } else if angle <= 0 {
            angle = 0;
            step = 5;
        }
    }
}
