//! Raspberry Pi sensor-input web server (actix-web) — issue #20.
//!
//! Reads controller / mouse X-axis input, stores the latest RAW value per player
//! in memory, and serves an entangled "steering" angle in the 0..180 range
//! (90 = centre) over HTTP. Two effects shape the served value:
//!   1. Opponent coupling (the core "Quantum Entanglement"): each player's angle
//!      is nudged in the OPPOSITE direction of the opponent's deviation.
//!   2. A gentle wandering drift per player: the neutral point slowly strays off
//!      90, so a centred stick isn't always exactly 90 and "turning left" can
//!      read slightly right — quantum uncertainty for flavour.
//!
//! We deliberately do NOT send TTL signals to the Arduino Uno. Instead the web
//! server *is* the controller-data source: consumers poll the API.
//!
//! Endpoints:
//!   GET  /                   -> client page: captures mouse + Gamepad X, live view
//!   POST /input/{p1|p2}      -> body {"x": -1.0..1.0}; store RAW input
//!   GET  /api/controls       -> {"p1": deg, "p2": deg}  0..180, 90=centre
//!   GET  /api/motor          -> {"motor": "start"|"stop"}
//!   POST /motor/{start|stop} -> set run/stop
//!
//! `MotorAction` mirrors `esp8266motorball::wifi_inputs::MotorAction` (the ESP's
//! `poll_server()` contract), kept in sync by hand — branches aren't merged.

use std::sync::Mutex;

use actix_web::{App, HttpResponse, HttpServer, Responder, get, post, web};
use gilrs::{Axis, Gilrs};
use serde::{Deserialize, Serialize};

/// How strongly a control tugs the opposing player's angle the opposite way.
const ENTANGLE: f32 = 0.15;
/// Ignore small resting-stick drift near centre.
const DEADZONE: f32 = 0.12;

/// Served-angle geometry: 0..180 degrees, 90 = neutral, full stick = +/-90.
const CENTER: f32 = 90.0;
const SPAN: f32 = 90.0;

/// Wandering-drift bounds (degrees). Set DRIFT_MAX = 0.0 for pure coupling.
const DRIFT_MAX: f32 = 15.0;
/// Random-walk step per tick (degrees) and pull-back toward centre.
const DRIFT_STEP: f32 = 1.5;
const DRIFT_DECAY: f32 = 0.97;
const DRIFT_TICK_MS: u64 = 120;

/// Whether the motorball should run. Mirrors esp8266motorball's MotorAction.
#[derive(Clone, Copy, PartialEq, Eq)]
enum MotorAction {
    Stop,
    Start,
}

impl MotorAction {
    fn as_str(self) -> &'static str {
        match self {
            MotorAction::Start => "start",
            MotorAction::Stop => "stop",
        }
    }
}

/// Latest RAW X-axis (-1.0..=1.0) per player, before drift/entanglement.
#[derive(Clone, Copy)]
struct Controls {
    p1: f32,
    p2: f32,
}

/// Slowly wandering neutral-point offset per player, in degrees.
#[derive(Clone, Copy, Default)]
struct Drift {
    p1: f32,
    p2: f32,
}

/// Entangled steering angle served to clients/devices (0..180, 90 = centre).
#[derive(Serialize)]
struct ControlsOut {
    p1: i32,
    p2: i32,
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

#[derive(Serialize)]
struct MotorStatus {
    motor: &'static str,
}

fn clamp_axis(v: f32) -> f32 {
    if v.is_nan() { 0.0 } else { v.clamp(-1.0, 1.0) }
}

/// Raw stick (-1..1) -> steering angle in degrees (0..180).
fn to_deg(x: f32) -> f32 {
    CENTER + x * SPAN
}

fn clamp_deg(v: f32) -> i32 {
    v.clamp(0.0, 180.0).round() as i32
}

/// Combine the two effects into the served angles:
///   * add each player's wandering drift to their own angle, then
///   * apply opponent coupling (the core entanglement): nudge opposite to the
///     opponent's deviation from centre.
fn entangled(raw: Controls, drift: Drift) -> ControlsOut {
    let b1 = to_deg(raw.p1) + drift.p1;
    let b2 = to_deg(raw.p2) + drift.p2;
    let o1 = CENTER + (b1 - CENTER) - ENTANGLE * (b2 - CENTER);
    let o2 = CENTER + (b2 - CENTER) - ENTANGLE * (b1 - CENTER);
    ControlsOut {
        p1: clamp_deg(o1),
        p2: clamp_deg(o2),
    }
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
    HttpResponse::Ok().json(entangled(raw, drift))
}

/// Controller data the device polls (instead of receiving TTL).
#[get("/api/controls")]
async fn api_controls(data: web::Data<AppState>) -> impl Responder {
    let raw = *data.controls.lock().unwrap();
    let drift = *data.drift.lock().unwrap();
    web::Json(entangled(raw, drift))
}

/// Whether the motorball should run or stop.
#[get("/api/motor")]
async fn api_motor(data: web::Data<AppState>) -> impl Responder {
    let action = *data.motor.lock().unwrap();
    web::Json(MotorStatus {
        motor: action.as_str(),
    })
}

/// Set run/stop.
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
            HttpResponse::Ok().json(MotorStatus {
                motor: action.as_str(),
            })
        }
        None => HttpResponse::BadRequest().body("unknown motor action (use start|stop)"),
    }
}

fn deadzone(x: f32) -> f32 {
    if x.abs() < DEADZONE { 0.0 } else { x }
}

/// Background thread: read the Xbox controller(s) on the (headless) Pi and feed
/// their X-axis into shared state. Left stick of the first pad -> P1, right
/// stick -> P2; a second connected pad's left stick overrides P2. No screen or
/// browser required. If no input subsystem is available it logs and exits so
/// the HTTP `/input` endpoints still work.
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
            // Pump events so `value()` reflects the latest stick positions.
            while gilrs.next_event().is_some() {}

            let pads: Vec<_> = gilrs.gamepads().collect();
            if let Some((_, pad0)) = pads.first() {
                let p1 = deadzone(pad0.value(Axis::LeftStickX));
                let p2 = match pads.get(1) {
                    Some((_, pad1)) => deadzone(pad1.value(Axis::LeftStickX)),
                    None => deadzone(pad0.value(Axis::RightStickX)),
                };
                let mut c = state.controls.lock().unwrap();
                c.p1 = clamp_axis(p1);
                c.p2 = clamp_axis(p2);
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    });
}

/// Client page: captures input and shows the live state.
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
  <title>Motorball Control</title>
  <style>
    :root { color-scheme: dark; }
    body { font-family: system-ui, sans-serif; margin: 0; min-height: 100vh; background: #11151c;
           color: #e6e6e6; display: grid; place-items: center; }
    .wrap { width: min(92vw, 560px); }
    h1 { font-size: 1rem; letter-spacing: .12em; text-transform: uppercase; color: #8a93a6; text-align: center; }
    .pill { display: inline-block; padding: .3rem 1rem; border-radius: 999px; font-weight: 700; letter-spacing: .05em; }
    .pill.start { background: #0aa06e; } .pill.stop { background: #c0392b; }
    .status { text-align: center; margin-bottom: 1.4rem; }
    .player { margin: 1rem 0; }
    .player .label { display: flex; justify-content: space-between; font-size: .85rem; color: #9aa4b8; margin-bottom: .35rem; }
    .track { position: relative; height: 14px; background: #1b2029; border-radius: 8px; }
    .track .center { position: absolute; left: 50%; top: -4px; bottom: -4px; width: 2px; background: #333c4d; }
    .fill { position: absolute; top: 0; bottom: 0; width: 14px; margin-left: -7px; border-radius: 7px;
            background: #7aa2f7; box-shadow: 0 0 10px #7aa2f7; transition: left .08s linear; left: 50%; }
    .p2 .fill { background: #f7768e; box-shadow: 0 0 10px #f7768e; }
    button { font: inherit; font-weight: 600; padding: .5rem 1.2rem; margin: .2rem; border: 0; border-radius: 10px;
             cursor: pointer; color: #fff; } .start { background: #0aa06e; } .stop { background: #c0392b; }
    .hint { text-align: center; color: #6b7280; font-size: .8rem; margin-top: 1rem; }
    a { color: #7aa2f7; }
  </style>
</head>
<body>
  <div class="wrap">
    <h1>Motorball Control</h1>
    <div class="status">
      motor should <span id="motor" class="pill stop">STOP</span><br><br>
      <button class="start" onclick="motor('start')">Start</button>
      <button class="stop" onclick="motor('stop')">Stop</button>
    </div>
    <div class="player p1">
      <div class="label"><span>Player 1 &nbsp;(mouse X)</span><span id="p1-val">90&deg;</span></div>
      <div class="track"><div class="center"></div><div id="p1-fill" class="fill"></div></div>
    </div>
    <div class="player p2">
      <div class="label"><span>Player 2 &nbsp;(gamepad axis 0)</span><span id="p2-val">90&deg;</span></div>
      <div class="track"><div class="center"></div><div id="p2-fill" class="fill"></div></div>
    </div>
    <div class="hint">Move the mouse to drive P1. Connect a gamepad for P2. Served angles are 0&ndash;180&deg;
      (90 = centre): opponent coupling plus a gentle wandering drift. Data at
      <a href="/api/controls">/api/controls</a>.</div>
  </div>
<script>
const throttle = {};
function post(player, x) {
  const now = performance.now();
  if (now - (throttle[player] || 0) < 60) return;   // cap ~16 posts/sec/player
  throttle[player] = now;
  fetch('/input/' + player, {
    method: 'POST', headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ x })
  }).catch(() => {});
}
addEventListener('mousemove', e => post('p1', (e.clientX / innerWidth) * 2 - 1));
function pollPads() {
  const pads = (navigator.getGamepads && navigator.getGamepads()) || [];
  if (pads[0]) post('p2', pads[0].axes[0] || 0);
  requestAnimationFrame(pollPads);
}
pollPads();
function setBar(id, deg) {
  document.getElementById(id + '-fill').style.left = (deg / 180 * 100) + '%';
  document.getElementById(id + '-val').textContent = Math.round(deg) + '°';
}
async function refresh() {
  try {
    const c = await (await fetch('/api/controls')).json();
    const m = await (await fetch('/api/motor')).json();
    setBar('p1', c.p1); setBar('p2', c.p2);
    const el = document.getElementById('motor');
    el.textContent = m.motor.toUpperCase();
    el.className = 'pill ' + m.motor;
  } catch (e) {}
}
setInterval(refresh, 150);
function motor(a) { fetch('/motor/' + a, { method: 'POST' }).catch(() => {}); }
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

    // Read the physical Xbox controller on the (headless) Pi.
    spawn_gamepad_reader(state.clone());
    // Slowly wander each player's neutral point (quantum uncertainty).
    spawn_drift(state.clone());

    let addr = ("0.0.0.0", 8080);
    println!("piwebserver: control panel  http://localhost:{}/", addr.1);
    println!("             controls API   http://localhost:{}/api/controls", addr.1);
    println!("             motor API      http://localhost:{}/api/motor", addr.1);

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
