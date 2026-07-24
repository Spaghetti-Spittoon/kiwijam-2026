#![no_std]
#![no_main]

use esp_println::println;

const SSID: &str = "";
const PASSWORD: &str = "";

#[esp_rtos::main]
async fn main(_spawner: Spawner) -> ! {
    println!("Creating config");

    let config = esp_hal::Config::default()
        .with_cpu_clock(CpuClock::max());

    let peripherals = esp_hal:: init(config);
    const HEAP_SIZE: usize = 72 * 1024;
    esp_alloc::heap_allocator!(size: HEAP_SIZE);

    let timer_timg0 = TimerGroup::new(peripherals.TIMG0);
    let sw_int = SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);

    esp_rtos::start(timer_timg0.timer0, sw_int.software_interrupt0);
    
    let station_builder = StationConfig::default()
        .with_ssid(SSID)
        .with_password(PASSWORD.into())
        .with_auth_method(AuthenticationMethod::None);

    let station_config: Config::Station(station_builder);

    println!("Starting WIFI driver");
    let controller_config = ControllerConfig::default()
        .with_initial_config(station_config);

    let mut maybe_controller = WifiController::new(peripherals.WIFI, controller_config);
    let controller = maybe_controller.unwrap();


    println!("WIFI driver started");

    loop {
        println!("connecting");
        let maybe_connection = controller.connect_async().await;
        
        match maybe_connection {
            Ok(info) => {
                println!("Connected to WIFI");
            },
            Err(e) => {
                panic!("Failed to connect to WIFI: {:?}", e);
            }
        }
        Timer::after(Duration::from_millis(5000)).await;
    }
}
