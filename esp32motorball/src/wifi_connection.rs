use crate::hardware_initialisation::{HardwareControls, WifiState};

const SSID: &str = "";
const PASSWORD: &str = "";

pub fn connect_wifi(mut controls: HardwareControls) -> Result<HardwareControls, ()> {
    let _ = SSID;
    let _ = PASSWORD;

    controls.wifi = WifiState::Connected;
    Ok(controls)
}
