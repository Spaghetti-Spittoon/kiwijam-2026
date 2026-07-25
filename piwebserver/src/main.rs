//! Raspberry Pi sensor-input web server (actix-web).
//!
//! Two players, both read DIRECTLY by the app from the OS input devices (no
//! browser):
//!   * P1 = MOUSE      — horizontal movement accumulated (Linux: evdev
//!                       /dev/input; Windows: cursor-position deltas).
//!   * P2 = CONTROLLER — Xbox pad via gilrs, direction accumulated.
//!
//! Each player's value is served as an integer 0..255 (128 = centre), shaped by
//! opponent coupling ("Quantum Entanglement") + a gentle wandering drift.
//! The web server only SERVES the values; it does not take input from a browser.
//!
//! Endpoints:
//!   GET  /                   -> plain-text status
//!   POST /input/{p1|p2}      -> body {"x": -1.0..1.0} (optional manual override)
//!   GET  /api/controls       -> "n1,n2"   (0..255, 128 = centre) plain text
//!   GET  /api/motor          -> "0" (stop) | "1" (start) plain text
//!   POST /motor/{start|stop} -> set run/stop, returns "0"|"1"

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use actix_web::{App, HttpResponse, HttpServer, Responder, get, post, web};
use gilrs::{Axis, Button, Event, EventType, Gamepad, Gilrs};
use serde::Deserialize;

/// How strongly a control tugs the opposing player's value the opposite way.
const ENTANGLE: f32 = 0.15;
/// Ignore small resting-stick drift near centre.
const DEADZONE: f32 = 0.12;

/// Served value geometry: 0..255, 128 = neutral, full deflection = +/-127.5.
const CENTER: f32 = 127.5;
const SPAN: f32 = 127.5;

/// Wandering-drift bounds (value units). Set DRIFT_MAX = 0.0 for pure coupling.
const DRIFT_MAX: f32 = 21.0;
const DRIFT_STEP: f32 = 2.0;
const DRIFT_DECAY: f32 = 0.97;
const DRIFT_TICK_MS: u64 = 120;

/// How often to print the live values to the terminal.
const LOG_TICK_MS: u64 = 500;

/// Controller accumulator: how far P2's raw value moves per 20 ms while a
/// direction is held (0.04 => centre-to-extreme in ~0.5 s).
const P2_RATE: f32 = 0.04;

/// Mouse accumulator: how far P1's raw value moves per unit of horizontal
/// mouse movement (left = toward 0, right = toward 255).
const MOUSE_RATE: f32 = 0.004;

/// Mouse LEFT-click: how far P1 ramps down per 20 ms while the button is held
/// (0.04 => centre-to-0 in ~0.5 s).
const CLICK_RATE: f32 = 0.04;

/// Whether the motorball should run. Mirrors esp8266motorball's MotorAction.
#[derive(Clone, Copy, PartialEq, Eq)]
enum MotorAction {
    Stop,
    Start,
}

impl MotorAction {
    /// Wire value: 0 = stop, 1 = start.
    fn as_num(self) -> u8 {
        match self {
            MotorAction::Start => 1,
            MotorAction::Stop => 0,
        }
    }
}

/// Latest RAW X-axis (-1.0..=1.0) per player, before drift/entanglement.
#[derive(Clone, Copy)]
struct Controls {
    p1: f32,
    p2: f32,
}

/// Slowly wandering neutral-point offset per player, in value units.
#[derive(Clone, Copy, Default)]
struct Drift {
    p1: f32,
    p2: f32,
}

struct AppState {
    motor: Mutex<MotorAction>,
    controls: Mutex<Controls>,
    drift: Mutex<Drift>,
    /// Mouse left button currently held (drives the P1 ramp-down).
    left_held: AtomicBool,
}

#[derive(Deserialize)]
struct AxisInput {
    x: f32,
}

fn clamp_axis(v: f32) -> f32 {
    if v.is_nan() { 0.0 } else { v.clamp(-1.0, 1.0) }
}

/// Raw stick (-1..1) -> value in 0..255 (128 = centre).
fn to_value(x: f32) -> f32 {
    CENTER + x * SPAN
}

fn clamp_value(v: f32) -> i32 {
    v.clamp(0.0, 255.0).round() as i32
}

/// Combine drift + opponent coupling; returns the two served values (0..255).
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

    /// Uniform in [-1.0, 1.0).
    fn signed_unit(&mut self) -> f32 {
        let u = (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32; // [0,1)
        u * 2.0 - 1.0
    }
}

/// One step of a bounded random walk that decays toward centre.
fn wander(v: f32, rng: &mut Rng) -> f32 {
    (v * DRIFT_DECAY + DRIFT_STEP * rng.signed_unit()).clamp(-DRIFT_MAX, DRIFT_MAX)
}

/// Background thread: slowly wander each player's neutral-point drift.
fn spawn_drift(state: web::Data<AppState>) {
    if DRIFT_MAX <= 0.0 {
        return; // pure coupling, no drift
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

/// Optional manual override: store a player's RAW X-axis.
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

/// Controller data the device polls: PLAIN TEXT "n1,n2" (0..255, 128 = centre).
#[get("/api/controls")]
async fn api_controls(data: web::Data<AppState>) -> impl Responder {
    let raw = *data.controls.lock().unwrap();
    let drift = *data.drift.lock().unwrap();
    let (n1, n2) = entangled(raw, drift);
    plain(format!("{n1},{n2}"))
}

/// Whether the motorball should run: PLAIN TEXT "0" (stop) or "1" (start).
#[get("/api/motor")]
async fn api_motor(data: web::Data<AppState>) -> impl Responder {
    let action = *data.motor.lock().unwrap();
    plain(action.as_num().to_string())
}

/// Set run/stop; returns the new value ("0"|"1").
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

/// -1 = steering left, +1 = right, 0 = neutral. Reads the D-pad buttons, the
/// D-pad axis, and the left stick, so whatever "left/right" the pad reports
/// counts. Any deflection past the deadzone is a full direction.
fn pad_direction(pad: &Gamepad) -> f32 {
    if pad.is_pressed(Button::DPadLeft) {
        return -1.0;
    }
    if pad.is_pressed(Button::DPadRight) {
        return 1.0;
    }
    let x = pad.value(Axis::LeftStickX) + pad.value(Axis::DPadX);
    if x < -DEADZONE {
        -1.0
    } else if x > DEADZONE {
        1.0
    } else {
        0.0
    }
}

/// Background thread: read the Xbox controller and drive Player 2 as an
/// accumulator (P1 is the mouse). Logs connects/buttons/axes for diagnostics.
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
                    break; // don't spin forever on a flood of stick-jitter events
                }
            }

            let pads: Vec<_> = gilrs.gamepads().collect();
            if let Some((_, pad0)) = pads.first() {
                let dir = pad_direction(pad0);
                if dir != 0.0 {
                    let mut c = state.controls.lock().unwrap();
                    c.p2 = (c.p2 + dir * P2_RATE).clamp(-1.0, 1.0);
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    });
}

/// Apply a horizontal mouse delta to P1 (accumulator; left = toward 0).
fn apply_mouse_dx(state: &web::Data<AppState>, dx: i32) {
    if dx == 0 {
        return;
    }
    let mut c = state.controls.lock().unwrap();
    c.p1 = (c.p1 + dx as f32 * MOUSE_RATE).clamp(-1.0, 1.0);
}

/// Linux/Pi: read mouse horizontal movement straight from evdev (/dev/input),
/// no desktop required. Picks the first device that reports REL_X.
#[cfg(target_os = "linux")]
fn spawn_mouse_reader(state: web::Data<AppState>) {
    std::thread::spawn(move || {
        use evdev::{EventType as EvType, RelativeAxisType};
        loop {
            let found = evdev::enumerate().find(|(_, d)| {
                let has_rel_x = d
                    .supported_relative_axes()
                    .map_or(false, |a| a.contains(RelativeAxisType::REL_X));
                // Require a mouse button too, so we don't grab non-pointer devices
                // that merely advertise REL_X (e.g. the Pi's `vc4` display node).
                let has_button = d
                    .supported_keys()
                    .map_or(false, |k| k.contains(evdev::Key::BTN_LEFT));
                has_rel_x && has_button
            });
            let (path, mut dev) = match found {
                Some(x) => x,
                None => {
                    eprintln!("mouse: no pointer device found; retrying...");
                    std::thread::sleep(std::time::Duration::from_secs(2));
                    continue;
                }
            };
            eprintln!(
                "mouse: reading {} ({})",
                dev.name().unwrap_or("?"),
                path.display()
            );
            loop {
                match dev.fetch_events() {
                    Ok(events) => {
                        let mut dx = 0;
                        for ev in events {
                            match ev.event_type() {
                                EvType::RELATIVE if ev.code() == RelativeAxisType::REL_X.0 => {
                                    dx += ev.value();
                                }
                                EvType::KEY if ev.code() == evdev::Key::BTN_LEFT.0 => {
                                    state.left_held.store(ev.value() != 0, Ordering::Relaxed);
                                }
                                _ => {}
                            }
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

/// Windows: poll the cursor position and accumulate the horizontal delta.
#[cfg(windows)]
fn spawn_mouse_reader(state: web::Data<AppState>) {
    std::thread::spawn(move || {
        use windows::Win32::Foundation::POINT;
        use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
        use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
        const VK_LBUTTON: i32 = 0x01;
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
            let held = (unsafe { GetAsyncKeyState(VK_LBUTTON) } as u16 & 0x8000) != 0;
            state.left_held.store(held, Ordering::Relaxed);
            std::thread::sleep(std::time::Duration::from_millis(8));
        }
    });
}

#[cfg(not(any(target_os = "linux", windows)))]
fn spawn_mouse_reader(_state: web::Data<AppState>) {
    eprintln!("mouse: direct reading not supported on this platform");
}

/// Background thread: while the mouse LEFT button is held, ramp P1 down toward 0.
fn spawn_mouse_button_ramp(state: web::Data<AppState>) {
    std::thread::spawn(move || {
        loop {
            if state.left_held.load(Ordering::Relaxed) {
                let mut c = state.controls.lock().unwrap();
                c.p1 = (c.p1 - CLICK_RATE).clamp(-1.0, 1.0);
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    });
}

/// Background thread: print the live values to the terminal in the same shape
/// as the two endpoints, e.g. `/api/controls: 137,110 /api/motor: 0`.
fn spawn_logger(state: web::Data<AppState>) {
    std::thread::spawn(move || {
        loop {
            let (n1, n2) = {
                let raw = *state.controls.lock().unwrap();
                let drift = *state.drift.lock().unwrap();
                entangled(raw, drift)
            };
            let m = state.motor.lock().unwrap().as_num();
            println!("/api/controls: {n1},{n2} /api/motor: {m}");
            std::thread::sleep(std::time::Duration::from_millis(LOG_TICK_MS));
        }
    });
}

/// Plain-text status root — no browser client; input is read device-side.
#[get("/")]
async fn index() -> impl Responder {
    plain(
        "piwebserver (motorball) — P1=mouse, P2=controller (read device-side)\n\
         GET /api/controls -> n1,n2 (0..255, 128=centre)   GET /api/motor -> 0|1\n"
            .to_string(),
    )
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let state = web::Data::new(AppState {
        motor: Mutex::new(MotorAction::Stop),
        controls: Mutex::new(Controls { p1: 0.0, p2: 0.0 }),
        drift: Mutex::new(Drift::default()),
        left_held: AtomicBool::new(false),
    });

    spawn_mouse_reader(state.clone()); // P1 = mouse movement (device-side)
    spawn_mouse_button_ramp(state.clone()); // left-click ramps P1 down
    spawn_gamepad_reader(state.clone()); // P2 = Xbox controller
    spawn_drift(state.clone());
    spawn_logger(state.clone());

    let addr = ("0.0.0.0", 8080);
    println!("piwebserver: controls (n1,n2) http://localhost:{}/api/controls", addr.1);
    println!("             motor (0|1)      http://localhost:{}/api/motor", addr.1);

    HttpServer::new(move || {
        App::new()
            .app_data(state.clone())
            .service(index)
            .service(set_input)
            .service(api_controls)
            .service(api_motor)
            .service(set_motor)
    })
    .bind(addr)?
    .run()
    .await
}
