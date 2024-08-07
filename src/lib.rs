// Copyright 2021 the rfbutton authors.
// This project is dual-licensed under Apache 2.0 and MIT terms.
// See LICENSE-APACHE and LICENSE-MIT for details.

//! A library for decoding 433 MHz RF remote codes.

use std::{
    fmt::{self, Debug, Formatter},
    ops::{Add, Div},
};
use thiserror::Error;

const BREAK_PULSE_LENGTH: u16 = 3000;

/// An error decoding an RF button code.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum Error {
    /// The start pulse of the code sequence couldn't be found.
    #[error("Couldn't find start pulse")]
    NoStart,
    /// There were not enough pulses to decode the code.
    #[error("Too few pulses")]
    TooShort,
    /// A pair of pulses in the code were of an unexpected length.
    #[error("Invalid pulse length ({0} μs high {1} μs low)")]
    InvalidPulseLength(u16, u16),
}

/// A decoded RF button code.
#[derive(Copy, Clone, Eq, Hash, PartialEq)]
pub struct Code {
    /// The decoded value.
    pub value: u32,
    /// The length in bits.
    pub length: u8,
}

impl Debug for Code {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(
            f,
            "{:#x} {:#026b} ({} bits)",
            self.value, self.value, self.length
        )
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for Code {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        if s.len() > 8 {
            return Err(serde::de::Error::invalid_value(
                serde::de::Unexpected::Str(&s),
                &"no more than 8 characters",
            ));
        }
        let value =
            u32::from_str_radix(&s, 16).map_err(|e| serde::de::Error::custom(e.to_string()))?;
        Ok(Self {
            value,
            length: s.len() as u8 * 4,
        })
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for Code {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        if self.length % 4 != 0 {
            return Err(serde::ser::Error::custom(
                "Only codes with length a multiple of 4 can be serialized.",
            ));
        }
        let s = format!("{:01$x}", self.value, usize::from(self.length) / 4);
        serializer.serialize_str(&s)
    }
}

/// Given a sequence of pulse durations in microseconds (starting with a high pulse), try to decode
/// a button code.
pub fn decode(pulses: &[u16]) -> Result<Code, Error> {
    // Look for a long low pulse to find the start.
    let start = pulses
        .iter()
        .position(|pulse| *pulse > BREAK_PULSE_LENGTH)
        .ok_or(Error::NoStart)?
        + 1;
    let pulses = &pulses[start..];

    if pulses.len() < 4 {
        return Err(Error::TooShort);
    }

    // Use the first 4 pulses to calculate the short pulse duration.
    let short_duration = pulses[0..4].iter().sum::<u16>() / 8;

    let mut value = 0;
    let mut length = 0;
    let mut pulses = pulses.iter();
    while let (Some(&high), Some(&low)) = (pulses.next(), pulses.next()) {
        let high_period = round_div(high, short_duration);
        let low_period = round_div(low, short_duration);
        if high_period == 3 && low_period == 1 {
            value = value << 1 | 1;
            length += 1;
        } else if high_period == 1 && low_period == 3 {
            value <<= 1;
            length += 1;
        } else if high > BREAK_PULSE_LENGTH || low > BREAK_PULSE_LENGTH {
            break;
        } else {
            return Err(Error::InvalidPulseLength(high, low));
        }
    }

    Ok(Code { value, length })
}

/// Divide one integer by another, rounding towards the closest integer.
fn round_div<T: Add<Output = T> + Div<Output = T> + From<u8> + Copy>(dividend: T, divisor: T) -> T {
    (dividend + divisor / 2.into()) / divisor
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_no_start() {
        assert_eq!(decode(&[]), Err(Error::NoStart));
    }

    #[test]
    fn decode_short() {
        assert_eq!(
            decode(&[300, 10000, 1000, 333, 1000, 333, 333, 1000, 1000, 333]),
            Ok(Code {
                value: 0b1101,
                length: 4
            })
        );
    }

    #[test]
    fn decode_short_repeated() {
        assert_eq!(
            decode(&[
                300, 10000, 1000, 333, 1000, 333, 333, 1000, 1000, 333, 333, 10000, 1000, 333
            ]),
            Ok(Code {
                value: 0b1101,
                length: 4
            })
        );
    }

    #[test]
    fn decode_full() {
        let decoded = decode(&[
            320, 10060, 320, 960, 960, 300, 300, 960, 320, 960, 960, 300, 300, 960, 300, 980, 300,
            960, 960, 300, 320, 960, 960, 300, 960, 320, 300, 960, 300, 960, 960, 320, 300, 960,
            960, 320, 300, 960, 960, 320, 300, 960, 300, 960, 980, 300, 300, 960, 320, 960, 300,
            10080, 320, 960, 960, 320, 300, 960, 300, 960, 980, 300, 300, 960, 320, 960, 300, 960,
            960, 320, 300, 960, 960, 320, 960, 300, 300, 960, 320, 960, 960, 300, 320, 960, 960,
            300, 320, 960, 960, 300, 320, 960, 300, 960, 960, 320, 300, 960, 320, 960, 300, 10080,
            320, 960, 960, 320, 300, 960, 300, 960, 960, 320, 300, 960, 320, 960, 300, 960, 960,
            320, 300, 960, 960, 320, 960, 300, 320, 960, 300, 960, 960, 320, 300, 960, 960, 320,
            300, 960, 960, 320, 300, 960, 300, 960, 980, 300, 300, 960, 320, 960, 300, 10100, 300,
            980, 960, 300, 300, 960, 320, 960, 960, 300, 320, 960, 300, 960, 300, 980, 960, 300,
            320, 960, 960, 300, 960, 320, 300, 960, 320, 960, 960, 300, 320, 960, 960, 300, 320,
            960, 960, 300, 320, 960, 300, 960, 960, 320, 300, 960, 300, 960, 320, 10100, 300, 960,
            960, 320, 300, 960, 320, 940, 980, 300, 300, 980, 300, 960, 300, 960, 980, 300, 300,
            960, 960, 320, 960, 320, 300, 960, 300, 960, 980, 300, 300, 960, 960, 320, 300, 960,
            980, 300, 300, 960, 320, 960, 960, 300, 320, 960, 300, 960, 320, 10080, 320, 960, 960,
            300, 320, 960, 300, 960, 960, 320, 300, 960, 320, 960, 300, 960, 960, 320, 300, 960,
            960, 320, 960, 300, 320, 960, 300, 960, 960, 320, 300, 960, 960, 320, 300, 960, 960,
            320, 300, 960, 320, 960, 960, 300, 320, 960, 300,
        ]);
        assert_eq!(
            decoded,
            Ok(Code {
                value: 0x48b2a4,
                length: 24
            })
        );
    }

    #[cfg(feature = "serde")]
    #[test]
    fn serde_code() {
        use serde_test::{assert_tokens, Token};

        assert_tokens(
            &Code {
                value: 0,
                length: 12,
            },
            &[Token::Str("000")],
        );
        assert_tokens(
            &Code {
                value: 0xf,
                length: 4,
            },
            &[Token::Str("f")],
        );
        assert_tokens(
            &Code {
                value: 0x123456,
                length: 24,
            },
            &[Token::Str("123456")],
        );
        assert_tokens(
            &Code {
                value: 0xabcdef,
                length: 24,
            },
            &[Token::Str("abcdef")],
        );
        assert_tokens(
            &Code {
                value: 0xff112233,
                length: 32,
            },
            &[Token::Str("ff112233")],
        );
    }
}
