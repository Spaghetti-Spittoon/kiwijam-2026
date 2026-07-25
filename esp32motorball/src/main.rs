#![no_std]
#![no_main]

mod hardware_initialisation;
mod hardware_inputs;
mod led_driver;
mod motor_driver;
mod wifi_connection;
mod wifi_inputs;

use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};
use esp_alloc as _;
use esp_backtrace as _;
use esp_println::println;

use crate::hardware_initialisation::initialise_hardware;
use crate::hardware_inputs::read_tilt_sensor;
use crate::led_driver::drive_led;
use crate::motor_driver::drive_motors;
use crate::wifi_connection::connect_wifi;
use crate::wifi_inputs::poll_server;

esp_bootloader_esp_idf::esp_app_desc!();

#[esp_rtos::main]
async fn main(_spawner: Spawner) -> ! {
    let hardware = initialise_hardware();
    let (mut hardware, mut connection) = connect_wifi(hardware).await;

    println!("motorball online");

    loop {
        let server_result = poll_server(&mut connection).await;
        let tilt = read_tilt_sensor().await;
        drive_motors(&hardware, server_result.motor_action, tilt).await;
        drive_led(&mut hardware, server_result.led_action).await;
        Timer::after(Duration::from_millis(100)).await;
    }
}
