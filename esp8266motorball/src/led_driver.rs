
use crate::wifi_inputs::LedAction;

pub fn drive_led(input: LedAction) {
    match input {
        LedAction::Off => (),
        LedAction::Blink => {}
    }
}
