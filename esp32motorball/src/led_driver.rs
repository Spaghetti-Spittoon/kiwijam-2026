use crate::wifi_inputs::LedAction;

pub async fn drive_led(input: LedAction) {
    match input {
        LedAction::Off => (),
        LedAction::Blink => {}
    }
}
