use esp_hal::clock::CpuClock;
use esp_hal::gpio::{Level, Output, OutputConfig};
use esp_hal::interrupt::software::SoftwareInterruptControl;
use esp_hal::peripherals::WIFI;
use esp_hal::rng::Rng;
use esp_hal::timer::timg::TimerGroup;

pub struct HardwareControls {
    pub led: Output<'static>,
    pub wifi: Option<WIFI<'static>>,
    pub rng: Rng,
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
        led,
        wifi: Some(peripherals.WIFI),
        rng: Rng::new(),
    }
}
