use crate::wifi_connection::{ConnectionControls, HttpConnection};
use esp_println::println;
use reqwless::headers::ContentType;
use reqwless::request::{Method, RequestBuilder};

const URL: &str = concat!("http://", env!("SERVER_IP_AND_PORT"), "/api/motor");

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
        HttpConnection::Connected(client) => &mut **client,
        HttpConnection::NotConnected => {
            return ServerResult {
                motor_action: motor,
                led_action: led,
            };
        }
    };

    let mut response_buffer = [0u8; 1024];

    let mut handle = match client.request(Method::GET, URL).await {
        Ok(h) => h.content_type(ContentType::TextPlain),
        Err(e) => {
            println!("request build failed: {e:?}");
            return ServerResult {
                motor_action: motor,
                led_action: led,
            };
        }
    };

    let response = match handle.send(&mut response_buffer).await {
        Ok(r) => r,
        Err(e) => {
            println!("request send failed: {e:?}");
            return ServerResult {
                motor_action: motor,
                led_action: led,
            };
        }
    };

    if !response.status.is_successful() {
        println!("http error: {:?}", response.status);
        return ServerResult {
            motor_action: motor,
            led_action: led,
        };
    }

    let body = match response.body().read_to_end().await {
        Ok(b) => b,
        Err(e) => {
            println!("body read failed: {e:?}");
            return ServerResult {
                motor_action: motor,
                led_action: led,
            };
        }
    };

    let body_str = match core::str::from_utf8(body) {
        Ok(s) => s.trim(),
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
        other => println!("Unknown command received: {}", other),
    }

    ServerResult {
        motor_action: motor,
        led_action: led,
    }
}
