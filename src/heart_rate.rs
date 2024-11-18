use time::OffsetDateTime;
use crate::Result;

pub struct HeartRate {
    pub range: u8,
    pub rates: Vec<u8>,
    pub date: OffsetDateTime,
}

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
    }
}

impl TryFrom<&[u8]> for HeartRateState {
    type Error = Box<dyn std::error::Error>;

    fn try_from(value: &[u8]) -> std::result::Result<Self, Self::Error> {
        if value[0] == 255 {
            return Ok(Self::Complete { rates: Vec::new(), date: OffsetDateTime::from_unix_timestamp(0)?, range: 0 })
        }
        if value.len() < 15 {
            return Err(format!("Packet too short for heart rate data 15 < {}", value.len()).into());
        }
        if value[0] != 0 {
            return Err(format!("unexpected initial heart rate message expected 0 found {}", value[0]).into());
        }
        Ok(Self::Length { size: value[1], range: value[2]})

    }
}

impl HeartRateState {
    pub fn step(&mut self, packet: [u8;16]) -> Result {
        *self = match self {
            HeartRateState::Length { size, range } => {
                Self::step_length(*size, *range, packet)?
            },
            HeartRateState::Recieving { date, size, range, rates } => {
                let rates = core::mem::take(rates);
                Self::step_receiving(*size, *range, *date, rates, packet)?
            },
            HeartRateState::Complete { .. } => {
                return Err(format!("Unexpected packet after complete!").into())
            },
            
        };
        Ok(())
    }

    fn step_length(size: u8, range: u8, packet: [u8;16]) -> Result<Self> {
        if packet[1] != 1 {
            return Err(format!("heart rate packet stream missing datetime packet found sub_type {}", packet[1]).into());
        }
        let mut timestamp_bytes = [0u8;4];
        timestamp_bytes.copy_from_slice(&packet[2..6]);
        let timestamp_int = u32::from_le_bytes(timestamp_bytes);
        let timestamp = OffsetDateTime::from_unix_timestamp(timestamp_int as _)?;
        let mut rates = Vec::with_capacity(size as usize * 13);
        for &byte in &packet[6..] {
            rates.push(byte);
        }
        Ok(Self::Recieving { range: range, date: timestamp, rates, size, })
    }

    fn step_receiving(size: u8, range: u8, date: OffsetDateTime, mut rates: Vec<u8>, packet: [u8;16]) -> Result<Self> {
        if packet[1] == 0 {
            return Err(format!("Unexpected size packet after date packet").into())
        }
        if packet[1] == 1 {
            return Err(format!("Unexpected date packet after date packet").into());
        }
        for &byte in &packet[2..15] {
            rates.push(byte);
        }
        Ok(if packet[1] == size {
            Self::Complete { range, rates, date }
        } else {
            Self::Recieving { date, size, range, rates }
        })
    }
}
