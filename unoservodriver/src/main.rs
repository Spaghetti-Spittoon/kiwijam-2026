//! Arduino Uno servo driver — AVR firmware for Wokwi and a real Uno.
//!
//! Timer1 produces hardware-timed 50 Hz hobby-servo signals for both players.
//! The Pi sends independent TTL-level, servo-style PWM controls: D2/INT0 drives
//! the Player 1 servo on D9/OC1A, while D3/INT1 drives the Player 2 servo on
//! D10/OC1B.  For both channels, 1000 µs means 0°, 1500 µs means centre, and
//! 2000 µs means 180°.
//!
//! # Pi-to-Uno wiring and protocol
//!
//! - Pi Player 1 PWM -> level shifter/buffer -> Uno D2.
//! - Pi Player 2 PWM -> level shifter/buffer -> Uno D3.
//! - Emit one positive pulse per player every 20 ms (50 Hz), with a width from
//!   1000 to 2000 µs.
//! - Add separate 10 kΩ pull-downs from D2 and D3 to GND so disconnected inputs
//!   have a defined LOW state.
//! - Pi GND -> Uno GND.  A shared ground is required; do *not* connect the Pi
//!   GPIO to the Uno without it.
//! - Uno D9 -> Player 1 servo signal; Uno D10 -> Player 2 servo signal.
//! - Power both servos from an appropriately rated external 5 V supply and
//!   connect that supply's ground to Uno/Pi ground.
//! - Uno D13 -> onboard status LED; ON means valid pulses are arriving for both
//!   players.  OFF means either control input has timed out.
//! - A 3.3 V Pi GPIO is not guaranteed to meet the ATmega328P's HIGH threshold
//!   when the Uno runs at 5 V.  The final circuit therefore needs a validated
//!   3.3 V-to-5 V level shifter/buffer (or a confirmed 3.3 V Uno design).
//!
//! If valid control pulses stop on either channel, only that player's servo
//! returns to centre after 100–120 ms (the timeout is checked on 20 ms frame
//! boundaries).  Both outputs remain exactly 50 Hz while input measurement and
//! fail-safe handling occur asynchronously.
//!
//! The merged ESP template also referred to GPIO1 (TX) and GPIO3 (RX).  Those
//! are ESP UART0 assignments, not Arduino Uno UART assignments.  On the Uno,
//! hardware serial would use D1/TX and D0/RX; neither is used by this PWM
//! control implementation.

#![no_std]
#![no_main]
#![feature(abi_avr_interrupt)]

use arduino_hal::port::{mode::Output, Pin};
use core::{cell::UnsafeCell, mem, sync::atomic};
use panic_halt as _;

// Timer1 runs at 2 MHz (16 MHz / 8), so each count is 0.5 µs.
const TIMER_COUNTS_PER_US: u16 = 2;
const FRAME_US: u16 = 20_000;
const FRAME_COUNTS: u16 = FRAME_US * TIMER_COUNTS_PER_US;
const TIMER_TOP: u16 = FRAME_COUNTS - 1;
const MIN_CONTROL_COUNTS: u16 = 1_000 * TIMER_COUNTS_PER_US;
const CENTRE_COUNTS: u16 = 1_500 * TIMER_COUNTS_PER_US;
const MAX_CONTROL_COUNTS: u16 = 2_000 * TIMER_COUNTS_PER_US;
const TIMEOUT_FRAMES: u8 = 6; // guarantees at least 100 ms; checked every 20 ms

struct ControlChannel {
    rise_count: u16,
    rise_overflows: u8,
    awaiting_fall: bool,
    frames_since_valid: u8,
}

impl ControlChannel {
    const fn timed_out() -> Self {
        Self {
            rise_count: 0,
            rise_overflows: 0,
            awaiting_fall: false,
            frames_since_valid: TIMEOUT_FRAMES,
        }
    }
}

struct InterruptState {
    status_led: Pin<Output>,
    player1: ControlChannel,
    player2: ControlChannel,
}

struct GlobalInterruptState(UnsafeCell<mem::MaybeUninit<InterruptState>>);

// SAFETY: The state is initialized before interrupts are enabled and is only
// accessed by non-nesting AVR interrupt handlers after that point.
unsafe impl Sync for GlobalInterruptState {}

static INTERRUPT_STATE: GlobalInterruptState =
    GlobalInterruptState(UnsafeCell::new(mem::MaybeUninit::uninit()));

fn interrupt_state() -> &'static mut InterruptState {
    // SAFETY: main initializes the state and executes a compiler fence before
    // enabling interrupts. AVR interrupt handlers do not nest by default.
    unsafe { &mut *(*INTERRUPT_STATE.0.get()).as_mut_ptr() }
}

fn timer1() -> &'static arduino_hal::pac::tc1::RegisterBlock {
    // SAFETY: Interrupt handlers are the only code accessing Timer1 after
    // initialization, and AVR interrupts do not nest.
    unsafe { &*arduino_hal::pac::TC1::ptr() }
}

fn external_interrupts() -> &'static arduino_hal::pac::exint::RegisterBlock {
    // SAFETY: EXINT is configured before global interrupts are enabled and is
    // subsequently changed only by the non-nesting INT0 and INT1 handlers.
    unsafe { &*arduino_hal::pac::EXINT::ptr() }
}

/// Records one edge for a player and returns a valid completed pulse width.
fn capture_edge(channel: &mut ControlChannel, now: u16) -> Option<u16> {
    if !channel.awaiting_fall {
        channel.rise_count = now;
        channel.rise_overflows = 0;
        channel.awaiting_fall = true;
        return None;
    }

    let width = if now >= channel.rise_count {
        now - channel.rise_count
    } else {
        FRAME_COUNTS - channel.rise_count + now
    };

    channel.awaiting_fall = false;
    if channel.rise_overflows <= 1 && (MIN_CONTROL_COUNTS..=MAX_CONTROL_COUNTS).contains(&width) {
        channel.frames_since_valid = 0;
        Some(width)
    } else {
        None
    }
}

fn update_status_led(state: &mut InterruptState) {
    let both_healthy = state.player1.frames_since_valid < TIMEOUT_FRAMES
        && state.player2.frames_since_valid < TIMEOUT_FRAMES;
    if both_healthy {
        state.status_led.set_high();
    } else {
        state.status_led.set_low();
    }
}

/// Advances one channel's timeout and returns whether its servo must centre.
fn advance_timeout(channel: &mut ControlChannel) -> bool {
    if channel.awaiting_fall {
        channel.rise_overflows = channel.rise_overflows.saturating_add(1);
    }
    if channel.frames_since_valid < TIMEOUT_FRAMES {
        channel.frames_since_valid += 1;
    }
    channel.frames_since_valid >= TIMEOUT_FRAMES
}

// D2/INT0 measures the Player 1 control pulse.
#[avr_device::interrupt(atmega328p)]
fn INT0() {
    let state = interrupt_state();
    let now = timer1().tcnt1().read().bits();
    let was_awaiting_fall = state.player1.awaiting_fall;

    if let Some(width) = capture_edge(&mut state.player1, now) {
        timer1().ocr1a().write(|w| w.set(width));
    }
    if was_awaiting_fall {
        external_interrupts()
            .eicra()
            .modify(|_, w| w.isc0().set(0b11)); // next rising edge
    } else {
        external_interrupts()
            .eicra()
            .modify(|_, w| w.isc0().set(0b10)); // next falling edge
    }
    update_status_led(state);
}

// D3/INT1 measures the Player 2 control pulse.
#[avr_device::interrupt(atmega328p)]
fn INT1() {
    let state = interrupt_state();
    let now = timer1().tcnt1().read().bits();
    let was_awaiting_fall = state.player2.awaiting_fall;

    if let Some(width) = capture_edge(&mut state.player2, now) {
        timer1().ocr1b().write(|w| w.set(width));
    }
    if was_awaiting_fall {
        external_interrupts()
            .eicra()
            .modify(|_, w| w.isc1().set(0b11)); // next rising edge
    } else {
        external_interrupts()
            .eicra()
            .modify(|_, w| w.isc1().set(0b10)); // next falling edge
    }
    update_status_led(state);
}

// Timer1 overflows exactly once per 20 ms servo frame.  It therefore provides
// a timeout clock without disturbing either hardware-generated PWM waveform.
#[avr_device::interrupt(atmega328p)]
fn TIMER1_OVF() {
    let state = interrupt_state();

    if advance_timeout(&mut state.player1) {
        timer1().ocr1a().write(|w| w.set(CENTRE_COUNTS));
    }

    if advance_timeout(&mut state.player2) {
        timer1().ocr1b().write(|w| w.set(CENTRE_COUNTS));
    }

    update_status_led(state);
}

#[arduino_hal::entry]
fn main() -> ! {
    let dp = arduino_hal::Peripherals::take().unwrap();
    let pins = arduino_hal::pins!(dp);

    // These conversions configure the corresponding ATmega328P DDR bits.
    // D9/D10 are Timer1 outputs OC1A/OC1B, D2/D3 are INT0/INT1, and D13 is
    // the Uno's onboard status LED.
    pins.d9.into_output();
    pins.d10.into_output();
    pins.d2.into_floating_input();
    pins.d3.into_floating_input();
    let mut status_led = pins.d13.into_output().downgrade();
    status_led.set_low();

    unsafe {
        *INTERRUPT_STATE.0.get() = mem::MaybeUninit::new(InterruptState {
            status_led,
            player1: ControlChannel::timed_out(),
            player2: ControlChannel::timed_out(),
        });
        atomic::compiler_fence(atomic::Ordering::SeqCst);
    }

    // Fast PWM mode 14: TOP=ICR1, non-inverting OC1A/D9 and OC1B/D10,
    // prescaler 8.
    // 16 MHz / 8 / 40_000 = exactly 50 Hz.
    dp.TC1.icr1().write(|w| w.set(TIMER_TOP));
    dp.TC1.ocr1a().write(|w| w.set(CENTRE_COUNTS));
    dp.TC1.ocr1b().write(|w| w.set(CENTRE_COUNTS));
    dp.TC1.tccr1a().write(|w| {
        w.wgm1()
            .set(0b10)
            .com1a()
            .match_clear()
            .com1b()
            .match_clear()
    });
    dp.TC1
        .tccr1b()
        .write(|w| w.wgm1().set(0b11).cs1().prescale_8());
    dp.TC1.timsk1().write(|w| w.toie1().set_bit());

    // Trigger both inputs on their first rising edge, then alternate edges.
    dp.EXINT
        .eicra()
        .write(|w| w.isc0().set(0b11).isc1().set(0b11));
    dp.EXINT
        .eimsk()
        .write(|w| w.int0().set_bit().int1().set_bit());

    unsafe { avr_device::interrupt::enable() };

    loop {
        avr_device::asm::sleep();
    }
}
