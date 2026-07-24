# kiwijam-2026

## Flashing the ESP8266

```
rustup default stable-x86_64-unknown-linux-gnu
cargo install espup --locked
cargo install espflash --locked
cd esp32motorball
espup install
```

Every time you open a terminal, run `. /home/USER_NAME/export-esp.sh`.

```
cargo build --release
cargo install espflash
espflash flash target/xtensa-esp8266-none-elf/debug/esp8266motorball
```