# kiwijam-2026

This project was created by Sam, Charisse, Yousongsun, Amer.

## Esp32motorball

Drives the motors inside the rolling ball and can accept stop / start commands from the web server. It blinks an LED on `GPIO2` every second if wifi connected and it requests to the webserver succeed.

To setup a linux environment for flashing the ESP32 with the Rust firmware:

```
rustup default stable-x86_64-unknown-linux-gnu
cargo install espup --locked
cargo install espflash --locked
cd esp32motorball
espup install
```

Every time you open a terminal, run `. /home/USER_NAME/export-esp.sh`.

To flash the MCU:

```
cargo build --release
cargo install espflash
espflash flash target/xtensa-esp8266-none-elf/debug/esp8266motorball
```

OR you can just run the device with the latest firmware:

```
sudo chmod 666 /dev/ttyUSB0
cargo run
```

At compile time, you will be prompted for 

- Wifi connection:

    - SSID, 
    - Password, 
    
- Piwebserver connection:

    - ip address + port combined string. (eg: localhost:7777)


## Piwebserver

Handles repluggable USB mice and gamepad controllers as input for the Motorball pong game.

By default, the webserver is served on localhost equivalent to the loopback address `http://127.0.0.1:7777`.

To prepare the program for reading mouse input without permissions, run these commands:

```
# 1. Ensure the 'input' group exists (it usually does; harmless if it already exists)
sudo groupadd -f input

# 2. Add your user to it
sudo usermod -aG input $USER

# 3. Install a udev rule so /dev/input/event* nodes get group=input, mode=0660
echo 'KERNEL=="event*", SUBSYSTEM=="input", GROUP="input", MODE="0660"' \
| sudo tee /etc/udev/rules.d/70-input.rules

# 4. Reload udev and re-trigger existing input nodes so the rule applies now
sudo udevadm control --reload
sudo udevadm trigger --subsystem-match=input
```

log out then log back in for verification

```
groups | grep -o input          # should print: input
ls -l /dev/input/event24        # should show: crw-rw---- root input
cargo run                        # no sudo needed
```

or test in a new shell, without logging out

```
newgrp input
```
