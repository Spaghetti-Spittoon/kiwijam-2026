pub enum TiltSensation {
    Idle,
    TiltingLeft,
    TiltingRight,
    TiltingForward,
    TiltingBack,
}

pub async fn read_tilt_sensor() -> TiltSensation {
    TiltSensation::Idle
}