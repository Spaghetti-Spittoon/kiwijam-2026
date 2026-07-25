use crate::hardware_initialisation::{HardwareControls, WifiState};
use esp_println::println;
use esp_radio::wifi::{
    sta::eap::{EapStationConfig, TtlsPhase2Method},
    AuthenticationMethod, Config, ControllerConfig, WifiController,
};

const SSID: &str = env!("WIFI_SSID");
const USERNAME: &str = env!("WIFI_USERNAME");
const PASSWORD: &str = env!("WIFI_PASSWORD");
const IDENTITY: &str = "anonymous";

pub async fn connect_wifi(mut controls: HardwareControls) -> HardwareControls {
    let wifi_peripheral = match controls.wifi {
        WifiState::NotConnected(p) => p,
        WifiState::Connected(_) => return controls,
    };

    let eap_config = EapStationConfig::default()
        .with_ssid(SSID)
        .with_auth_method(AuthenticationMethod::Wpa2Enterprise)
        .with_identity(IDENTITY.into())
        .with_username(USERNAME.into())
        .with_password(PASSWORD.into())
        .with_ttls_phase2_method(TtlsPhase2Method::Mschapv2);

    let station_config = Config::EapStation(eap_config);

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
