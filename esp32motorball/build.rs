use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};

fn main() {
    println!("cargo:rerun-if-changed=NULL");
    println!("cargo:rustc-link-arg-bins=-Tlinkall.x");

    let mut tty_out = OpenOptions::new()
        .write(true)
        .open("/dev/tty")
        .expect("failed to open /dev/tty for writing (build must run in a real terminal)");
    let tty_in = OpenOptions::new()
        .read(true)
        .open("/dev/tty")
        .expect("failed to open /dev/tty for reading (build must run in a real terminal)");
    let mut tty_in = BufReader::new(tty_in);

    let ssid = prompt(&mut tty_out, &mut tty_in, "SSID");
    let username = prompt(&mut tty_out, &mut tty_in, "Username");
    let password = prompt(&mut tty_out, &mut tty_in, "Password");

    assert!(!ssid.is_empty(), "SSID must not be empty");
    assert!(!username.is_empty(), "Username must not be empty");
    assert!(!password.is_empty(), "Password must not be empty");

    println!("cargo:rustc-env=WIFI_SSID={ssid}");
    println!("cargo:rustc-env=WIFI_USERNAME={username}");
    println!("cargo:rustc-env=WIFI_PASSWORD={password}");
}

fn prompt<W: Write, R: BufRead>(out: &mut W, input: &mut R, label: &str) -> String {
    write!(out, "{label}: ").unwrap();
    out.flush().unwrap();

    let mut line = String::new();
    input.read_line(&mut line).unwrap();
    line.trim_end_matches(['\r', '\n']).to_string()
}
