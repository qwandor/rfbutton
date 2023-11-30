// Copyright 2023 the rfbutton authors.
// This project is dual-licensed under Apache 2.0 and MIT terms.
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::time::{Duration, Instant};

use eyre::{bail, Context, Report};
use log::{debug, trace};
use rfbutton::decode;
use rppal::gpio::{Gpio, InputPin, Level, Trigger};

/// The GPIO pin to which the 433 MHz receiver's data pin is connected.
const RX_PIN: u8 = 27;

const MAX_PULSE_LENGTH: Duration = Duration::from_millis(10);
const BREAK_PULSE_LENGTH: Duration = Duration::from_millis(7);

fn main() -> Result<(), Report> {
    color_eyre::install()?;
    pretty_env_logger::init();
    color_backtrace::install();

    let gpio = Gpio::new()?;
    let mut rx_pin = gpio.get(RX_PIN)?.into_input();

    rx_pin.set_interrupt(Trigger::Both)?;

    loop {
        match receive(&mut rx_pin) {
            Ok(pulses) => {
                if pulses.len() > 10 {
                    println!("{} pulses: {:?}...", pulses.len(), &pulses[0..10]);
                } else {
                    println!("{} pulses: {:?}", pulses.len(), pulses);
                }
                match decode(&pulses) {
                    Ok(code) => {
                        if code.length > 0 {
                            println!("Decoded: {:?}", code);
                            break;
                        } else {
                            println!("Decoded 0 bits.");
                        }
                    }
                    Err(e) => {
                        println!("Decode error: {}", e);
                    }
                }
            }
            Err(e) => {
                println!("Receive error: {}", e);
            }
        }
    }

    Ok(())
}

/// Wait for a single code.
fn receive(rx_pin: &mut InputPin) -> Result<Vec<u16>, Report> {
    debug!("Waiting for interrupt...");
    let level = rx_pin.poll_interrupt(false, None)?;
    if level.is_none() {
        bail!("Unexpected initial level {:?}", level);
    }
    debug!("Initial level: {:?}", level);
    let mut last_timestamp = Instant::now();
    let mut pulses = Vec::new();

    debug!("Waiting for initial break pulse...");
    // Wait for a long pulse to start.
    let mut last_pulse = Duration::default();
    while let Some(level) = rx_pin.poll_interrupt(false, None)? {
        let timestamp = Instant::now();
        let pulse_length = timestamp - last_timestamp;
        last_timestamp = timestamp;

        if level == Level::High && pulse_length > BREAK_PULSE_LENGTH {
            trace!(
                "Found possible initial break pulse {} μs.",
                pulse_length.as_micros()
            );
        } else if level == Level::Low
            && last_pulse > BREAK_PULSE_LENGTH
            && pulse_length < BREAK_PULSE_LENGTH
        {
            debug!(
                "Found initial break pulse {} μs and first pulse {} μs.",
                last_pulse.as_micros(),
                pulse_length.as_micros()
            );
            pulses.push(
                last_pulse
                    .as_micros()
                    .try_into()
                    .context("Pulse length too long")?,
            );
            pulses.push(
                pulse_length
                    .as_micros()
                    .try_into()
                    .context("Pulse length too long")?,
            );
            break;
        } else {
            trace!(
                "Ignoring {} μs {:?} pulse.",
                pulse_length.as_micros(),
                !level
            );
        }

        last_pulse = pulse_length;
    }

    debug!("Reading pulses...");
    while let Some(level) = rx_pin.poll_interrupt(false, Some(MAX_PULSE_LENGTH))? {
        let timestamp = Instant::now();
        let pulse_length = timestamp - last_timestamp;
        pulses.push(
            pulse_length
                .as_micros()
                .try_into()
                .context("Pulse length too long")?,
        );
        if pulse_length > BREAK_PULSE_LENGTH {
            debug!(
                "Found final {:?} break pulse {} μs.",
                !level,
                pulse_length.as_micros()
            );
            break;
        }
        last_timestamp = timestamp;
    }

    Ok(pulses)
}
