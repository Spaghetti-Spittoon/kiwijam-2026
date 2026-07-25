//! Raspberry Pi hardware-PWM transmitter for the two Uno control inputs.
//!
//! PWM0 is routed to BCM GPIO18 (physical pin 12) for Player 1, and PWM1 is
//! routed to BCM GPIO19 (physical pin 35) for Player 2. The Pi device-tree
//! overlay determines that pin routing; see `piwebserver/README.md`.

use std::io;

#[cfg(any(target_os = "linux", test))]
const MIN_PULSE_US: u64 = 1_000;
#[cfg(any(target_os = "linux", test))]
const PULSE_RANGE_US: u64 = 1_000;

/// Maps the web server's 0..=255 control value onto a 1000..=2000 us pulse.
///
/// Integer division intentionally follows the requested linear formula:
/// `1000 + value * 1000 / 255`.
#[cfg(any(target_os = "linux", test))]
fn pulse_width_us(value: i32) -> u64 {
    MIN_PULSE_US + value.clamp(0, 255) as u64 * PULSE_RANGE_US / 255
}

#[cfg(target_os = "linux")]
pub fn spawn<F>(read_values: F) -> io::Result<()>
where
    F: Fn() -> (i32, i32) + Send + 'static,
{
    use std::time::Duration;

    use rppal::pwm::{Channel, Polarity, Pwm};

    const PERIOD: Duration = Duration::from_millis(20);

    if std::env::var_os("PIWS_DISABLE_SERVO_PWM").is_some() {
        eprintln!("servo PWM: disabled by PIWS_DISABLE_SERVO_PWM");
        return Ok(());
    }

    let (initial_p1, initial_p2) = read_values();
    let p1 = Pwm::with_period(
        Channel::Pwm0,
        PERIOD,
        Duration::from_micros(pulse_width_us(initial_p1)),
        Polarity::Normal,
        true,
    )
    .map_err(|error| {
        io::Error::other(format!("cannot start Player 1 PWM0 on BCM GPIO18: {error}"))
    })?;
    let p2 = Pwm::with_period(
        Channel::Pwm1,
        PERIOD,
        Duration::from_micros(pulse_width_us(initial_p2)),
        Polarity::Normal,
        true,
    )
    .map_err(|error| {
        io::Error::other(format!("cannot start Player 2 PWM1 on BCM GPIO19: {error}"))
    })?;

    std::thread::Builder::new()
        .name("servo-pwm".into())
        .spawn(move || {
            eprintln!("servo PWM: P1 BCM18/PWM0 -> Uno D2; P2 BCM19/PWM1 -> Uno D3 (50 Hz)");
            let mut previous = (initial_p1, initial_p2);
            loop {
                let current = read_values();
                if current.0 != previous.0 {
                    if let Err(error) =
                        p1.set_pulse_width(Duration::from_micros(pulse_width_us(current.0)))
                    {
                        eprintln!("servo PWM: Player 1 update failed ({error}); outputs disabled");
                        return;
                    }
                }
                if current.1 != previous.1 {
                    if let Err(error) =
                        p2.set_pulse_width(Duration::from_micros(pulse_width_us(current.1)))
                    {
                        eprintln!("servo PWM: Player 2 update failed ({error}); outputs disabled");
                        return;
                    }
                }
                previous = current;

                // This only controls how quickly new values reach the peripheral.
                // The PWM peripheral itself continuously generates the stable signal.
                std::thread::sleep(Duration::from_millis(5));
            }
        })
        .map(|_| ())
        .map_err(|error| io::Error::other(format!("cannot spawn servo PWM thread: {error}")))
}

#[cfg(not(target_os = "linux"))]
pub fn spawn<F>(_read_values: F) -> io::Result<()>
where
    F: Fn() -> (i32, i32) + Send + 'static,
{
    eprintln!("servo PWM: Raspberry Pi hardware output is only enabled on Linux");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::pulse_width_us;

    #[test]
    fn maps_full_control_range_to_servo_pulses() {
        assert_eq!(pulse_width_us(0), 1_000);
        assert_eq!(pulse_width_us(128), 1_501);
        assert_eq!(pulse_width_us(255), 2_000);
    }

    #[test]
    fn clamps_values_before_mapping() {
        assert_eq!(pulse_width_us(-1), 1_000);
        assert_eq!(pulse_width_us(256), 2_000);
    }
}
