use crate::hardware_initialisation::{HardwareControls, WifiState};
use esp_println::println;
use esp_radio::wifi::{
    sta::StationConfig, AuthenticationMethod, Config, ControllerConfig, WifiController,
};

const SSID: &str = "UoA-Unleash";
const PASSWORD: &str = "UoA_Unl3ash";

pub async fn connect_wifi(mut controls: HardwareControls) -> HardwareControls {
    let wifi_peripheral = match controls.wifi {
        WifiState::NotConnected(p) => p,
        WifiState::Connected(_) => return controls,
    };

    let station_config = Config::Station(
        StationConfig::default()
            .with_ssid(SSID)
            .with_password(PASSWORD.into())
            .with_auth_method(AuthenticationMethod::None),
    );

    println!("Starting WiFi");

    let mut controller = WifiController::new(
        wifi_peripheral,
        ControllerConfig::default().with_initial_config(station_config),
    )
    .unwrap();

    println!("Wifi configured and started!");

    match controller.connect_async().await {
        Ok(info) => println!("Wifi connected to {:?}", info),
        Err(e) => println!("Failed to connect to wifi: {e:?}"),
    }

    controls.wifi = WifiState::Connected(controller);
    controls
}
