use crate::Result;
use time::OffsetDateTime;

#[derive(Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct HeartRate {
    pub range: u8,
    pub rates: Vec<u8>,
    pub date: OffsetDateTime,
}

#[derive(Debug)]
pub enum HeartRateState {
    Length {
        size: u8,
        range: u8,
    },
    Recieving {
        date: OffsetDateTime,
        size: u8,
        range: u8,
        rates: Vec<u8>,
    },
    Complete {
        range: u8,
        rates: Vec<u8>,
        date: OffsetDateTime,
    },
}

impl TryFrom<&[u8]> for HeartRateState {
    type Error = Box<dyn std::error::Error>;

    fn try_from(value: &[u8]) -> std::result::Result<Self, Self::Error> {
        if value[1] == 255 {
            return Ok(Self::Complete {
                rates: Vec::new(),
                date: OffsetDateTime::from_unix_timestamp(0)?,
                range: 0,
            });
        }
        if value.len() < 15 {
            return Err(
                format!("Packet too short for heart rate data 15 < {}", value.len()).into(),
            );
        }
        if value[1] != 0 {
            return Err(format!(
                "unexpected initial heart rate message expected 0 found {}",
                value[1]
            )
            .into());
        }
        Ok(Self::Length {
            size: value[2].saturating_sub(1),
            range: value[3],
        })
    }
}

impl HeartRateState {
    pub fn step(&mut self, packet: &[u8]) -> Result {
        *self = match self {
            HeartRateState::Length { size, range } => Self::step_length(*size, *range, packet)?,
            HeartRateState::Recieving {
                date,
                size,
                range,
                rates,
            } => {
                let rates = core::mem::take(rates);
                Self::step_receiving(*size, *range, *date, rates, packet)?
            }
            HeartRateState::Complete { .. } => {
                return Err("Unexpected packet after complete!".to_string().into())
            }
        };
        Ok(())
    }

    fn step_length(size: u8, range: u8, packet: &[u8]) -> Result<Self> {
        if packet[1] != 1 {
            return Err(format!(
                "heart rate packet stream missing datetime packet found sub_type {}",
                packet[1]
            )
            .into());
        }
        let mut timestamp_bytes = [0u8; 4];
        timestamp_bytes.copy_from_slice(&packet[2..6]);
        let timestamp_int = u32::from_le_bytes(timestamp_bytes);
        let timestamp = OffsetDateTime::from_unix_timestamp(timestamp_int as _)?;
        let mut rates = Vec::with_capacity(size as usize * 13);
        for &byte in &packet[6..15] {
            rates.push(byte);
        }
        Ok(Self::Recieving {
            range,
            date: timestamp,
            rates,
            size,
        })
    }

    fn step_receiving(
        size: u8,
        range: u8,
        date: OffsetDateTime,
        mut rates: Vec<u8>,
        packet: &[u8],
    ) -> Result<Self> {
        if packet[1] == 0 {
            return Err("Unexpected size packet after date packet"
                .to_string()
                .into());
        }
        if packet[1] == 1 {
            return Err("Unexpected date packet after date packet"
                .to_string()
                .into());
        }
        for &byte in &packet[2..15] {
            rates.push(byte);
        }
        Ok(if packet[1] == size {
            Self::Complete { range, rates, date }
        } else {
            Self::Recieving {
                date,
                size,
                range,
                rates,
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use time::{Date, Time};

    use super::*;

    #[test]
    fn parse_multi_packet() {
        let mut packets = VecDeque::from_iter(
            [
                *b"\x15\x00\x18\x05\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x002",
                *b"\x15\x01\x80\xad\xb6f\x00\x00\x00\x00\x00\x00\x00\x00\x00_",
                *b"\x15\x02\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x17",
                *b"\x15\x03\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x18",
                *b"\x15\x04\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x19",
                *b"\x15\x05\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x1a",
                *b"\x15\x06\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x1b",
                *b"\x15\x07\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x1c",
                *b"\x15\x08\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x1d",
                *b"\x15\t\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x1e",
                *b"\x15\n\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x1f",
                *b"\x15\x0b\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00 ",
                *b"\x15\x0c\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00!",
                *b"\x15\r\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\"",
                *b"\x15\x0e\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00#",
                *b"\x15\x0f\x00\x00Y\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00}",
                *b"\x15\x10\x00k\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x90",
                *b"\x15\x11`\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00k\xf1",
                *b"\x15\x12\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00'",
                *b"\x15\x13\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00P\x00\x00x",
                *b"\x15\x14\x00\x00\x00\x00\x00\x00\x00\x00\x00F\x00\x00\x00o",
                *b"\x15\x15\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00*",
                *b"\x15\x16\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00+",
                *b"\x15\x17\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00,",
            ]
            .into_iter(),
        );
        let mut state =
            HeartRateState::try_from(packets.pop_front().unwrap().as_slice()).unwrap();
        for packet in packets {
            state.step(&packet[..packet.len() - 1]).unwrap();
        }
        let HeartRateState::Complete { range, rates, date } = state else {
            panic!("invalid state: {state:?}");
        };
        assert_eq!(range, 5);
        assert_eq!(
            date,
            OffsetDateTime::new_utc(
                Date::from_calendar_date(2024, time::Month::August, 10).unwrap(),
                Time::from_hms(0, 0, 0).unwrap()
            )
        );
        insta::assert_debug_snapshot!(rates);
    }
}
