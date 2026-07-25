# Raspberry Pi web server and Uno USB serial control

`piwebserver` reads the two player inputs, calculates `n1,n2` in the range
0–255, and sends both values to the Uno over its USB serial connection.
The Uno—not Linux—generates both stable 50 Hz servo signals.

## Connections

- Connect the Uno USB port to the Raspberry Pi.
- Uno D9 is Player 1's servo signal.
- Uno D10 is Player 2's servo signal.
- Power both servos from a regulated external 5 V supply rated for their
  combined current.
- Join the external supply ground to Uno ground.
- Do not connect Pi GPIO18/GPIO19 to Uno D2/D3; they are not used by the serial
  transport.

USB powers the Uno but must not be used to power both servos.

## Serial protocol

The default device is `/dev/ttyACM0` at 115200 baud. Every 50 ms the Pi sends:

```text
0xA5 | sequence | player1 | player2 | CRC-8
```

CRC-8 uses polynomial `0x07` over the first four bytes. Opening the serial port
resets an Uno, so the server waits two seconds for its bootloader. It reconnects
automatically after unplugging. The Uno returns both servos to centre if valid
frames stop for approximately 250 ms.

For a persistent udev symlink, override the path in the systemd service:

```ini
Environment=PIWS_UNO_SERIAL=/dev/serial/by-id/<exact-device-name>
```

Use the exact path reported by:

```bash
ls -l /dev/serial/by-id/
```

The service currently runs as root and therefore has serial-device permission.
For a non-root service, add its user to the `dialout` group.

The earlier GPIO PWM transport is no longer used. `dtoverlay=pwm-2chan` and
`dtparam=audio=off` may be removed from `/boot/config.txt` if analog audio is
needed again; changing boot configuration requires one reboot.
