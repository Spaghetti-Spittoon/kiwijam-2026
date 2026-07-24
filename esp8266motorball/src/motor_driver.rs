use crate::{hardware_inputs::TiltSensation, wifi_inputs::MotorAction};

pub fn drive_motors(input: MotorAction, tilt: TiltSensation) {
    match input {
        MotorAction::Stop => stop_motors(),
        MotorAction::Start => start_motors(tilt),
    }
}

fn stop_motors() {}

fn start_motors(tilt: TiltSensation) {
    match tilt {
        TiltSensation::Idle => {}
        TiltSensation::TiltingLeft => {}
        TiltSensation::TiltingRight => {}
        TiltSensation::TiltingForward => {}
        TiltSensation::TiltingBack => {}
    }
}
