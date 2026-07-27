use crate::{hardware_initialisation::HardwareControls, hardware_inputs::TiltSensation, wifi_inputs::MotorAction};
use esp_println::println;

pub async fn drive_motors(hardware: &mut HardwareControls, input: MotorAction, tilt: TiltSensation) {
    match input {
        MotorAction::Stop => stop_motors(hardware).await,
        MotorAction::Start => start_motors(hardware, tilt).await,
    }
}

async fn stop_motors(hardware: &mut HardwareControls) {
    println!("stopping motors");
    hardware.motor_left.set_low();
    hardware.motor_right.set_low();
}

async fn start_motors(hardware: &mut HardwareControls, tilt: TiltSensation) {
    match tilt {
        TiltSensation::Idle => {
            println!("moving forwards");
            hardware.motor_left.set_high();
            hardware.motor_right.set_high();
        }
        TiltSensation::TiltingLeft => {
            println!("moving left");
            hardware.motor_left.set_low();
            hardware.motor_right.set_high();
        }
        TiltSensation::TiltingRight => {
            println!("moving right");
            hardware.motor_left.set_high();
            hardware.motor_right.set_low();
        }
        TiltSensation::TiltingForward => {
            println!("moving forwards");
            hardware.motor_left.set_high();
            hardware.motor_right.set_high();
        }
        TiltSensation::TiltingBack => {
            println!("turning around");
            hardware.motor_left.set_high();
            hardware.motor_right.set_high();
        }
    }
}
