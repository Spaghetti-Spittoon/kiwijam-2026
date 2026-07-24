#![no_std]
#![no_main]

mod hardware_initialisation;
mod hardware_inputs;
mod led_driver;
mod motor_driver;
mod wifi_connection;
mod wifi_inputs;

use panic_halt as _;

use crate::hardware_initialisation::initialise_hardware;
use crate::hardware_inputs::read_tilt_sensor;
use crate::led_driver::drive_led;
use crate::motor_driver::drive_motors;
use crate::wifi_connection::connect_wifi;
use crate::wifi_inputs::poll_server;

#[unsafe(no_mangle)]
pub extern "C" fn call_user_start() -> ! {
    run()
}

fn run() -> ! {
    let controller = initialise_hardware().unwrap();
    let _controller = connect_wifi(controller).unwrap();

    loop {
        let server_result = poll_server();
        let tilt = read_tilt_sensor();
        drive_motors(server_result.motor_action, tilt);
        drive_led(server_result.led_action);
        delay_ms(5000);
    }
}

fn delay_ms(ms: u32) {
    let iterations = ms.saturating_mul(10_000);
    for _ in 0..iterations {
        core::hint::spin_loop();
    }
}
