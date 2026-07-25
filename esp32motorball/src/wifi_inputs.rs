use crate::hardware_initialisation::HardwareControls;
use crate::wifi_connection::ConnectionControls;
use crate::motor_driver::MotorAction;
use crate::led_driver::LedAction;

const IP_AND_PORT: &str = env!("SERVER_IP_AND_PORT");

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

pub async fn poll_server(connection: ConnectionControls) -> ServerResult {
    let mut motor = MotorAction::Stop;
    let mut led = LedAction::Off;

    let mut response_buffer = [0u8; 1024];
    let url = format!("{}/api/motor", IP_AND_PORT);
    println!("{}", url);

    let response = client
        .request(Method::GET, &url)
        .await
        .unwrap()
        .content_type(ContentType::TextPlain)
        .send(&mut response_buffer)
        .await
        .unwrap();

    match response {
        Response::Ok(body) => {
            let body_str = core::str::from_utf8(body).unwrap();

            match body_str {
                "1" => motor = MotorAction::Start,
                "0" => motor = MotorAction::Stop,
                _ => println!("Unknown command received: {}", body_str),
            }
        }
        Response::Error(status) => {
            println!("Error: {}", status);
        }
    }

    return ServerResult {
        motor_action: motor,
        led_action: led,
    };
}
