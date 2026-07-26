#[cfg(target_os = "linux")]
mod platform {
    use std::thread;
    use std::time::Duration;

    use actix_web::web;
    use embedded_graphics::mono_font::{
        MonoTextStyleBuilder,
        ascii::{FONT_6X10, FONT_10X20},
    };
    use embedded_graphics::pixelcolor::BinaryColor;
    use embedded_graphics::prelude::*;
    use embedded_graphics::text::{Alignment, Baseline, Text, TextStyleBuilder};
    use linux_embedded_hal::I2cdev;
    use ssd1306::{I2CDisplayInterface, Ssd1306, mode::BufferedGraphicsMode, prelude::*};

    use crate::{AppState, CountdownStatus, current_countdown};

    const DEFAULT_DEVICE: &str = "/dev/i2c-1";
    const REFRESH_MS: u64 = 200;
    const RETRY_SECONDS: u64 = 2;

    pub(crate) fn spawn(state: web::Data<AppState>) {
        thread::spawn(move || {
            loop {
                if let Err(error) = run(&state) {
                    eprintln!("oled: {error}; retrying in {RETRY_SECONDS}s");
                    thread::sleep(Duration::from_secs(RETRY_SECONDS));
                }
            }
        });
    }

    fn run(state: &web::Data<AppState>) -> Result<(), String> {
        let device = std::env::var("PIWS_OLED_I2C").unwrap_or_else(|_| DEFAULT_DEVICE.to_owned());
        let i2c = I2cdev::new(&device).map_err(|error| format!("cannot open {device}: {error}"))?;
        let interface = I2CDisplayInterface::new(i2c);
        let mut display = Ssd1306::new(interface, DisplaySize128x64, DisplayRotation::Rotate0)
            .into_buffered_graphics_mode();
        display
            .init()
            .map_err(|error| format!("SSD1315 init failed on {device}: {error:?}"))?;
        display
            .flush()
            .map_err(|error| format!("initial flush failed: {error:?}"))?;
        println!("oled: SSD1315 display connected on {device} at 0x3c");

        let mut previous = None;
        loop {
            let status = current_countdown(state.get_ref());
            let marker = (status.state, status.remaining_seconds, status.game);
            if previous != Some(marker) {
                draw(&mut display, status)?;
                previous = Some(marker);
            }
            thread::sleep(Duration::from_millis(REFRESH_MS));
        }
    }

    fn draw<DI>(
        display: &mut Ssd1306<DI, DisplaySize128x64, BufferedGraphicsMode<DisplaySize128x64>>,
        status: CountdownStatus,
    ) -> Result<(), String>
    where
        DI: display_interface::WriteOnlyDataCommand,
    {
        let small = MonoTextStyleBuilder::new()
            .font(&FONT_6X10)
            .text_color(BinaryColor::On)
            .build();
        let large = MonoTextStyleBuilder::new()
            .font(&FONT_10X20)
            .text_color(BinaryColor::On)
            .build();

        display
            .clear(BinaryColor::Off)
            .map_err(|error| format!("clear failed: {error:?}"))?;
        centered(display, "KIWIJAM", 0, small)?;

        match status.state {
            "running" => {
                let game = format!("GAME {}", status.game);
                let seconds = format!("{}", status.remaining_seconds);
                centered(display, &game, 13, small)?;
                centered(display, &seconds, 25, large)?;
                centered(display, "SECONDS", 51, small)?;
            }
            "chaos" => {
                let seconds = format!("{}s", status.remaining_seconds);
                centered(display, "CHAOS!", 13, small)?;
                centered(display, &seconds, 25, large)?;
                centered(display, "BALL GOES WILD", 51, small)?;
            }
            "finished" => {
                centered(display, "GAME OVER", 19, large)?;
                centered(display, "HOLD Y 5 SEC", 49, small)?;
            }
            _ => {
                centered(display, "READY", 19, large)?;
                centered(display, "HOLD Y 5 SEC", 49, small)?;
            }
        }

        display
            .flush()
            .map_err(|error| format!("flush failed: {error:?}"))
    }

    fn centered<D, C>(
        display: &mut D,
        text: &str,
        y: i32,
        style: embedded_graphics::mono_font::MonoTextStyle<'_, C>,
    ) -> Result<(), String>
    where
        D: DrawTarget<Color = C>,
        C: PixelColor,
        D::Error: core::fmt::Debug,
    {
        let text_style = TextStyleBuilder::new()
            .alignment(Alignment::Center)
            .baseline(Baseline::Top)
            .build();
        Text::with_text_style(text, Point::new(64, y), style, text_style)
            .draw(display)
            .map(|_| ())
            .map_err(|error| format!("text draw failed: {error:?}"))
    }
}

#[cfg(target_os = "linux")]
pub(crate) use platform::spawn;

#[cfg(not(target_os = "linux"))]
pub(crate) fn spawn(_state: actix_web::web::Data<crate::AppState>) {}
