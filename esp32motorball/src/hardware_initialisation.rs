use esp_hal::clock::CpuClock;
use esp_hal::gpio::{Level, Output, OutputConfig};
use esp_hal::i2c::master::{Config as I2cConfig, I2c};
use esp_hal::interrupt::software::SoftwareInterruptControl;
use esp_hal::peripherals::WIFI;
use esp_hal::rng::Rng;
use esp_hal::timer::timg::TimerGroup;
use esp_hal::Blocking;
use esp_println::println;
use mma8x5x::{ic, mode, Mma8x5x, SlaveAddr};

pub type TiltSensor = Mma8x5x<I2c<'static, Blocking>, ic::Mma8452, mode::Active>;

pub struct HardwareControls {
    pub led: Output<'static>,
    pub motor_left: Output<'static>,
    pub motor_right: Output<'static>,
    pub tilt_sensor: Option<TiltSensor>,
    pub wifi: Option<WIFI<'static>>,
    pub rng: Rng,
}

pub fn initialise_hardware() -> HardwareControls {
    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::_80MHz);
    let peripherals = esp_hal::init(config);

    esp_alloc::heap_allocator!(size: 72 * 1024);

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let sw_int = SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);
    esp_rtos::start(timg0.timer0, sw_int.software_interrupt0);

    let led = Output::new(peripherals.GPIO2, Level::Low, OutputConfig::default());
    let motor_left = Output::new(peripherals.GPIO26, Level::Low, OutputConfig::default());
    let motor_right = Output::new(peripherals.GPIO27, Level::Low, OutputConfig::default());

    let tilt_sensor = match I2c::new(peripherals.I2C0, I2cConfig::default()) {
        Ok(i2c) => {
            let i2c = i2c.with_sda(peripherals.GPIO21).with_scl(peripherals.GPIO22);
            let sensor = Mma8x5x::new_mma8452(i2c, SlaveAddr::default());
            match sensor.into_active() {
                Ok(active) => Some(active),
                Err(_) => {
                    println!("failed to activate MMA8452Q");
                    None
                }
            }
        }
        Err(e) => {
            println!("failed to initialise I2C: {e:?}");
            None
        }
    };

    HardwareControls {
        led,
        motor_left,
        motor_right,
        tilt_sensor,
        wifi: Some(peripherals.WIFI),
        rng: Rng::new(),
    }
}
