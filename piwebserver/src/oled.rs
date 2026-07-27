#![cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]

const SCREEN_WIDTH: f32 = 128.0;
const PLAY_TOP: f32 = 12.0;
const PLAY_BOTTOM: f32 = 63.0;
const PADDLE_HEIGHT: f32 = 16.0;
const PADDLE_WIDTH: f32 = 3.0;
const LEFT_PADDLE_X: f32 = 2.0;
const RIGHT_PADDLE_X: f32 = 123.0;
const BALL_SIZE: f32 = 3.0;
const BALL_SPEED_X: f32 = 38.0;
const BALL_SPEED_Y: f32 = 24.0;

#[derive(Clone, Copy)]
struct Pong {
    game: u64,
    ball_x: f32,
    ball_y: f32,
    velocity_x: f32,
    velocity_y: f32,
    left_y: f32,
    right_y: f32,
    left_score: u8,
    right_score: u8,
}

impl Default for Pong {
    fn default() -> Self {
        Self {
            game: 0,
            ball_x: (SCREEN_WIDTH - BALL_SIZE) / 2.0,
            ball_y: (PLAY_TOP + PLAY_BOTTOM - BALL_SIZE) / 2.0,
            velocity_x: BALL_SPEED_X,
            velocity_y: BALL_SPEED_Y,
            left_y: PLAY_TOP,
            right_y: PLAY_TOP,
            left_score: 0,
            right_score: 0,
        }
    }
}

impl Pong {
    fn begin_game(&mut self, game: u64) {
        self.game = game;
        self.left_score = 0;
        self.right_score = 0;
        self.reset_ball(if game.is_multiple_of(2) { -1.0 } else { 1.0 });
    }

    fn update(&mut self, game: u64, active: bool, player1: i32, player2: i32, dt: f32) {
        if game > 0 && game != self.game {
            self.begin_game(game);
        }

        self.left_y = paddle_y(player1);
        self.right_y = paddle_y(255 - player2.clamp(0, 255));
        if !active {
            return;
        }

        self.ball_x += self.velocity_x * dt.clamp(0.0, 0.2);
        self.ball_y += self.velocity_y * dt.clamp(0.0, 0.2);

        if self.ball_y <= PLAY_TOP {
            self.ball_y = PLAY_TOP;
            self.velocity_y = self.velocity_y.abs();
        } else if self.ball_y + BALL_SIZE >= PLAY_BOTTOM {
            self.ball_y = PLAY_BOTTOM - BALL_SIZE;
            self.velocity_y = -self.velocity_y.abs();
        }

        let ball_bottom = self.ball_y + BALL_SIZE;
        if self.velocity_x < 0.0
            && self.ball_x <= LEFT_PADDLE_X + PADDLE_WIDTH
            && self.ball_x + BALL_SIZE >= LEFT_PADDLE_X
            && ball_bottom >= self.left_y
            && self.ball_y <= self.left_y + PADDLE_HEIGHT
        {
            self.ball_x = LEFT_PADDLE_X + PADDLE_WIDTH;
            self.velocity_x = self.velocity_x.abs();
            self.add_paddle_spin(self.left_y);
        } else if self.velocity_x > 0.0
            && self.ball_x + BALL_SIZE >= RIGHT_PADDLE_X
            && self.ball_x <= RIGHT_PADDLE_X + PADDLE_WIDTH
            && ball_bottom >= self.right_y
            && self.ball_y <= self.right_y + PADDLE_HEIGHT
        {
            self.ball_x = RIGHT_PADDLE_X - BALL_SIZE;
            self.velocity_x = -self.velocity_x.abs();
            self.add_paddle_spin(self.right_y);
        }

        if self.ball_x + BALL_SIZE < 0.0 {
            self.right_score = self.right_score.saturating_add(1);
            self.reset_ball(1.0);
        } else if self.ball_x > SCREEN_WIDTH {
            self.left_score = self.left_score.saturating_add(1);
            self.reset_ball(-1.0);
        }
    }

    fn add_paddle_spin(&mut self, paddle_y: f32) {
        let paddle_center = paddle_y + PADDLE_HEIGHT / 2.0;
        let ball_center = self.ball_y + BALL_SIZE / 2.0;
        self.velocity_y =
            ((ball_center - paddle_center) / (PADDLE_HEIGHT / 2.0) * BALL_SPEED_Y * 1.4)
                .clamp(-BALL_SPEED_Y * 1.4, BALL_SPEED_Y * 1.4);
    }

    fn reset_ball(&mut self, direction: f32) {
        self.ball_x = (SCREEN_WIDTH - BALL_SIZE) / 2.0;
        self.ball_y = (PLAY_TOP + PLAY_BOTTOM - BALL_SIZE) / 2.0;
        self.velocity_x = BALL_SPEED_X * direction;
        self.velocity_y = if self
            .left_score
            .wrapping_add(self.right_score)
            .is_multiple_of(2)
        {
            BALL_SPEED_Y
        } else {
            -BALL_SPEED_Y
        };
    }
}

fn paddle_y(value: i32) -> f32 {
    let travel = PLAY_BOTTOM - PLAY_TOP - PADDLE_HEIGHT;
    PLAY_TOP + value.clamp(0, 255) as f32 / 255.0 * travel
}

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
    use embedded_graphics::primitives::{Line, PrimitiveStyle, Rectangle};
    use embedded_graphics::text::{Alignment, Baseline, Text, TextStyleBuilder};
    use linux_embedded_hal::I2cdev;
    use ssd1306::{I2CDisplayInterface, Ssd1306, mode::BufferedGraphicsMode, prelude::*};

    use crate::{AppState, CountdownStatus, current_countdown, entangled};

    const DEFAULT_DEVICE: &str = "/dev/i2c-1";
    const REFRESH_MS: u64 = 100;
    const RETRY_SECONDS: u64 = 2;

    use super::{BALL_SIZE, LEFT_PADDLE_X, PADDLE_HEIGHT, PADDLE_WIDTH, Pong, RIGHT_PADDLE_X};

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

        let mut pong = Pong::default();
        let mut previous_tick = std::time::Instant::now();
        loop {
            let now = std::time::Instant::now();
            let dt = now.duration_since(previous_tick).as_secs_f32();
            previous_tick = now;
            let status = current_countdown(state.get_ref());
            let (player1, player2) = {
                let raw = *state.controls.lock().unwrap();
                let drift = *state.drift.lock().unwrap();
                entangled(raw, drift)
            };
            let active = matches!(status.state, "running" | "chaos");
            pong.update(status.game, active, player1, player2, dt);
            draw(&mut display, status, &pong)?;
            thread::sleep(Duration::from_millis(REFRESH_MS));
        }
    }

    fn draw<DI>(
        display: &mut Ssd1306<DI, DisplaySize128x64, BufferedGraphicsMode<DisplaySize128x64>>,
        status: CountdownStatus,
        pong: &Pong,
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
        match status.state {
            "running" | "chaos" => draw_pong(display, status, pong, small)?,
            "finished" => {
                centered(display, "GAME OVER", 2, large)?;
                let score = format!("{}  -  {}", pong.left_score, pong.right_score);
                centered(display, &score, 26, large)?;
                centered(display, "HOLD Y 5 SEC", 52, small)?;
            }
            _ => {
                centered(display, "OLED PONG", 3, large)?;
                centered(display, "P1         P2", 29, small)?;
                centered(display, "HOLD Y 5 SEC", 48, small)?;
            }
        }

        display
            .flush()
            .map_err(|error| format!("flush failed: {error:?}"))
    }

    fn draw_pong<DI>(
        display: &mut Ssd1306<DI, DisplaySize128x64, BufferedGraphicsMode<DisplaySize128x64>>,
        status: CountdownStatus,
        pong: &Pong,
        style: embedded_graphics::mono_font::MonoTextStyle<'_, BinaryColor>,
    ) -> Result<(), String>
    where
        DI: display_interface::WriteOnlyDataCommand,
    {
        let header = if status.state == "chaos" {
            format!(
                "{}  CHAOS {}  {}",
                pong.left_score, status.remaining_seconds, pong.right_score
            )
        } else {
            format!(
                "{}       {}       {}",
                pong.left_score, status.remaining_seconds, pong.right_score
            )
        };
        centered(display, &header, 0, style)?;

        let on = PrimitiveStyle::with_fill(BinaryColor::On);
        Rectangle::new(
            Point::new(LEFT_PADDLE_X as i32, pong.left_y.round() as i32),
            Size::new(PADDLE_WIDTH as u32, PADDLE_HEIGHT as u32),
        )
        .into_styled(on)
        .draw(display)
        .map_err(|error| format!("left paddle draw failed: {error:?}"))?;
        Rectangle::new(
            Point::new(RIGHT_PADDLE_X as i32, pong.right_y.round() as i32),
            Size::new(PADDLE_WIDTH as u32, PADDLE_HEIGHT as u32),
        )
        .into_styled(on)
        .draw(display)
        .map_err(|error| format!("right paddle draw failed: {error:?}"))?;
        Rectangle::new(
            Point::new(pong.ball_x.round() as i32, pong.ball_y.round() as i32),
            Size::new(BALL_SIZE as u32, BALL_SIZE as u32),
        )
        .into_styled(on)
        .draw(display)
        .map_err(|error| format!("ball draw failed: {error:?}"))?;

        let dotted = PrimitiveStyle::with_stroke(BinaryColor::On, 1);
        for y in (13..64).step_by(6) {
            Line::new(Point::new(64, y), Point::new(64, y + 2))
                .into_styled(dotted)
                .draw(display)
                .map_err(|error| format!("centre line draw failed: {error:?}"))?;
        }
        Ok(())
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

#[cfg(test)]
mod tests {
    use super::{PADDLE_HEIGHT, PLAY_BOTTOM, PLAY_TOP, Pong, SCREEN_WIDTH, paddle_y};

    #[test]
    fn player_values_map_to_the_full_paddle_travel() {
        assert_eq!(paddle_y(0), PLAY_TOP);
        assert_eq!(paddle_y(255), PLAY_BOTTOM - PADDLE_HEIGHT);
        assert_eq!(paddle_y(-10), PLAY_TOP);
        assert_eq!(paddle_y(300), PLAY_BOTTOM - PADDLE_HEIGHT);
    }

    #[test]
    fn player_two_paddle_is_mirrored() {
        let mut pong = Pong::default();
        pong.update(0, false, 0, 0, 0.0);
        assert_eq!(pong.left_y, PLAY_TOP);
        assert_eq!(pong.right_y, PLAY_BOTTOM - PADDLE_HEIGHT);

        pong.update(0, false, 255, 255, 0.0);
        assert_eq!(pong.left_y, PLAY_BOTTOM - PADDLE_HEIGHT);
        assert_eq!(pong.right_y, PLAY_TOP);
    }

    #[test]
    fn pong_scores_and_resets_the_ball() {
        let mut pong = Pong::default();
        pong.begin_game(1);
        pong.ball_x = SCREEN_WIDTH + 1.0;
        pong.update(1, true, 128, 128, 0.0);

        assert_eq!(pong.left_score, 1);
        assert_eq!(pong.right_score, 0);
        assert!(pong.ball_x < SCREEN_WIDTH);
    }

    #[test]
    fn a_new_game_resets_the_score() {
        let mut pong = Pong {
            left_score: 3,
            right_score: 2,
            ..Pong::default()
        };
        pong.update(4, false, 128, 128, 0.0);

        assert_eq!(pong.game, 4);
        assert_eq!(pong.left_score, 0);
        assert_eq!(pong.right_score, 0);
    }
}
