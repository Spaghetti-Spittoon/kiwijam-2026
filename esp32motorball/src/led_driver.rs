use embassy_time::Instant;
use esp_hal::gpio::Level;

use crate::hardware_initialisation::HardwareControls;
use crate::wifi_inputs::LedAction;

pub async fn drive_led(hardware: &mut HardwareControls, input: LedAction) {
    match input {
        LedAction::Off => hardware.led.set_low(),
        LedAction::Blink => {
            let phase = (Instant::now().as_millis() / 1000) % 2;
            let level = if phase == 0 { Level::High } else { Level::Low };
            hardware.led.set_level(level);
        }
    }
}
