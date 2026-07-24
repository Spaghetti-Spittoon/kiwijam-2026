//! Raspberry Pi web server (actix-web) — motor start/stop dashboard.
//!
//! Shows whether the motor should START or STOP, lets you toggle it, and
//! exposes a JSON endpoint the device can poll.
//!
//! The `MotorAction` enum below intentionally mirrors the one in the
//! `esp8266motorball` firmware (`wifi_inputs.rs`), whose `poll_server()` fetches
//! a `ServerResult`. The branches are kept separate on purpose (not merged), so
//! this copy is the web-side source of truth for the same contract:
//! `GET /api/motor` -> `{"motor":"start"|"stop"}`.

use std::sync::Mutex;

use actix_web::{App, HttpResponse, HttpServer, Responder, get, post, web};
use serde::Serialize;

/// Whether the motor should run. Mirrors `esp8266motorball::wifi_inputs::MotorAction`.
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

/// Shared, thread-safe server state.
struct AppState {
    motor: Mutex<MotorAction>,
}

/// JSON payload for `GET /api/motor` — what the ESP polls.
#[derive(Serialize)]
struct MotorStatus {
    motor: &'static str,
}

/// Status dashboard.
#[get("/")]
async fn index(data: web::Data<AppState>) -> impl Responder {
    let action = *data.motor.lock().unwrap();
    HttpResponse::Ok()
        .content_type("text/html; charset=utf-8")
        .body(render_page(action))
}

/// Machine-readable status the device polls.
#[get("/api/motor")]
async fn api_motor(data: web::Data<AppState>) -> impl Responder {
    let action = *data.motor.lock().unwrap();
    web::Json(MotorStatus {
        motor: action.as_str(),
    })
}

/// Set the motor state, then return to the dashboard.
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
            // 303 -> browser re-GETs the dashboard.
            HttpResponse::SeeOther()
                .insert_header(("Location", "/"))
                .finish()
        }
        None => HttpResponse::BadRequest().body("unknown motor action (use start|stop)"),
    }
}

fn render_page(action: MotorAction) -> String {
    let (label, accent, disabled_start, disabled_stop) = match action {
        MotorAction::Start => ("START", "#0aa06e", "disabled", ""),
        MotorAction::Stop => ("STOP", "#c0392b", "", "disabled"),
    };
    format!(
        r##"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <meta http-equiv="refresh" content="3">
  <title>Motor Control</title>
  <style>
    :root {{ color-scheme: light dark; }}
    body {{ font-family: system-ui, sans-serif; margin: 0; min-height: 100vh;
            display: grid; place-items: center; background: #11151c; color: #e6e6e6; }}
    .card {{ background: #1b2029; padding: 2.5rem 3rem; border-radius: 16px;
             box-shadow: 0 10px 40px rgba(0,0,0,.4); text-align: center; min-width: 280px; }}
    h1 {{ font-size: .95rem; letter-spacing: .15em; text-transform: uppercase;
          color: #8a93a6; margin: 0 0 1rem; }}
    .state {{ font-size: 3.2rem; font-weight: 800; letter-spacing: .05em;
              color: {accent}; margin: .2rem 0 1.6rem; }}
    .dot {{ display:inline-block; width:.7em; height:.7em; border-radius:50%;
            background:{accent}; margin-right:.4em; vertical-align:middle;
            box-shadow:0 0 12px {accent}; }}
    form {{ display: inline; }}
    button {{ font: inherit; font-weight: 600; padding: .6rem 1.4rem; margin: 0 .3rem;
              border: 0; border-radius: 10px; cursor: pointer; color: #fff; }}
    .start {{ background: #0aa06e; }} .stop {{ background: #c0392b; }}
    button[disabled] {{ opacity: .35; cursor: default; }}
    .api {{ margin-top: 1.6rem; font-size: .8rem; color: #6b7280; }}
    a {{ color: #7aa2f7; }}
  </style>
</head>
<body>
  <div class="card">
    <h1>Motor should</h1>
    <div class="state"><span class="dot"></span>{label}</div>
    <form method="post" action="/motor/start"><button class="start" {disabled_start}>Start</button></form>
    <form method="post" action="/motor/stop"><button class="stop" {disabled_stop}>Stop</button></form>
    <div class="api">device polls <a href="/api/motor">/api/motor</a></div>
  </div>
</body>
</html>"##
    )
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let state = web::Data::new(AppState {
        motor: Mutex::new(MotorAction::Stop),
    });

    let addr = ("0.0.0.0", 8080);
    println!("piwebserver: motor dashboard on http://localhost:{}/", addr.1);
    println!("             JSON status at   http://localhost:{}/api/motor", addr.1);

    HttpServer::new(move || {
        App::new()
            .app_data(state.clone())
            .service(index)
            .service(api_motor)
            .service(set_motor)
    })
    .bind(addr)?
    .run()
    .await
}
