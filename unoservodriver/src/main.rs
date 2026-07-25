//! Arduino Uno servo driver — genuine AVR firmware (runs in Wokwi and on a real Uno).
//!
//! Produces a standard 50 Hz hobby-servo signal on D9.  A control pulse on D2
//! selects the angle: 1000 µs means 0° and 2000 µs means 180°.
//!
//! # Pi-to-Uno wiring / protocol — must be confirmed before connecting hardware
//!
//! This is an initial, deliberately conservative contract so firmware can be
//! tested before the final electronics design is agreed:
//!
//! - Pi GPIO (PWM output) -> Uno D2.  The Pi must emit one positive pulse every
//!   20 ms (50 Hz), with a width from 1000 to 2000 µs.
//! - Pi GND -> Uno GND.  A shared ground is required; do *not* connect the Pi
//!   GPIO to the Uno without it.
//! - Uno D9 -> servo signal; power the servo from an appropriately rated 5 V
//!   supply and connect that supply's ground to Uno/Pi ground.
//! - A 3.3 V Pi GPIO is not guaranteed to meet the ATmega328P's HIGH threshold
//!   when the Uno runs at 5 V.  The final circuit therefore needs a validated
//!   3.3 V-to-5 V level shifter/buffer (or a confirmed 3.3 V Uno design).
//!
//! Do not treat D2 as a TTL/UART serial input: it measures servo-style PWM.
//! If the intended Pi protocol is UART, I2C, or separate direction buttons,
//! this interface and wiring need to be redesigned.

#![no_std]
#![no_main]

use panic_halt as _;

const FRAME_US: u32 = 20_000;
const MIN_CONTROL_US: u16 = 1_000;
const MAX_CONTROL_US: u16 = 2_000;
const SAMPLE_US: u16 = 10;
const CONTROL_TIMEOUT_US: u16 = 2_500;

/// Samples a HIGH control pulse on D2.  The short timeout prevents a broken
/// or disconnected control wire from preventing servo frames forever.
fn read_control_pulse_us<PIN: arduino_hal::port::PinOps>(
    control: &arduino_hal::port::Pin<
        arduino_hal::port::mode::Input<arduino_hal::port::mode::Floating>,
        PIN,
    >,
) -> Option<u16> {
    let mut waited = 0;
    while control.is_low() {
        if waited >= CONTROL_TIMEOUT_US {
            return None;
        }
        arduino_hal::delay_us(SAMPLE_US as u32);
        waited += SAMPLE_US;
    }

    let mut width = 0;
    while control.is_high() {
        if width >= CONTROL_TIMEOUT_US {
            return None;
        }
        arduino_hal::delay_us(SAMPLE_US as u32);
        width += SAMPLE_US;
    }

    (MIN_CONTROL_US..=MAX_CONTROL_US).contains(&width).then_some(width)
}

/// Maps the confirmed Pi PWM contract to a servo pulse, clamping sampling
/// jitter at the endpoints.
fn control_to_servo_us(control_us: u16) -> u16 {
    control_us.clamp(MIN_CONTROL_US, MAX_CONTROL_US)
}

#[arduino_hal::entry]
fn main() -> ! {
    let dp = arduino_hal::Peripherals::take().unwrap();
    let pins = arduino_hal::pins!(dp);

    // Servo signal on D9.  D2 is the provisional Pi PWM-control input; see
    // the module documentation before wiring it to physical hardware.
    let mut servo = pins.d9.into_output();
    let control = pins.d2.into_floating_input();

    // Fail-safe position for a missing or malformed control signal.  This
    // should be reviewed with the mechanical team: “centre” is not always the
    // safest position for the final game mechanism.
    let mut servo_us: u16 = 1_500;

    loop {
        if let Some(control_us) = read_control_pulse_us(&control) {
            servo_us = control_to_servo_us(control_us);
        }

        // One 20 ms servo frame: HIGH for the pulse width, LOW for the rest.
        servo.set_high();
        arduino_hal::delay_us(servo_us as u32);
        servo.set_low();
        arduino_hal::delay_us(FRAME_US - servo_us as u32);
    }
}
