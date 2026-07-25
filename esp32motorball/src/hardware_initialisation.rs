use esp_hal::clock::CpuClock;
use esp_hal::gpio::{Level, Output, OutputConfig};
use esp_hal::interrupt::software::SoftwareInterruptControl;
use esp_hal::peripherals::WIFI;
use esp_hal::timer::timg::TimerGroup;
use esp_radio::wifi::WifiController;

pub struct HardwareControls {
    pub wifi: WifiState,
    pub led: Output<'static>,
}

pub enum WifiState {
    NotConnected(WIFI<'static>),
    Connected(WifiController<'static>),
}

pub fn initialise_hardware() -> HardwareControls {
    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);

    esp_alloc::heap_allocator!(size: 72 * 1024);

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let sw_int = SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);
    esp_rtos::start(timg0.timer0, sw_int.software_interrupt0);

    let led = Output::new(peripherals.GPIO2, Level::Low, OutputConfig::default());

    HardwareControls {
        wifi: WifiState::NotConnected(peripherals.WIFI),
        led,
    }
}
