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

## Game countdown

Hold the controller button reported by `gilrs` as `West` (the physical Y button
on the project controller) firmly for five continuous seconds to start or
restart a game. One hold can trigger only once; release Y before starting
another game. A game lasts 60 seconds by default, starts the motor, and stops
the motor when time expires.

Read the current timer state:

```bash
curl http://localhost:7777/api/countdown
```

Start a game without the controller:

```bash
curl -X POST http://localhost:7777/game/start
```

The JSON response includes `state`, `remaining_seconds`, `duration_seconds`,
and the incrementing `game` number. The periodic service log reports controls,
motor state, and `/api/countdown` together on one line. Game start and finish
events are also logged:

```bash
journalctl -u piwebserver -f
```

Set a different game duration in the systemd service with, for example,
`Environment=PIWS_GAME_SECONDS=90`, then run `systemctl daemon-reload` and
restart the service.
For a non-root service, add its user to the `dialout` group.

The earlier GPIO PWM transport is no longer used. `dtoverlay=pwm-2chan` and
`dtparam=audio=off` may be removed from `/boot/config.txt` if analog audio is
needed again; changing boot configuration requires one reboot.
