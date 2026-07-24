#![no_std]
#![no_main]

use esp_println::println;

const SSID: &str = "";
const PASSWORD: &str = "";

#[esp_rtos::main]
async fn main(_spawner: Spawner) -> ! {
    println!("Connecting to WIFI...");

    
}
