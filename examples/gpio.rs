// Copyright 2023 the rfbutton authors.
// This project is dual-licensed under Apache 2.0 and MIT terms.
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::time::{Duration, Instant};

use cc1101::{
    lowlevel::types::AutoCalibration, Cc1101, FilterLength, Modulation, RadioMode, SyncMode,
    TargetAmplitude,
};
use eyre::{bail, Context, Report};
use linux_embedded_hal::{
    spidev::{SpiModeFlags, SpidevOptions},
    sysfs_gpio::{Direction, Edge},
    Spidev, SysfsPin,
};
use log::{debug, trace};
use rfbutton::decode;

/// The GPIO pin to which the 433 MHz receiver's data pin is connected.
const RX_PIN: u64 = 27;
const CS_PIN: u64 = 25;

const MAX_PULSE_LENGTH: Duration = Duration::from_millis(10);
const BREAK_PULSE_LENGTH: Duration = Duration::from_millis(7);

fn main() -> Result<(), Report> {
    color_eyre::install()?;
    pretty_env_logger::init();
    color_backtrace::install();

    let rx_pin = SysfsPin::new(RX_PIN);

    let cs = SysfsPin::new(CS_PIN);
    cs.export()?;
    cs.set_direction(Direction::High)?;
    let mut spi = Spidev::open("/dev/spidev0.0")?;
    spi.configure(
        &SpidevOptions::new()
            .bits_per_word(8)
            .max_speed_hz(1_000_000)
            .mode(SpiModeFlags::SPI_MODE_0)
            .build(),
    )?;
    let mut cc1101 = Cc1101::new(spi, cs).wrap_err("Error creating CC1101 device")?;
    cc1101.reset()?;
    let (partnum, version) = cc1101
        .get_hw_info()
        .wrap_err("Error getting hardware info")?;
    println!("Part number {}, version {}", partnum, version);
    cc1101
        .set_frequency(433940000)
        .wrap_err("Error setting frequency")?;
    cc1101.set_raw_mode()?;

    // Frequency synthesizer IF 211 kHz. Doesn't seem to affect big button, but affects sensitivity to small remote.
    cc1101.set_synthesizer_if(152_300)?;
    // DC blocking filter enabled, OOK modulation, manchester encoding disabled, no preamble/sync.
    cc1101.set_sync_mode(SyncMode::Disabled)?;
    cc1101.set_modulation(Modulation::OnOffKeying)?;
    // Channel bandwidth and data rate.
    cc1101.set_chanbw(232_000)?;
    cc1101.set_data_rate(3_000)?;
    // Automatically calibrate when going from IDLE to RX or TX.
    // XOSC stable timeout was being set to 64, but this doesn't seem important.
    cc1101.set_autocalibration(AutoCalibration::FromIdle)?;
    // Medium hysteresis, 16 channel filter samples, normal operation, OOK decision boundary 12 dB. Seems to affect sensitivity to small remote.
    cc1101.set_agc_filter_length(FilterLength::Samples32)?;
    // All gain settings can be used, maximum possible LNA gain, 36 dB target value.
    // TODO: 36 dB or 42 dB? 36 dB seems to let some noise through. Default value lets noise through all the time.
    cc1101.set_agc_target(TargetAmplitude::Db42)?;
    // Front-end RX current configuration. Unclear whether this affects sensitivity.
    //cc1101.0.write_register(Config::FREND1, 0xb6)?;
    cc1101.set_radio_mode(RadioMode::Receive)?;

    println!("Set up CC1101, enabling interrupts...");

    rx_pin.set_edge(Edge::BothEdges)?;

    loop {
        match receive(&rx_pin) {
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
fn receive(rx_pin: &SysfsPin) -> Result<Vec<u16>, Report> {
    let mut poller = rx_pin.get_poller()?;
    debug!("Waiting for interrupt...");
    let level = poller.poll(isize::MAX)?;
    if level.is_none() {
        bail!("Unexpected initial level {:?}", level);
    }
    debug!("Initial level: {:?}", level);
    let mut last_timestamp = Instant::now();
    let mut pulses = Vec::new();

    debug!("Waiting for initial break pulse...");
    // Wait for a long pulse to start.
    let mut last_pulse = Duration::default();
    while let Some(level) = poller.poll(isize::MAX)? {
        let timestamp = Instant::now();
        let pulse_length = timestamp - last_timestamp;
        last_timestamp = timestamp;

        if level != 0 && pulse_length > BREAK_PULSE_LENGTH {
            trace!(
                "Found possible initial break pulse {} μs.",
                pulse_length.as_micros()
            );
        } else if level == 0 && last_pulse > BREAK_PULSE_LENGTH && pulse_length < BREAK_PULSE_LENGTH
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
    let max_pulse_length_ms = MAX_PULSE_LENGTH.as_millis().try_into().unwrap();
    while let Some(level) = poller.poll(max_pulse_length_ms)? {
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
