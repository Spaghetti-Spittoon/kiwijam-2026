//! Arduino Uno dual-servo receiver for Raspberry Pi USB serial control.
//!
//! The Pi sends five-byte frames at 115200 baud over the Uno USB connection:
//! `0xA5, sequence, player1, player2, CRC-8`. Player values span 0..=255.
//! Timer1 independently generates stable 50 Hz servo signals on D9 and D10.
//! If valid frames stop for roughly 250 ms, both servos return to centre.

#![no_std]
#![no_main]
#![feature(abi_avr_interrupt)]

use arduino_hal::port::{mode::Output, Pin};
use arduino_hal::prelude::*;
use core::{cell::UnsafeCell, mem, sync::atomic};
use panic_halt as _;

const SYNC: u8 = 0xA5;
const PACKET_LEN: usize = 5;
const TIMER_TOP: u16 = 39_999;
const MIN_COUNTS: u16 = 2_000;
const CENTRE_COUNTS: u16 = 3_000;
const COUNT_RANGE: u16 = 2_000;
const TIMEOUT_FRAMES: u8 = 13;

struct ReceiverState {
    status_led: Pin<Output>,
    frames_since_valid: u8,
}

struct GlobalState(UnsafeCell<mem::MaybeUninit<ReceiverState>>);

// SAFETY: Initialized before interrupts are enabled. Main accesses it only
// inside an interrupt-free section; the Timer1 handler cannot nest.
unsafe impl Sync for GlobalState {}

static STATE: GlobalState = GlobalState(UnsafeCell::new(mem::MaybeUninit::uninit()));

fn state() -> &'static mut ReceiverState {
    // SAFETY: The access rules documented on GlobalState are upheld.
    unsafe { &mut *(*STATE.0.get()).as_mut_ptr() }
}

fn timer1() -> &'static arduino_hal::pac::tc1::RegisterBlock {
    // SAFETY: Timer1 is initialized before interrupts and all later access is
    // either from its handler or an interrupt-free main section.
    unsafe { &*arduino_hal::pac::TC1::ptr() }
}

fn crc8(bytes: &[u8]) -> u8 {
    let mut crc = 0;
    for &byte in bytes {
        crc ^= byte;
        for _ in 0..8 {
            crc = if crc & 0x80 != 0 {
                (crc << 1) ^ 0x07
            } else {
                crc << 1
            };
        }
    }
    crc
}

fn servo_counts(value: u8) -> u16 {
    MIN_COUNTS + (value as u32 * COUNT_RANGE as u32 / 255) as u16
}

struct PacketParser {
    bytes: [u8; PACKET_LEN],
    len: usize,
}

impl PacketParser {
    const fn new() -> Self {
        Self {
            bytes: [0; PACKET_LEN],
            len: 0,
        }
    }

    fn push(&mut self, byte: u8) -> Option<(u8, u8)> {
        if self.len == 0 {
            if byte == SYNC {
                self.bytes[0] = byte;
                self.len = 1;
            }
            return None;
        }

        self.bytes[self.len] = byte;
        self.len += 1;
        if self.len < PACKET_LEN {
            return None;
        }

        self.len = 0;
        if crc8(&self.bytes[..PACKET_LEN - 1]) == self.bytes[PACKET_LEN - 1] {
            Some((self.bytes[2], self.bytes[3]))
        } else {
            if byte == SYNC {
                self.bytes[0] = SYNC;
                self.len = 1;
            }
            None
        }
    }
}

#[avr_device::interrupt(atmega328p)]
fn TIMER1_OVF() {
    let receiver = state();
    receiver.frames_since_valid = receiver.frames_since_valid.saturating_add(1);
    if receiver.frames_since_valid >= TIMEOUT_FRAMES {
        timer1().ocr1a().write(|w| w.set(CENTRE_COUNTS));
        timer1().ocr1b().write(|w| w.set(CENTRE_COUNTS));
        receiver.status_led.set_low();
    }
}

#[arduino_hal::entry]
fn main() -> ! {
    let dp = arduino_hal::Peripherals::take().unwrap();
    let pins = arduino_hal::pins!(dp);

    pins.d9.into_output();
    pins.d10.into_output();
    let mut status_led = pins.d13.into_output().downgrade();
    status_led.set_low();

    unsafe {
        *STATE.0.get() = mem::MaybeUninit::new(ReceiverState {
            status_led,
            frames_since_valid: TIMEOUT_FRAMES,
        });
        atomic::compiler_fence(atomic::Ordering::SeqCst);
    }

    // Fast PWM mode 14, 16 MHz / 8 / 40_000 = exactly 50 Hz.
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

    let mut serial = arduino_hal::default_serial!(dp, pins, 115_200);
    let mut parser = PacketParser::new();
    unsafe { avr_device::interrupt::enable() };

    loop {
        if let Ok(byte) = serial.read() {
            if let Some((player1, player2)) = parser.push(byte) {
                avr_device::interrupt::free(|_| {
                    timer1().ocr1a().write(|w| w.set(servo_counts(player1)));
                    timer1().ocr1b().write(|w| w.set(servo_counts(player2)));
                    let receiver = state();
                    receiver.frames_since_valid = 0;
                    receiver.status_led.set_high();
                });
            }
        }
    }
}
