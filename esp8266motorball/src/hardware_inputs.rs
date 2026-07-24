pub enum TiltSensation {
    Idle,
    TiltingLeft,
    TiltingRight,
    TiltingForward,
    TiltingBack,
}

pub fn read_tilt_sensor() -> TiltSensation {
    TiltSensation::Idle
}