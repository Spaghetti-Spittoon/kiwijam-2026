use std::io::{self, BufRead, Write};

fn main() {
    println!("cargo:rerun-if-changed=NULL");

    let ssid = prompt("SSID");
    let username = prompt("Username");
    let password = prompt("Password");

    println!("cargo:rustc-env=WIFI_SSID={ssid}");
    println!("cargo:rustc-env=WIFI_USERNAME={username}");
    println!("cargo:rustc-env=WIFI_PASSWORD={password}");

    println!("cargo:rustc-link-arg-bins=-Tlinkall.x");
}

fn prompt(label: &str) -> String {
    let stderr = io::stderr();
    let mut stderr = stderr.lock();
    write!(stderr, "{label}: ").unwrap();
    stderr.flush().unwrap();

    let stdin = io::stdin();
    let mut line = String::new();
    stdin.lock().read_line(&mut line).unwrap();
    line.trim_end_matches(['\r', '\n']).to_string()
}
