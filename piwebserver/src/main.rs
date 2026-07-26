use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use actix_web::{App, HttpResponse, HttpServer, Responder, get, post, web};
use gilrs::{Axis, Button, Event, EventType, Gamepad, Gilrs};
use serde::{Deserialize, Serialize};

mod uno_serial;

const ENTANGLE: f32 = 0.15;
const DEADZONE: f32 = 0.12;

const CENTER: f32 = 127.5;
const SPAN: f32 = 127.5;

const DRIFT_MAX: f32 = 21.0;
const DRIFT_STEP: f32 = 2.0;
const DRIFT_DECAY: f32 = 0.97;
const DRIFT_TICK_MS: u64 = 120;

const LOG_TICK_MS: u64 = 500;
const P2_RATE: f32 = 0.03;
const MOUSE_RATE: f32 = 0.00025;
const CLICK_RATE: f32 = 0.03;
const DEFAULT_GAME_SECONDS: u64 = 60;
const HARD_PRESS_THRESHOLD: f32 = 0.75;
const START_HOLD_SECONDS: u64 = 5;

#[derive(Clone, Copy, PartialEq, Eq)]
enum MotorAction {
    Stop,
    Start,
}

impl MotorAction {
    fn as_num(self) -> u8 {
        match self {
            MotorAction::Start => 1,
            MotorAction::Stop => 0,
        }
    }
}

#[derive(Clone, Copy)]
struct Controls {
    p1: f32,
    p2: f32,
}

#[derive(Clone, Copy, Default)]
struct Drift {
    p1: f32,
    p2: f32,
}

#[derive(Clone, Copy)]
struct Countdown {
    duration_seconds: u64,
    deadline: Option<Instant>,
    game: u64,
}

struct AppState {
    motor: Mutex<MotorAction>,
    controls: Mutex<Controls>,
    drift: Mutex<Drift>,
    countdown: Mutex<Countdown>,
    left_held: AtomicBool,
    right_held: AtomicBool,
}

#[derive(Deserialize)]
struct AxisInput {
    x: f32,
}

#[derive(Clone, Copy, Serialize)]
struct CountdownStatus {
    state: &'static str,
    remaining_seconds: u64,
    duration_seconds: u64,
    game: u64,
}

#[derive(Default)]
struct HoldToStart {
    pressed_since: Option<Instant>,
    triggered: bool,
}

impl HoldToStart {
    fn update(&mut self, pressed: bool, now: Instant) -> bool {
        if !pressed {
            self.pressed_since = None;
            self.triggered = false;
            return false;
        }

        let pressed_since = self.pressed_since.get_or_insert(now);
        if !self.triggered
            && now.saturating_duration_since(*pressed_since)
                >= Duration::from_secs(START_HOLD_SECONDS)
        {
            self.triggered = true;
            return true;
        }
        false
    }
}

fn clamp_axis(v: f32) -> f32 {
    if v.is_nan() { 0.0 } else { v.clamp(-1.0, 1.0) }
}

fn to_value(x: f32) -> f32 {
    CENTER + x * SPAN
}

fn clamp_value(v: f32) -> i32 {
    v.clamp(0.0, 255.0).round() as i32
}

fn seconds_remaining(deadline: Instant, now: Instant) -> u64 {
    let remaining = deadline.saturating_duration_since(now);
    (remaining.as_millis() as u64).div_ceil(1000)
}

fn countdown_status(countdown: Countdown, now: Instant) -> CountdownStatus {
    match countdown.deadline {
        Some(deadline) if deadline > now => CountdownStatus {
            state: "running",
            remaining_seconds: seconds_remaining(deadline, now),
            duration_seconds: countdown.duration_seconds,
            game: countdown.game,
        },
        _ if countdown.game > 0 => CountdownStatus {
            state: "finished",
            remaining_seconds: 0,
            duration_seconds: countdown.duration_seconds,
            game: countdown.game,
        },
        _ => CountdownStatus {
            state: "idle",
            remaining_seconds: 0,
            duration_seconds: countdown.duration_seconds,
            game: 0,
        },
    }
}

fn current_countdown(state: &AppState) -> CountdownStatus {
    countdown_status(*state.countdown.lock().unwrap(), Instant::now())
}

fn start_game(state: &AppState, source: &str) -> CountdownStatus {
    let status = {
        let mut countdown = state.countdown.lock().unwrap();
        countdown.game = countdown.game.saturating_add(1);
        countdown.deadline = Some(Instant::now() + Duration::from_secs(countdown.duration_seconds));
        countdown_status(*countdown, Instant::now())
    };
    *state.motor.lock().unwrap() = MotorAction::Start;
    println!(
        "countdown: game {} started by {}; {} seconds; motor started",
        status.game, source, status.duration_seconds
    );
    status
}

fn entangled(raw: Controls, drift: Drift) -> (i32, i32) {
    let b1 = to_value(raw.p1) + drift.p1;
    let b2 = to_value(raw.p2) + drift.p2;
    let o1 = CENTER + (b1 - CENTER) - ENTANGLE * (b2 - CENTER);
    let o2 = CENTER + (b2 - CENTER) - ENTANGLE * (b1 - CENTER);
    (clamp_value(o1), clamp_value(o2))
}

/// Tiny xorshift64 PRNG — enough for gentle drift without a `rand` dependency.
struct Rng(u64);

impl Rng {
    fn from_time() -> Self {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9E37_79B9_7F4A_7C15);
        Rng(seed | 1)
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn signed_unit(&mut self) -> f32 {
        let u = (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32;
        u * 2.0 - 1.0
    }
}

fn wander(v: f32, rng: &mut Rng) -> f32 {
    (v * DRIFT_DECAY + DRIFT_STEP * rng.signed_unit()).clamp(-DRIFT_MAX, DRIFT_MAX)
}

fn spawn_drift(state: web::Data<AppState>) {
    if DRIFT_MAX <= 0.0 {
        return;
    }
    std::thread::spawn(move || {
        let mut rng = Rng::from_time();
        loop {
            {
                let mut d = state.drift.lock().unwrap();
                d.p1 = wander(d.p1, &mut rng);
                d.p2 = wander(d.p2, &mut rng);
            }
            std::thread::sleep(std::time::Duration::from_millis(DRIFT_TICK_MS));
        }
    });
}

fn plain(body: String) -> HttpResponse {
    HttpResponse::Ok()
        .content_type("text/plain; charset=utf-8")
        .body(body)
}

#[post("/input/{player}")]
async fn set_input(
    path: web::Path<String>,
    body: web::Json<AxisInput>,
    data: web::Data<AppState>,
) -> impl Responder {
    let x = clamp_axis(body.x);
    let raw = {
        let mut c = data.controls.lock().unwrap();
        match path.into_inner().as_str() {
            "p1" => c.p1 = x,
            "p2" => c.p2 = x,
            _ => return HttpResponse::BadRequest().body("unknown player (use p1|p2)"),
        }
        *c
    };
    let drift = *data.drift.lock().unwrap();
    let (n1, n2) = entangled(raw, drift);
    plain(format!("{n1},{n2}"))
}

#[get("/api/controls")]
async fn api_controls(data: web::Data<AppState>) -> impl Responder {
    let raw = *data.controls.lock().unwrap();
    let drift = *data.drift.lock().unwrap();
    let (n1, n2) = entangled(raw, drift);
    plain(format!("{n1},{n2}"))
}

#[get("/api/motor")]
async fn api_motor(data: web::Data<AppState>) -> impl Responder {
    let action = *data.motor.lock().unwrap();
    plain(action.as_num().to_string())
}

#[get("/api/countdown")]
async fn api_countdown(data: web::Data<AppState>) -> impl Responder {
    web::Json(current_countdown(data.get_ref()))
}

#[post("/game/start")]
async fn api_start_game(data: web::Data<AppState>) -> impl Responder {
    web::Json(start_game(data.get_ref(), "HTTP"))
}

#[post("/motor/{action}")]
async fn set_motor(path: web::Path<String>, data: web::Data<AppState>) -> impl Responder {
    let new = match path.into_inner().as_str() {
        "start" => Some(MotorAction::Start),
        "stop" => Some(MotorAction::Stop),
        _ => None,
    };
    match new {
        Some(action) => {
            *data.motor.lock().unwrap() = action;
            plain(action.as_num().to_string())
        }
        None => HttpResponse::BadRequest().body("unknown motor action (use start|stop)"),
    }
}

fn input_direction(left: bool, right: bool, axes: [f32; 3]) -> f32 {
    match (left, right) {
        (true, false) => return -1.0,
        (false, true) => return 1.0,
        (true, true) => return 0.0,
        (false, false) => {}
    }

    // Use whichever supported horizontal axis is being moved the furthest.
    let mut x = axes[0];
    for candidate in &axes[1..] {
        if candidate.abs() > x.abs() {
            x = *candidate;
        }
    }
    if x < -DEADZONE {
        -1.0
    } else if x > DEADZONE {
        1.0
    } else {
        0.0
    }
}

fn mouse_button_direction(left: bool, right: bool) -> f32 {
    match (left, right) {
        (true, false) => -1.0,
        (false, true) => 1.0,
        _ => 0.0,
    }
}

fn hard_pressed(pad: &Gamepad, button: Button) -> bool {
    pad.button_data(button)
        .is_some_and(|data| data.value() >= HARD_PRESS_THRESHOLD)
}

fn pad_direction(pad: &Gamepad) -> f32 {
    // This Pi's Xbox driver reports physical X as North and physical B as East.
    let left = pad.is_pressed(Button::DPadLeft) || pad.is_pressed(Button::North);
    let right = pad.is_pressed(Button::DPadRight) || pad.is_pressed(Button::East);
    input_direction(
        left,
        right,
        [
            pad.value(Axis::LeftStickX),
            pad.value(Axis::RightStickX),
            pad.value(Axis::DPadX),
        ],
    )
}

fn spawn_gamepad_reader(state: web::Data<AppState>) {
    std::thread::spawn(move || {
        let mut gilrs = match Gilrs::new() {
            Ok(g) => g,
            Err(e) => {
                eprintln!("gamepad: input unavailable ({e}); controller reading disabled");
                return;
            }
        };
        eprintln!("gamepad: reader started (waiting for a controller)");
        let mut west_hold = HoldToStart::default();
        loop {
            let mut drained = 0;
            while let Some(Event { id, event, .. }) = gilrs.next_event() {
                match event {
                    EventType::Connected => {
                        eprintln!("gamepad {}: CONNECTED: {}", id, gilrs.gamepad(id).name());
                    }
                    EventType::Disconnected => eprintln!("gamepad {}: disconnected", id),
                    EventType::ButtonPressed(b, _) => eprintln!("gamepad {}: BTN {:?}", id, b),
                    EventType::ButtonChanged(b, v, _) if v > 0.25 => {
                        eprintln!("gamepad {}: BTN~ {:?} = {:.2}", id, b, v)
                    }
                    EventType::AxisChanged(a, v, _) if v.abs() > 0.25 => {
                        eprintln!("gamepad {}: AXIS {:?} = {:.2}", id, a, v)
                    }
                    _ => {}
                }
                drained += 1;
                if drained > 512 {
                    break; // Avoid starvation during a flood of stick-jitter events.
                }
            }

            let pads: Vec<_> = gilrs.gamepads().collect();
            if let Some((_, pad0)) = pads.first() {
                let west_is_hard = hard_pressed(pad0, Button::West);
                if west_hold.update(west_is_hard, Instant::now()) {
                    start_game(state.get_ref(), "controller West/Y five-second hold");
                }

                let dir = pad_direction(pad0);
                if dir != 0.0 {
                    let mut c = state.controls.lock().unwrap();
                    c.p2 = (c.p2 + dir * P2_RATE).clamp(-1.0, 1.0);
                }
            } else {
                west_hold.update(false, Instant::now());
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    });
}

fn apply_mouse_dx(state: &web::Data<AppState>, dx: i32) {
    if dx == 0 {
        return;
    }
    let mut c = state.controls.lock().unwrap();
    c.p1 = (c.p1 + dx as f32 * MOUSE_RATE).clamp(-1.0, 1.0);
}

#[cfg(target_os = "linux")]
fn diagnose_input_devices() {
    use evdev::{BusType, Device, Key, RelativeAxisType};
    let dir = match std::fs::read_dir("/dev/input") {
        Ok(d) => d,
        Err(e) => {
            eprintln!("mouse: cannot read /dev/input ({e})");
            return;
        }
    };
    let mut nodes: Vec<_> = dir
        .filter_map(|r| r.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("event"))
        .map(|e| e.path())
        .collect();
    nodes.sort();
    if nodes.is_empty() {
        eprintln!("mouse: /dev/input has no event* nodes");
        return;
    }
    let mut denied = 0;
    for path in nodes {
        match Device::open(&path) {
            Ok(d) => {
                let has_rel_x = d
                    .supported_relative_axes()
                    .map_or(false, |a| a.contains(RelativeAxisType::REL_X));
                let has_button = d
                    .supported_keys()
                    .map_or(false, |k| k.contains(Key::BTN_LEFT));
                let bus = d.input_id().bus_type();
                eprintln!(
                    "mouse: scan {} name={:?} bus={} usb={} rel_x={} btn_left={}",
                    path.display(),
                    d.name().unwrap_or("?"),
                    bus,
                    bus == BusType::BUS_USB,
                    has_rel_x,
                    has_button
                );
            }
            Err(e) => {
                if e.kind() == std::io::ErrorKind::PermissionDenied {
                    denied += 1;
                }
                eprintln!("mouse: scan {} error: {}", path.display(), e);
            }
        }
    }
    if denied > 0 {
        eprintln!(
            "mouse: {denied} device(s) refused with permission denied — \
             add your user to the `input` group (usermod -aG input $USER) \
             or install a udev rule granting the input group access to /dev/input/event*"
        );
    }
}

#[cfg(target_os = "linux")]
fn spawn_mouse_reader(state: web::Data<AppState>) {
    std::thread::spawn(move || {
        use evdev::{BusType, EventType as EvType, RelativeAxisType};
        let mut diagnosed = false;
        let mut waiting_logged = false;
        loop {
            let found = evdev::enumerate().find(|(_, d)| {
                if d.input_id().bus_type() != BusType::BUS_USB {
                    return false;
                }
                let has_rel_x = d
                    .supported_relative_axes()
                    .map_or(false, |a| a.contains(RelativeAxisType::REL_X));
                let has_button = d
                    .supported_keys()
                    .map_or(false, |k| k.contains(evdev::Key::BTN_LEFT));
                has_rel_x && has_button
            });
            let (path, mut dev) = match found {
                Some(x) => x,
                None => {
                    if !waiting_logged {
                        eprintln!("mouse: no USB pointer connected; waiting...");
                        waiting_logged = true;
                    }
                    if !diagnosed {
                        diagnose_input_devices();
                        diagnosed = true;
                    }
                    std::thread::sleep(std::time::Duration::from_secs(1));
                    continue;
                }
            };
            waiting_logged = false;
            eprintln!(
                "mouse: reading {} ({})",
                dev.name().unwrap_or("?"),
                path.display()
            );
            let debug = std::env::var_os("PIWS_MOUSE_DEBUG").is_some();
            let mut logged_first = false;
            loop {
                match dev.fetch_events() {
                    Ok(events) => {
                        let mut dx = 0;
                        let mut count = 0usize;
                        for ev in events {
                            count += 1;
                            if debug && !logged_first {
                                eprintln!(
                                    "mouse: first event type={:?} code={} value={}",
                                    ev.event_type(),
                                    ev.code(),
                                    ev.value()
                                );
                                logged_first = true;
                            }
                            match ev.event_type() {
                                EvType::RELATIVE if ev.code() == RelativeAxisType::REL_X.0 => {
                                    dx += ev.value();

                                    if debug {
                                        println!("mouse moved: {}", ev.value());
                                    }
                                }
                                EvType::KEY if ev.code() == evdev::Key::BTN_LEFT.0 => {
                                    state.left_held.store(ev.value() != 0, Ordering::Relaxed);

                                    if debug {
                                        println!("mouse left button: {}", ev.value() != 0);
                                    }
                                }
                                EvType::KEY if ev.code() == evdev::Key::BTN_RIGHT.0 => {
                                    state.right_held.store(ev.value() != 0, Ordering::Relaxed);

                                    if debug {
                                        println!("mouse right button: {}", ev.value() != 0);
                                    }
                                }
                                _ => {}
                            }
                        }
                        if debug && count > 0 {
                            eprintln!("mouse: fetch got {count} event(s), dx={dx}");
                        }
                        apply_mouse_dx(&state, dx);
                    }
                    Err(e) => {
                        eprintln!("mouse: read error ({e}); rescanning");
                        break;
                    }
                }
            }
        }
    });
}

#[cfg(windows)]
fn spawn_mouse_reader(state: web::Data<AppState>) {
    std::thread::spawn(move || {
        use windows::Win32::Foundation::POINT;
        use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
        use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
        const VK_LBUTTON: i32 = 0x01;
        const VK_RBUTTON: i32 = 0x02;
        eprintln!("mouse: polling cursor position");
        let mut last: Option<i32> = None;
        loop {
            let mut p = POINT::default();
            if unsafe { GetCursorPos(&mut p) }.is_ok() {
                if let Some(lx) = last {
                    apply_mouse_dx(&state, p.x - lx);
                }
                last = Some(p.x);
            }
            let left = (unsafe { GetAsyncKeyState(VK_LBUTTON) } as u16 & 0x8000) != 0;
            let right = (unsafe { GetAsyncKeyState(VK_RBUTTON) } as u16 & 0x8000) != 0;
            state.left_held.store(left, Ordering::Relaxed);
            state.right_held.store(right, Ordering::Relaxed);
            std::thread::sleep(std::time::Duration::from_millis(8));
        }
    });
}

#[cfg(not(any(target_os = "linux", windows)))]
fn spawn_mouse_reader(_state: web::Data<AppState>) {
    eprintln!("mouse: direct reading not supported on this platform");
}

fn spawn_mouse_button_ramp(state: web::Data<AppState>) {
    std::thread::spawn(move || {
        loop {
            let direction = mouse_button_direction(
                state.left_held.load(Ordering::Relaxed),
                state.right_held.load(Ordering::Relaxed),
            );
            if direction != 0.0 {
                let mut c = state.controls.lock().unwrap();
                c.p1 = (c.p1 + direction * CLICK_RATE).clamp(-1.0, 1.0);
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    });
}

fn spawn_countdown(state: web::Data<AppState>) {
    std::thread::spawn(move || {
        loop {
            let expired_game = {
                let mut countdown = state.countdown.lock().unwrap();
                match countdown.deadline {
                    Some(deadline) if deadline <= Instant::now() => {
                        countdown.deadline = None;
                        Some(countdown.game)
                    }
                    _ => None,
                }
            };

            if let Some(game) = expired_game {
                *state.motor.lock().unwrap() = MotorAction::Stop;
                println!("countdown: game {game} finished; motor stopped");
            }

            std::thread::sleep(Duration::from_millis(100));
        }
    });
}

fn spawn_logger(state: web::Data<AppState>) {
    std::thread::spawn(move || {
        loop {
            let (n1, n2) = {
                let raw = *state.controls.lock().unwrap();
                let drift = *state.drift.lock().unwrap();
                entangled(raw, drift)
            };
            let m = state.motor.lock().unwrap().as_num();
            let countdown = current_countdown(state.get_ref());
            println!(
                "/api/controls: {n1},{n2} /api/motor: {m} \
                 /api/countdown: {},{}s,game={}",
                countdown.state, countdown.remaining_seconds, countdown.game
            );
            std::thread::sleep(std::time::Duration::from_millis(LOG_TICK_MS));
        }
    });
}

#[get("/")]
async fn index() -> impl Responder {
    plain(
        "piwebserver (motorball) — P1=mouse, P2=controller (read device-side)\n\
         GET /api/controls -> n1,n2 (0..255, 128=centre)   GET /api/motor -> 0|1\n\
         GET /api/countdown -> game timer JSON   POST /game/start -> start/restart game\n"
            .to_string(),
    )
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let game_seconds = std::env::var("PIWS_GAME_SECONDS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|seconds| *seconds > 0)
        .unwrap_or(DEFAULT_GAME_SECONDS);
    let state = web::Data::new(AppState {
        motor: Mutex::new(MotorAction::Stop),
        controls: Mutex::new(Controls { p1: 0.0, p2: 0.0 }),
        drift: Mutex::new(Drift::default()),
        countdown: Mutex::new(Countdown {
            duration_seconds: game_seconds,
            deadline: None,
            game: 0,
        }),
        left_held: AtomicBool::new(false),
        right_held: AtomicBool::new(false),
    });

    spawn_mouse_reader(state.clone());
    spawn_mouse_button_ramp(state.clone());
    spawn_gamepad_reader(state.clone());
    spawn_drift(state.clone());
    spawn_countdown(state.clone());
    spawn_logger(state.clone());
    let pwm_state = state.clone();
    uno_serial::spawn(move || {
        let raw = *pwm_state.controls.lock().unwrap();
        let drift = *pwm_state.drift.lock().unwrap();
        entangled(raw, drift)
    })?;

    let addr = ("0.0.0.0", 7777);
    println!(
        "piwebserver: controls (n1,n2) http://localhost:{}/api/controls",
        addr.1
    );
    println!(
        "             motor (0|1)      http://localhost:{}/api/motor",
        addr.1
    );
    println!(
        "         countdown (JSON)      http://localhost:{}/api/countdown ({}s rounds)",
        addr.1, game_seconds
    );

    HttpServer::new(move || {
        App::new()
            .app_data(state.clone())
            .service(index)
            .service(set_input)
            .service(api_controls)
            .service(api_motor)
            .service(api_countdown)
            .service(api_start_game)
            .service(set_motor)
    })
    .bind(addr)?
    .run()
    .await
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::{
        Countdown, HoldToStart, countdown_status, input_direction, mouse_button_direction,
        seconds_remaining,
    };

    #[test]
    fn face_buttons_drive_both_directions() {
        assert_eq!(input_direction(true, false, [0.0; 3]), -1.0);
        assert_eq!(input_direction(false, true, [0.0; 3]), 1.0);
        assert_eq!(input_direction(true, true, [0.0; 3]), 0.0);
    }

    #[test]
    fn strongest_stick_or_dpad_axis_wins() {
        assert_eq!(input_direction(false, false, [0.2, -0.8, 0.0]), -1.0);
        assert_eq!(input_direction(false, false, [0.2, 0.4, 0.9]), 1.0);
        assert_eq!(input_direction(false, false, [0.01, -0.02, 0.0]), 0.0);
    }

    #[test]
    fn mouse_buttons_drive_opposite_directions() {
        assert_eq!(mouse_button_direction(true, false), -1.0);
        assert_eq!(mouse_button_direction(false, true), 1.0);
        assert_eq!(mouse_button_direction(true, true), 0.0);
        assert_eq!(mouse_button_direction(false, false), 0.0);
    }

    #[test]
    fn countdown_rounds_partial_seconds_up_for_display() {
        let now = Instant::now();
        assert_eq!(seconds_remaining(now + Duration::from_millis(1001), now), 2);
        assert_eq!(seconds_remaining(now + Duration::from_secs(1), now), 1);
        assert_eq!(seconds_remaining(now, now), 0);
    }

    #[test]
    fn countdown_reports_idle_running_and_finished() {
        let now = Instant::now();
        let mut countdown = Countdown {
            duration_seconds: 60,
            deadline: None,
            game: 0,
        };
        assert_eq!(countdown_status(countdown, now).state, "idle");

        countdown.game = 1;
        countdown.deadline = Some(now + Duration::from_secs(30));
        assert_eq!(countdown_status(countdown, now).state, "running");
        assert_eq!(countdown_status(countdown, now).remaining_seconds, 30);

        countdown.deadline = None;
        assert_eq!(countdown_status(countdown, now).state, "finished");
    }

    #[test]
    fn west_button_requires_a_five_second_hold_and_release() {
        let now = Instant::now();
        let mut hold = HoldToStart::default();

        assert!(!hold.update(true, now));
        assert!(!hold.update(true, now + Duration::from_millis(4999)));
        assert!(hold.update(true, now + Duration::from_secs(5)));
        assert!(!hold.update(true, now + Duration::from_secs(10)));

        assert!(!hold.update(false, now + Duration::from_secs(11)));
        assert!(!hold.update(true, now + Duration::from_secs(12)));
        assert!(hold.update(true, now + Duration::from_secs(17)));
    }
}
