//! Reconnecting USB serial transmitter for the two-player Uno servo firmware.

use std::io;

#[cfg(any(target_os = "linux", test))]
const SYNC: u8 = 0xA5;
#[cfg(any(target_os = "linux", test))]
const PACKET_LEN: usize = 5;
#[cfg(any(target_os = "linux", test))]
const SERVO_MAX: i32 = 255;

#[cfg(any(target_os = "linux", test))]
fn crc8(bytes: &[u8]) -> u8 {
    let mut crc = 0;
    for &byte in bytes {
        crc ^= byte;
        for _ in 0..8 {
            crc = if crc & 0x80 != 0 {
                (crc << 1) ^ 0x07
            } else {
                crc << 1
            };
        }
    }
    crc
}

#[cfg(any(target_os = "linux", test))]
fn inverted_servo_value(value: i32) -> u8 {
    (SERVO_MAX - value.clamp(0, SERVO_MAX)) as u8
}

#[cfg(any(target_os = "linux", test))]
fn packet(sequence: u8, player1: i32, player2: i32) -> [u8; PACKET_LEN] {
    let mut frame = [
        SYNC,
        sequence,
        inverted_servo_value(player1),
        inverted_servo_value(player2),
        0,
    ];
    frame[PACKET_LEN - 1] = crc8(&frame[..PACKET_LEN - 1]);
    frame
}

#[cfg(target_os = "linux")]
pub fn spawn<F>(read_values: F) -> io::Result<()>
where
    F: Fn() -> (i32, i32) + Send + 'static,
{
    use std::io::Write;
    use std::time::Duration;

    const BAUD: u32 = 115_200;
    const HEARTBEAT: Duration = Duration::from_millis(50);
    const RECONNECT_DELAY: Duration = Duration::from_secs(1);
    const UNO_BOOT_DELAY: Duration = Duration::from_secs(2);

    let path = std::env::var("PIWS_UNO_SERIAL").unwrap_or_else(|_| "/dev/ttyACM0".to_string());

    std::thread::Builder::new()
        .name("uno-serial".into())
        .spawn(move || {
            let mut sequence = 0u8;
            loop {
                let mut port = match serialport::new(&path, BAUD)
                    .timeout(Duration::from_millis(250))
                    .dtr_on_open(true)
                    .exclusive(true)
                    .open()
                {
                    Ok(port) => port,
                    Err(error) => {
                        eprintln!("Uno serial: cannot open {path} ({error}); retrying");
                        std::thread::sleep(RECONNECT_DELAY);
                        continue;
                    }
                };

                // Opening an Uno USB serial device resets its ATmega328P.
                eprintln!("Uno serial: connected to {path} at {BAUD} baud; waiting for bootloader");
                std::thread::sleep(UNO_BOOT_DELAY);

                loop {
                    let (player1, player2) = read_values();
                    let frame = packet(sequence, player1, player2);
                    if let Err(error) = port.write_all(&frame) {
                        eprintln!("Uno serial: write failed ({error}); reconnecting");
                        break;
                    }
                    sequence = sequence.wrapping_add(1);
                    std::thread::sleep(HEARTBEAT);
                }
            }
        })
        .map(|_| ())
        .map_err(|error| io::Error::other(format!("cannot spawn Uno serial thread: {error}")))
}

#[cfg(not(target_os = "linux"))]
pub fn spawn<F>(_read_values: F) -> io::Result<()>
where
    F: Fn() -> (i32, i32) + Send + 'static,
{
    eprintln!("Uno serial: transmitter is only enabled on Linux");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{SYNC, crc8, packet};

    #[test]
    fn builds_a_valid_packet() {
        let frame = packet(7, 0, 255);
        assert_eq!(frame[0], SYNC);
        assert_eq!(&frame[1..4], &[7, 255, 0]);
        assert_eq!(frame[4], crc8(&frame[..4]));
    }

    #[test]
    fn clamps_control_values() {
        let frame = packet(0, -1, 256);
        assert_eq!(&frame[2..4], &[255, 0]);
    }

    #[test]
    fn inverts_both_servo_channels_around_centre() {
        assert_eq!(&packet(0, 64, 192)[2..4], &[191, 63]);
        assert_eq!(&packet(0, 127, 128)[2..4], &[128, 127]);
    }
}
