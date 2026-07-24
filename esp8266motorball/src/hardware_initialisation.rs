use esp8266::Peripherals;

pub struct HardwareControls {
    pub wifi: WifiState,
    pub peripherals: Peripherals,
}

pub enum WifiState {
    NotConnected,
    Connected,
}

pub fn initialise_hardware() -> Result<HardwareControls, ()> {
    let peripherals = Peripherals::take().ok_or(())?;

    Ok(HardwareControls {
        wifi: WifiState::NotConnected,
        peripherals,
    })
}
