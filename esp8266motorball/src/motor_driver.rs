use crate::{hardware_inputs::TiltSensation, wifi_inputs::MotorAction};
use esp_println::println;

pub async fn drive_motors(input: MotorAction, tilt: TiltSensation) {
    match input {
        MotorAction::Stop => stop_motors().await,
        MotorAction::Start => start_motors(tilt).await,
    }
}

async fn stop_motors() {
    println!("stopping motors");
}

async fn start_motors(tilt: TiltSensation) {
    match tilt {
        TiltSensation::Idle => println!("moving forwards"),
        TiltSensation::TiltingLeft => println!("moving left"),
        TiltSensation::TiltingRight => println!("moving right"),
        TiltSensation::TiltingForward => println!("moving forwards"),
        TiltSensation::TiltingBack => println!("turning around"),
    }
}
