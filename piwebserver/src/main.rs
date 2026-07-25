//! Raspberry Pi sensor-input web server (actix-web).
//!
//! Two players:
//!   * P1 = MOUSE  — a minimal browser page posts the mouse X position.
//!   * P2 = Xbox CONTROLLER — read server-side via gilrs.
//!
//! Each player's steering is served as an integer 0..255 (128 = centre), shaped
//! by opponent coupling ("Quantum Entanglement") plus a gentle wandering drift.
//! `/api/controls` returns PLAIN TEXT "n1,n2" (no JSON).
//!
//! Endpoints:
//!   GET  /                   -> minimal mouse-capture page (drives P1)
//!   POST /input/{p1|p2}      -> body {"x": -1.0..1.0}; store RAW input
//!   GET  /api/controls       -> "n1,n2"   (0..255, 128 = centre) plain text
//!   GET  /api/motor          -> "0" (stop) | "1" (start)   plain text
//!   POST /motor/{start|stop} -> set run/stop, returns "0"|"1"

use std::sync::Mutex;

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
/// direction is held (0.04 => centre-to-extreme in ~0.5 s). Holding left ramps
/// to 0 even if a cheap pad only deflects the stick partially.
const P2_RATE: f32 = 0.04;

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

/// Store a player's RAW X-axis; drift + entanglement are applied when read.
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
/// counts. Any deflection past the deadzone is a full direction, so a cheap
/// partial-range pad still ramps to the extreme.
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

/// Background thread: read the Xbox controller and drive Player 2 (P1 is the
/// mouse). Logs connects/disconnects/buttons/axes for diagnostics.
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

            // Controller drives P2 as an accumulator: holding a direction ramps
            // the value to the extreme (P1 belongs to the mouse).
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

/// Minimal mouse-capture page: mouse X position drives Player 1.
#[get("/")]
async fn index() -> impl Responder {
    HttpResponse::Ok()
        .content_type("text/html; charset=utf-8")
        .body(PAGE)
}

const PAGE: &str = r##"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>P1 Mouse Control</title>
  <style>
    body { font-family: system-ui, sans-serif; margin: 0; height: 100vh; background: #11151c;
           color: #e6e6e6; display: grid; place-items: center; cursor: crosshair; }
    .box { text-align: center; }
    .big { font-size: 3rem; font-weight: 800; letter-spacing: .05em; }
    .p1 { color: #7aa2f7; } .p2 { color: #f7768e; }
    .hint { color: #6b7280; font-size: .85rem; margin-top: 1rem; }
  </style>
</head>
<body>
  <div class="box">
    <div>P1 (mouse) <span id="p1" class="big p1">128</span>
      &nbsp;&nbsp; P2 (controller) <span id="p2" class="big p2">128</span></div>
    <div class="hint">Move the mouse left/right to steer Player 1 (left = 0, centre = 128, right = 255).
      Values are 0&ndash;255 at <code>/api/controls</code>.</div>
  </div>
<script>
let last = 0;
addEventListener('mousemove', e => {
  const now = performance.now();
  if (now - last < 50) return;          // ~20 posts/sec
  last = now;
  const x = (e.clientX / innerWidth) * 2 - 1;   // left edge -1, right edge +1
  fetch('/input/p1', {
    method: 'POST', headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ x })
  }).catch(() => {});
});
async function refresh() {
  try {
    const t = await (await fetch('/api/controls')).text();   // "n1,n2"
    const [n1, n2] = t.split(',');
    document.getElementById('p1').textContent = n1;
    document.getElementById('p2').textContent = n2;
  } catch (e) {}
}
setInterval(refresh, 150);
</script>
</body>
</html>"##;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let state = web::Data::new(AppState {
        motor: Mutex::new(MotorAction::Stop),
        controls: Mutex::new(Controls { p1: 0.0, p2: 0.0 }),
        drift: Mutex::new(Drift::default()),
    });

    spawn_gamepad_reader(state.clone()); // P2 = Xbox controller
    spawn_drift(state.clone());
    spawn_logger(state.clone()); // live values to the terminal

    let addr = ("0.0.0.0", 8080);
    println!("piwebserver: P1 mouse page  http://localhost:{}/", addr.1);
    println!("             controls (n1,n2) http://localhost:{}/api/controls", addr.1);
    println!("             motor API        http://localhost:{}/api/motor", addr.1);

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
