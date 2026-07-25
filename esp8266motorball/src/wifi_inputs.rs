pub struct ServerResult {
    pub motor_action: MotorAction,
    pub led_action: LedAction,
}

pub enum MotorAction {
    Stop,
    Start,
}

pub enum LedAction {
    Off,
    Blink,
}

pub async fn poll_server() -> ServerResult {
    ServerResult {
        motor_action: MotorAction::Start,
        led_action: LedAction::Blink,
    }
}
