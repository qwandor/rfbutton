// Copyright 2024 the rfbutton authors.
// This project is dual-licensed under Apache 2.0 and MIT terms.
// See LICENSE-APACHE and LICENSE-MIT for details.

//! An example using an RFM69 module (such as the Adafruit Radio Bonnet) connected to a Raspberry
//! Pi.

use eyre::{bail, Context, Report};
use log::{debug, trace};
use rfbutton::decode;
use rfm69::{
    registers::{
        DataMode, DccCutoff, DioMapping, DioMode, DioPin, DioType, LnaConfig, LnaGain,
        LnaImpedance, Mode, Modulation, ModulationShaping, ModulationType, RxBw, RxBwOok,
    },
    Rfm69,
};
use rppal::{
    gpio::{Gpio, InputPin, Level, Trigger},
    spi::{self, Bus, SimpleHalSpiDevice, SlaveSelect, Spi},
};
use std::{
    thread::sleep,
    time::{Duration, Instant},
};

/// The GPIO pin to which the RFM69's data pin is connected.
const RX_PIN: u8 = 27;
/// The GPIO pin to which the RFM69's reset pin is connected.
const RESET_PIN: u8 = 25;

const MAX_PULSE_LENGTH: Duration = Duration::from_millis(10);
const BREAK_PULSE_LENGTH: Duration = Duration::from_millis(7);

fn main() -> Result<(), Report> {
    color_eyre::install()?;
    pretty_env_logger::init();
    color_backtrace::install();

    let gpio = Gpio::new()?;
    let mut rx_pin = gpio.get(RX_PIN)?.into_input();
    let mut reset_pin = gpio.get(RESET_PIN)?.into_output();

    // Reset the radio.
    reset_pin.set_high();
    sleep(Duration::from_millis(1));
    reset_pin.set_low();
    sleep(Duration::from_millis(5));

    let spi = SimpleHalSpiDevice::new(Spi::new(
        Bus::Spi0,
        SlaveSelect::Ss1,
        1_000_000,
        spi::Mode::Mode0,
    )?);
    let mut rfm = Rfm69::new(spi);
    rfm.frequency(433_850_000).unwrap();
    rfm.rssi_threshold(175).unwrap();
    rfm.lna(LnaConfig {
        zin: LnaImpedance::Ohm200,
        gain_select: LnaGain::AgcLoop,
    })
    .unwrap();
    rfm.rx_bw(RxBw {
        dcc_cutoff: DccCutoff::Percent4,
        rx_bw: RxBwOok::Khz200dot0,
    })
    .unwrap();
    rfm.modulation(Modulation {
        data_mode: DataMode::Packet,
        modulation_type: ModulationType::Ook,
        shaping: ModulationShaping::Shaping00,
    })
    .unwrap();
    rfm.dio_mapping(DioMapping {
        pin: DioPin::Dio2,
        dio_type: DioType::Dio01, // Data
        dio_mode: DioMode::Rx,
    })
    .unwrap();
    rfm.dio_mapping(DioMapping {
        pin: DioPin::Dio3,
        dio_type: DioType::Dio01, // RSSI
        dio_mode: DioMode::Rx,
    })
    .unwrap();

    rfm.mode(Mode::Receiver).unwrap();
    while !rfm.is_mode_ready().unwrap() {
        sleep(Duration::from_millis(1));
    }

    println!("Set up RFM69, enabling interrupts...");

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
