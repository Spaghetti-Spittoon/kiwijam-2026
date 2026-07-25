# Raspberry Pi web server and Uno PWM transmitter

`piwebserver` reads the two player inputs, calculates `n1,n2` in the range
0–255, and transmits both values to the Uno as independent, hardware-timed
50 Hz servo-style PWM signals.

## Pin assignment

| Player | Pi hardware PWM | BCM GPIO | Pi header pin | Uno input |
| --- | --- | --- | --- | --- |
| 1 | PWM0 | GPIO18 | 12 | D2 / INT0 |
| 2 | PWM1 | GPIO19 | 35 | D3 / INT1 |

Each control value is converted with:

```text
pulse_us = 1000 + value * 1000 / 255
```

Thus 0 produces approximately 1000 us HIGH, 128 approximately 1500 us, and
255 exactly 2000 us. RPPAL programs the Pi PWM peripheral with a 20 ms period;
ordinary Linux sleep timing is not used to form the pulses.

## Raspberry Pi setup

On the Raspberry Pi 3B+, add these lines to `/boot/config.txt`:

```ini
dtparam=audio=off
dtoverlay=pwm-2chan
```

The `pwm-2chan` overlay defaults to PWM0 on GPIO18 and PWM1 on GPIO19. Analog
audio must be disabled because it otherwise uses the same PWM channels. Reboot
after changing the boot configuration.

The process must have permission to use `/sys/class/pwm`. Run the deployed
service with suitable PWM permissions (or as root). Startup deliberately fails
with a descriptive error if either hardware channel cannot be opened, so the
web server cannot appear healthy while control output is missing.

For development on a Linux machine that is not a Pi, set
`PIWS_DISABLE_SERVO_PWM=1`. Non-Linux builds automatically omit the Pi hardware
backend.

## Electrical connection

Pi GPIO is 3.3 V. Route GPIO18 and GPIO19 through separate, validated
non-inverting 3.3 V-to-5 V level-shifter/buffer channels before Uno D2 and D3.
Add a 10 kΩ pull-down at each Uno input and join the Pi, Uno, level shifter,
servo supply, and servo grounds. Do not power either servo from a Pi GPIO pin;
use an external 5 V supply rated for both servos.

The Uno firmware then drives Player 1's servo signal on D9 and Player 2's on
D10.
