//! Raspberry Pi sensor-input web server (actix-web) — issue #20.
//!
//! Reads controller / mouse X-axis input from a browser client, stores the
//! latest value per player in memory, applies "Quantum Entanglement" (a control
//! nudges the opposing player's axis the opposite way), and serves the result
//! over HTTP.
//!
//! We deliberately do NOT send TTL signals to the Arduino Uno. Instead the web
//! server *is* the controller-data source: consumers poll the API.
//!
//! Endpoints:
//!   GET  /                   -> client page: captures mouse + Gamepad X, live view
//!   POST /input/{p1|p2}      -> body {"x": -1.0..1.0}; store + entangle opponent
//!   GET  /api/controls       -> {"p1": x, "p2": x}   (replaces TTL-to-Uno)
//!   GET  /api/motor          -> {"motor": "start"|"stop"}
//!   POST /motor/{start|stop} -> set run/stop
//!
//! `MotorAction` mirrors `esp8266motorball::wifi_inputs::MotorAction` (the ESP's
//! `poll_server()` contract), kept in sync by hand — branches aren't merged.

use std::sync::Mutex;

use actix_web::{App, HttpResponse, HttpServer, Responder, get, post, web};
use gilrs::{Axis, Gilrs};
use serde::{Deserialize, Serialize};

/// How strongly a control tugs the opposing player's axis the opposite way.
const ENTANGLE: f32 = 0.15;

/// Ignore small resting-stick drift near centre.
const DEADZONE: f32 = 0.12;

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

/// Latest RAW X-axis (-1.0..=1.0) per player, before entanglement.
#[derive(Clone, Copy)]
struct Controls {
    p1: f32,
    p2: f32,
}

/// Entangled X-axis output served to clients/devices.
#[derive(Serialize)]
struct ControlsOut {
    p1: f32,
    p2: f32,
}

struct AppState {
    motor: Mutex<MotorAction>,
    controls: Mutex<Controls>,
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

/// Quantum Entanglement: each player's served axis is nudged in the opposite
/// direction of the opponent's raw value. Derived from the raw inputs (not
/// stored) so a continuous 50 Hz controller stream doesn't compound the nudge.
fn entangled(raw: Controls) -> ControlsOut {
    ControlsOut {
        p1: clamp_axis(raw.p1 - ENTANGLE * raw.p2),
        p2: clamp_axis(raw.p2 - ENTANGLE * raw.p1),
    }
}

/// Store a player's RAW X-axis; entanglement is applied when the data is read.
#[post("/input/{player}")]
async fn set_input(
    path: web::Path<String>,
    body: web::Json<AxisInput>,
    data: web::Data<AppState>,
) -> impl Responder {
    let x = clamp_axis(body.x);
    let mut c = data.controls.lock().unwrap();
    match path.into_inner().as_str() {
        "p1" => c.p1 = x,
        "p2" => c.p2 = x,
        _ => return HttpResponse::BadRequest().body("unknown player (use p1|p2)"),
    }
    HttpResponse::Ok().json(entangled(*c))
}

/// Controller data the device polls (instead of receiving TTL).
#[get("/api/controls")]
async fn api_controls(data: web::Data<AppState>) -> impl Responder {
    let raw = *data.controls.lock().unwrap();
    web::Json(entangled(raw))
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
      <div class="label"><span>Player 1 &nbsp;(mouse X)</span><span id="p1-val">0.00</span></div>
      <div class="track"><div class="center"></div><div id="p1-fill" class="fill"></div></div>
    </div>
    <div class="player p2">
      <div class="label"><span>Player 2 &nbsp;(gamepad axis 0)</span><span id="p2-val">0.00</span></div>
      <div class="track"><div class="center"></div><div id="p2-fill" class="fill"></div></div>
    </div>
    <div class="hint">Move the mouse to drive P1. Connect a gamepad for P2. Quantum entanglement
      tugs the opponent the opposite way. Data at <a href="/api/controls">/api/controls</a>.</div>
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
function setBar(id, x) {
  document.getElementById(id + '-fill').style.left = ((x + 1) / 2 * 100) + '%';
  document.getElementById(id + '-val').textContent = x.toFixed(2);
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
    });

    // Read the physical Xbox controller on the (headless) Pi.
    spawn_gamepad_reader(state.clone());

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
