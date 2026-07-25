use crate::wifi_connection::{ConnectionControls, HttpConnection};

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

pub async fn poll_server(connection: &mut ConnectionControls) -> ServerResult {
    let mut motor = MotorAction::Stop;
    let mut led = LedAction::Off;

    let client = match &mut connection.http {
        HttpConnection::Connected(client) => client,
        HttpConnection::NotConnected => {
            return ServerResult {
                motor_action: motor,
                led_action: led,
            };
        }
    };

    let mut response_buffer = [0u8; 1024];
    let url = format!("{}/api/motor", IP_AND_PORT);
    println!("{}", url);

    let request = match client.request(Method::GET, &url).await {
        Ok(r) => r,
        Err(_) => {
            return ServerResult {
                motor_action: motor,
                led_action: led,
            };
        }
    };

    let response = match request
        .content_type(ContentType::TextPlain)
        .send(&mut response_buffer)
        .await
    {
        Ok(r) => r,
        Err(_) => {
            return ServerResult {
                motor_action: motor,
                led_action: led,
            };
        }
    };

    match response {
        Response::Ok(body) => {
            let body_str = match core::str::from_utf8(body) {
                Ok(s) => s,
                Err(_) => {
                    return ServerResult {
                        motor_action: motor,
                        led_action: led,
                    };
                }
            };

            match body_str {
                "1" => {
                    motor = MotorAction::Start;
                    led = LedAction::Blink;
                }
                "0" => {
                    motor = MotorAction::Stop;
                    led = LedAction::Blink;
                }
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
