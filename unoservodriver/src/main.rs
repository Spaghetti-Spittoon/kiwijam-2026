//! Arduino Uno servo driver — AVR firmware for Wokwi and a real Uno.
//!
//! Timer1 produces a hardware-timed 50 Hz hobby-servo signal on D9.  The Pi
//! sends TTL-level, servo-style PWM to D2: 1000 µs means 0°, 1500 µs means
//! centre, and 2000 µs means 180°.  D2/INT0 measures the input in an interrupt,
//! independently of the D9 output waveform.
//!
//! # Pi-to-Uno wiring and protocol
//!
//! - Pi GPIO (PWM output) -> level shifter/buffer -> Uno D2.  Emit one positive
//!   pulse every 20 ms (50 Hz), with a width from 1000 to 2000 µs.
//! - Add a 10 kΩ pull-down from Uno D2 to GND so a disconnected input is LOW.
//! - Pi GND -> Uno GND.  A shared ground is required; do *not* connect the Pi
//!   GPIO to the Uno without it.
//! - Uno D9 -> servo signal; power the servo from an appropriately rated 5 V
//!   supply and connect that supply's ground to Uno/Pi ground.
//! - Uno D13 -> onboard status LED; ON means valid D2 pulses are arriving.
//!   OFF means the input has timed out.
//! - A 3.3 V Pi GPIO is not guaranteed to meet the ATmega328P's HIGH threshold
//!   when the Uno runs at 5 V.  The final circuit therefore needs a validated
//!   3.3 V-to-5 V level shifter/buffer (or a confirmed 3.3 V Uno design).
//!
//! If valid control pulses stop, the servo returns to centre after 100–120 ms
//! (the timeout is checked on 20 ms frame boundaries).  The output remains
//! exactly 50 Hz while input measurement and fail-safe handling occur
//! asynchronously.
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

struct InterruptState {
    status_led: Pin<Output>,
    rise_count: u16,
    awaiting_fall: bool,
    frames_since_valid: u8,
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
    // subsequently changed only by the INT0/TIMER1_OVF handlers.
    unsafe { &*arduino_hal::pac::EXINT::ptr() }
}

// D2/INT0 alternates between rising- and falling-edge detection.  Timer1's
// free-running counter timestamps both edges with 0.5 µs resolution.
#[avr_device::interrupt(atmega328p)]
fn INT0() {
    let state = interrupt_state();
    let now = timer1().tcnt1().read().bits();

    if state.awaiting_fall {
        let width = if now >= state.rise_count {
            now - state.rise_count
        } else {
            FRAME_COUNTS - state.rise_count + now
        };

        if (MIN_CONTROL_COUNTS..=MAX_CONTROL_COUNTS).contains(&width) {
            timer1().ocr1a().write(|w| w.set(width));
            state.frames_since_valid = 0;
            state.status_led.set_high();
        }

        state.awaiting_fall = false;
        external_interrupts()
            .eicra()
            .modify(|_, w| w.isc0().set(0b11)); // next rising edge
    } else {
        state.rise_count = now;
        state.awaiting_fall = true;
        external_interrupts()
            .eicra()
            .modify(|_, w| w.isc0().set(0b10)); // next falling edge
    }
}

// Timer1 overflows exactly once per 20 ms servo frame.  It therefore provides
// a timeout clock without disturbing the hardware-generated D9 waveform.
#[avr_device::interrupt(atmega328p)]
fn TIMER1_OVF() {
    let state = interrupt_state();

    if state.frames_since_valid < TIMEOUT_FRAMES {
        state.frames_since_valid += 1;
    }

    if state.frames_since_valid >= TIMEOUT_FRAMES {
        timer1().ocr1a().write(|w| w.set(CENTRE_COUNTS));
        state.status_led.set_low();

        // Recover if an input rose but never fell.
        state.awaiting_fall = false;
        external_interrupts()
            .eicra()
            .modify(|_, w| w.isc0().set(0b11));
    }
}

#[arduino_hal::entry]
fn main() -> ! {
    let dp = arduino_hal::Peripherals::take().unwrap();
    let pins = arduino_hal::pins!(dp);

    // These conversions configure the corresponding ATmega328P DDR bits.
    // D9 is OC1A, D2 is INT0, and D13 is the Uno's onboard LED.
    pins.d9.into_output();
    pins.d2.into_floating_input();
    let mut status_led = pins.d13.into_output().downgrade();
    status_led.set_low();

    unsafe {
        *INTERRUPT_STATE.0.get() = mem::MaybeUninit::new(InterruptState {
            status_led,
            rise_count: 0,
            awaiting_fall: false,
            frames_since_valid: TIMEOUT_FRAMES,
        });
        atomic::compiler_fence(atomic::Ordering::SeqCst);
    }

    // Fast PWM mode 14: TOP=ICR1, non-inverting OC1A/D9, prescaler 8.
    // 16 MHz / 8 / 40_000 = exactly 50 Hz.
    dp.TC1.icr1().write(|w| w.set(TIMER_TOP));
    dp.TC1.ocr1a().write(|w| w.set(CENTRE_COUNTS));
    dp.TC1
        .tccr1a()
        .write(|w| w.wgm1().set(0b10).com1a().match_clear());
    dp.TC1
        .tccr1b()
        .write(|w| w.wgm1().set(0b11).cs1().prescale_8());
    dp.TC1.timsk1().write(|w| w.toie1().set_bit());

    // Trigger INT0 on the first rising edge, then let the ISR alternate edges.
    dp.EXINT.eicra().write(|w| w.isc0().set(0b11));
    dp.EXINT.eimsk().write(|w| w.int0().set_bit());

    unsafe { avr_device::interrupt::enable() };

    loop {
        avr_device::asm::sleep();
    }
}
