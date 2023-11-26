// Copyright 2023 the rfbutton authors.
// This project is dual-licensed under Apache 2.0 and MIT terms.
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::time::{Duration, Instant};

use eyre::{bail, Context, Report};
use log::trace;
use rfbutton::decode;
use rppal::gpio::{Gpio, InputPin, Level, Trigger};

/// The GPIO pin to which the 433 MHz receiver's data pin is connected.
const RX_PIN: u8 = 27;

const MAX_PULSE_LENGTH: Duration = Duration::from_millis(10);

fn main() -> Result<(), Report> {
    color_eyre::install()?;
    pretty_env_logger::init();
    color_backtrace::install();

    let gpio = Gpio::new()?;
    let mut rx_pin = gpio.get(RX_PIN)?.into_input();

    rx_pin.set_interrupt(Trigger::Both)?;

    let pulses = receive(&mut rx_pin)?;
    println!("Pulses: {:?}", pulses);
    let code = decode(&pulses)?;
    println!("Decoded: {:?}", code);

    Ok(())
}

/// Wait for a single code.
fn receive(rx_pin: &mut InputPin) -> Result<Vec<u16>, Report> {
    trace!("Waiting for interrupt...");
    let level = rx_pin.poll_interrupt(false, None)?;
    if level != Some(Level::High) {
        bail!("Unexpected initial level {:?}", level);
    }
    let mut pulses = Vec::new();
    let mut last_timestamp = Instant::now();
    while let Some(_) = rx_pin.poll_interrupt(false, Some(MAX_PULSE_LENGTH))? {
        let timestamp = Instant::now();
        pulses.push(
            (timestamp - last_timestamp)
                .as_micros()
                .try_into()
                .context("Pulse length too long")?,
        );
        last_timestamp = timestamp;
    }

    Ok(pulses)
}
