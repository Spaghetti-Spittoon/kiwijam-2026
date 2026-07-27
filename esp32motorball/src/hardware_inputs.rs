use crate::hardware_initialisation::HardwareControls;
use mma8x5x::Measurement;

pub enum TiltSensation {
    Idle,
    TiltingLeft,
    TiltingRight,
    TiltingForward,
    TiltingBack,
}

const TILT_THRESHOLD_G: f32 = 0.3;

pub async fn read_tilt_sensor(hardware: &mut HardwareControls) -> TiltSensation {
    let Some(sensor) = &mut hardware.tilt_sensor else {
        return TiltSensation::Idle;
    };

    match sensor.read() {
        Ok(measurement) => classify(measurement),
        Err(_) => TiltSensation::Idle,
    }
}

fn classify(m: Measurement) -> TiltSensation {
    let ax = abs(m.x);
    let ay = abs(m.y);

    if ax > TILT_THRESHOLD_G && ax >= ay {
        if m.x > 0.0 {
            TiltSensation::TiltingRight
        } else {
            TiltSensation::TiltingLeft
        }
    } else if ay > TILT_THRESHOLD_G {
        if m.y > 0.0 {
            TiltSensation::TiltingForward
        } else {
            TiltSensation::TiltingBack
        }
    } else {
        TiltSensation::Idle
    }
}

fn abs(v: f32) -> f32 {
    if v < 0.0 { -v } else { v }
}