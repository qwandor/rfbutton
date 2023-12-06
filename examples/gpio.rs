// Copyright 2023 the rfbutton authors.
// This project is dual-licensed under Apache 2.0 and MIT terms.
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::time::{Duration, Instant};

use cc1101::{lowlevel::registers::Config, Cc1101, Modulation, RadioMode, SyncMode};
use eyre::{bail, eyre, Context, Report};
use linux_embedded_hal::{
    spidev::{SpiModeFlags, SpidevOptions},
    sysfs_gpio::Direction,
    Spidev, SysfsPin,
};
use log::{debug, trace};
use rfbutton::decode;
use rppal::gpio::{Gpio, InputPin, Level, Trigger};

/// The GPIO pin to which the 433 MHz receiver's data pin is connected.
const RX_PIN: u8 = 27;
const CS_PIN: u64 = 25;

const MAX_PULSE_LENGTH: Duration = Duration::from_millis(10);
const BREAK_PULSE_LENGTH: Duration = Duration::from_millis(7);

fn main() -> Result<(), Report> {
    color_eyre::install()?;
    pretty_env_logger::init();
    color_backtrace::install();

    let gpio = Gpio::new()?;
    let mut rx_pin = gpio.get(RX_PIN)?.into_input();

    //let cs = gpio.get(CS_PIN)?.into_output();
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
    let mut cc1101 =
        Cc1101::new(spi, cs).map_err(|e| eyre!("Error creating CC1101 device: {:?}", e))?;
    cc1101.reset().unwrap();
    let (partnum, version) = cc1101
        .get_hw_info()
        .map_err(|e| eyre!("Error getting hardware info: {:?}", e))?;
    println!("Part number {}, version {}", partnum, version);
    cc1101
        .set_frequency(433940000)
        .map_err(|e| eyre!("Error setting frequency: {:?}", e))?;

    // Serial data output.
    cc1101.0.write_register(Config::IOCFG0, 0x0d).unwrap();
    // Disable data whitening and CRC, fixed packet length, asynchronous serial mode.
    cc1101.0.write_register(Config::PKTCTRL0, 0x30).unwrap();
    //cc1101.0.write_register(Config::PKTLEN, 0x04).unwrap();
    // Frequency synthesizer offset (0x00 reset value).
    //cc1101.0.write_register(Config::FSCTRL0, 0x00).unwrap();
    // Frequency synthesizer IF 211 kHz. Doesn't seem to affect big button, but affects sensitivity to small remote.
    cc1101.0.write_register(Config::FSCTRL1, 0x06).unwrap();
    // Channel spacing. (Seems irrelevant, default value.)
    //cc1101.0.write_register(Config::MDMCFG0, 0xf8).unwrap();
    // FEC disabled, 4 preamble bytes, 2 bit exponent of channel spacing. (Seems irrelevant, default value.)
    //cc1101.0.write_register(Config::MDMCFG1, 0x22).unwrap();
    // DC blocking filter enabled, OOK modulation, manchester encoding disabled, no preamble/sync.
    cc1101.set_sync_mode(SyncMode::Disabled).unwrap();
    cc1101.set_modulation(Modulation::OnOffKeying).unwrap();
    // Channel bandwidth and data rate.
    cc1101.set_chanbw(232_000).unwrap();
    cc1101.set_data_rate(3_000).unwrap();
    // Automatically calibrate when going from IDLE to RX or TX, XOSC stable timeout 64.
    cc1101.0.write_register(Config::MCSM0, 0x18).unwrap();
    // Clear channel indication always, RX off mode idle, TX off mode idle.
    //cc1101.0.write_register(Config::MCSM1, 0x00).unwrap();
    // RX timeout. (Seems irrelevant, default value.)
    //cc1101.0.write_register(Config::MCSM2, 0x07).unwrap();
    // Medium hysteresis, 18 channel filter samples, normal operation, OOK decision boundary 12 dB. Seems to affect sensitivity to small remote.
    cc1101.0.write_register(Config::AGCCTRL0, 0x92).unwrap();
    // LNA2 gain decreased first, relative carrier sense threshold disabled, absolute RSSI threshold at target setting.
    //cc1101.0.write_register(Config::AGCCTRL1, 0x00).unwrap();
    // All gain settings can be used, maximum possible LNA gain, 36 dB target value.
    // TODO: 0x04 or 0x07? 0x04 seems to let some noise through. Default value lets noise through all the time.
    cc1101.0.write_register(Config::AGCCTRL2, 0x07).unwrap();
    // Front-end TX current configuration.
    //cc1101.0.write_register(Config::FREND0, 0x11).unwrap();
    // Front-end RX current configuration. Unclear whether this affects sensitivity.
    //cc1101.0.write_register(Config::FREND1, 0xb6).unwrap();
    // Frequency synthesiser calibration.
    /*cc1101.0.write_register(Config::FSCAL0, 0x1f).unwrap();
    cc1101.0.write_register(Config::FSCAL1, 0x00).unwrap();
    cc1101.0.write_register(Config::FSCAL2, 0x2a).unwrap();
    cc1101.0.write_register(Config::FSCAL3, 0xe9).unwrap();*/
    // Test settings.
    /*cc1101.0.write_register(Config::TEST0, 0x09).unwrap();
    cc1101.0.write_register(Config::TEST1, 0x35).unwrap();
    cc1101.0.write_register(Config::TEST2, 0x81).unwrap();*/
    cc1101.set_radio_mode(RadioMode::Receive).unwrap();

    println!("Set up CC1101, enabling interrupts...");

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
