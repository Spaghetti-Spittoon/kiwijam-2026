use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::io::AsRawFd;

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
    let mut tty_reader = BufReader::new(tty_in);

    writeln!(tty_out).unwrap();

    let ssid = prompt(&mut tty_out, &mut tty_reader, "SSID", false);
    let password = prompt(&mut tty_out, &mut tty_reader, "Password", true);

    assert!(!ssid.is_empty(), "SSID must not be empty");
    assert!(!password.is_empty(), "Password must not be empty");

    println!("cargo:rustc-env=WIFI_SSID={ssid}");
    println!("cargo:rustc-env=WIFI_PASSWORD={password}");
}

fn prompt<W: Write>(out: &mut W, input: &mut BufReader<File>, label: &str, hide: bool) -> String {
    write!(out, "\r\x1b[2K{label}: ").unwrap();
    out.flush().unwrap();

    let fd = input.get_ref().as_raw_fd();
    let restore = if hide { Some(disable_echo(fd)) } else { None };

    let mut line = String::new();
    input.read_line(&mut line).unwrap();

    if let Some(orig) = restore {
        restore_termios(fd, orig);
        writeln!(out).unwrap();
    }

    line.trim_end_matches(['\r', '\n']).to_string()
}

fn disable_echo(fd: i32) -> libc::termios {
    let mut term: libc::termios = unsafe { std::mem::zeroed() };
    let ok = unsafe { libc::tcgetattr(fd, &mut term) };
    assert_eq!(ok, 0, "tcgetattr failed");
    let orig = term;
    term.c_lflag &= !(libc::ECHO);
    let ok = unsafe { libc::tcsetattr(fd, libc::TCSANOW, &term) };
    assert_eq!(ok, 0, "tcsetattr failed");
    orig
}

fn restore_termios(fd: i32, orig: libc::termios) {
    unsafe { libc::tcsetattr(fd, libc::TCSANOW, &orig) };
}
