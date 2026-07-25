# kiwijam-2026

The Raspberry Pi server's two-player hardware PWM setup and Pi-to-Uno wiring
are documented in [`piwebserver/README.md`](piwebserver/README.md).

## Flashing the ESP8266

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

To just run the device with the latest firmware:

```
sudo chmod 666 /dev/ttyUSB0
cargo run
```
