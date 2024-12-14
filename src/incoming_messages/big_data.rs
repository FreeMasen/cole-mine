use std::{fmt::Display, time::Duration};

use time::{OffsetDateTime, PrimitiveDateTime};

use crate::{
    constants,
    util::{try_u16_from_iter, try_u16_from_le_slice, DurationExt as _},
    Result,
};

#[derive(Debug)]
pub enum BigDataState {
    Partial {
        target_length: usize,
        packet: BigDataPacket,
    },
    Complete(BigDataPacket),
}

#[derive(Debug, Clone)]
pub enum BigDataPacket {
    Sleep(Vec<u8>),
    Oxygen(Vec<u8>),
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct SleepData {
    pub sessions: Vec<SleepSession>,
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct SleepSession {
    pub start: PrimitiveDateTime,
    pub end: PrimitiveDateTime,
    pub stages: Vec<SleepStage>,
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
pub enum SleepStage {
    Light(u8),
    Deep(u8),
    Rem(u8),
    Awake(u8),
}

impl TryFrom<BigDataPacket> for SleepData {
    type Error = Box<dyn std::error::Error>;
    fn try_from(value: BigDataPacket) -> std::result::Result<Self, Self::Error> {
        let BigDataPacket::Sleep(data) = value else {
            return Err(format!("Invlaid big data packet for sleep: {value:?}").into());
        };
        let days = data.first().copied().unwrap_or_default();
        log::debug!("trying to parse sleep data with {days} days");
        log::trace!("{:?}", data);
        let mut sessions = Vec::with_capacity(days as _);
        fn too_short_error(idx: u8, msg: impl Display) -> impl Fn() -> Box<dyn std::error::Error> {
            move || -> Box<dyn std::error::Error + 'static> {
                format!("Packet too short at {idx}: {msg}").into()
            }
        }

        let mut iter = data[1..].iter().copied();
        let now = OffsetDateTime::now_local().unwrap_or_else(|_| OffsetDateTime::now_utc());
        let today = now.date();
        for i in 1..days {
            let days_ago = iter.next().ok_or_else(too_short_error(i, "days ago"))?;
            log::trace!("handling day {days_ago} days in the past");
            let day = today - Duration::days(days_ago as u64 - 1);
            log::trace!("{day:?}");
            let day_bytes = iter.next().ok_or_else(too_short_error(i, "day bytes"))?;
            log::trace!("day bytes: {day_bytes}");
            let start = try_u16_from_iter(&mut iter).ok_or_else(too_short_error(i, "start"))?;
            let end = try_u16_from_iter(&mut iter).ok_or_else(too_short_error(i, "end"))?;
            let start = if start > end {
                println!("{} {}", start, (start as i32) - 1440);
                day.midnight() - Duration::minutes(1440 - start as u64)
            } else {
                day.previous_day().ok_or("Invalid day")?.midnight() + Duration::minutes(start as _)
            };
            let end = day.midnight() + Duration::minutes(end as _);
            log::debug!("sleep session {start:?}-{end:?}",);
            let mut stages = Vec::new();
            let mut remaining_bytes = day_bytes - 4;
            while remaining_bytes > 0 {
                let stage = iter
                    .next()
                    .ok_or_else(too_short_error(i, &format!("{remaining_bytes} stage")))?;
                let minutes = iter
                    .next()
                    .ok_or_else(too_short_error(i, &format!("{remaining_bytes} minutes")))?;
                log::debug!("{stage}-{minutes}");
                remaining_bytes -= 2;
                stages.push(match stage {
                    0 => {
                        log::warn!("empty sleep stage");
                        continue;
                    }
                    constants::SLEEP_TYPE_LIGHT => SleepStage::Light(minutes),
                    constants::SLEEP_TYPE_DEEP => SleepStage::Deep(minutes),
                    constants::SLEEP_TYPE_REM => SleepStage::Rem(minutes),
                    constants::SLEEP_TYPE_AWAKE => SleepStage::Awake(minutes),
                    _ => {
                        return Err(format!(
                            "{i}/{remaining_bytes} sleep sample type invalid {stage}"
                        )
                        .into())
                    }
                });
            }
            sessions.push(SleepSession { start, end, stages })
        }
        Ok(Self { sessions })
    }
}

impl BigDataState {
    pub fn new(bytes: &[u8]) -> Result<Self> {
        if bytes[0] != crate::constants::CMD_BIG_DATA_V2 {
            return Err(format!("Invalid bytes for bigdata state: {bytes:?}").into());
        }
        log::debug!("with bytes {}", bytes.len());
        let target_length = try_u16_from_le_slice(&bytes[2..4]).unwrap() as usize;
        let data = Vec::with_capacity(target_length);
        let tag = bytes[1];
        let mut ret = Self::Partial {
            target_length,
            packet: if tag == constants::BIG_DATA_TYPE_SLEEP {
                BigDataPacket::Sleep(data)
            } else if bytes[1] == constants::BIG_DATA_TYPE_SPO2 {
                BigDataPacket::Oxygen(data)
            } else {
                return Err(format!("Unknown big data type: {bytes:?}").into());
            },
        };
        ret.step(&bytes[6..])?;
        Ok(ret)
    }

    pub fn step(&mut self, bytes: &[u8]) -> Result {
        let Self::Partial {
            target_length,
            packet,
        } = self
        else {
            return Err("step after complete".into());
        };
        packet.extend_from_slice(bytes);
        if packet.len() == *target_length {
            *self = Self::Complete(packet.clone());
        }
        Ok(())
    }
}

impl BigDataPacket {
    pub fn extend_from_slice(&mut self, slice: &[u8]) {
        self.get_data_mut().extend_from_slice(slice);
    }

    pub fn len(&self) -> usize {
        self.get_data_ref().len()
    }

    #[allow(unused)]
    pub fn is_empty(&self) -> bool {
        self.get_data_ref().is_empty()
    }

    #[allow(unused)]
    pub fn capacity(&self) -> usize {
        self.get_data_ref().capacity()
    }

    pub fn get_data_ref(&self) -> &Vec<u8> {
        match self {
            Self::Oxygen(data) | Self::Sleep(data) => data,
        }
    }

    pub fn get_data_mut(&mut self) -> &mut Vec<u8> {
        match self {
            Self::Oxygen(data) | Self::Sleep(data) => data,
        }
    }
}

#[derive(Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct OxygenData {
    pub samples: Vec<OxygenMeasurement>,
}

#[derive(Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct OxygenMeasurement {
    pub min: u8,
    pub max: u8,
    pub when: PrimitiveDateTime,
}

impl TryFrom<BigDataPacket> for OxygenData {
    type Error = String;
    fn try_from(value: BigDataPacket) -> std::result::Result<Self, Self::Error> {
        let BigDataPacket::Oxygen(data) = value else {
            return Err(format!(
                "Error, attempt to parse oxygen data with wron packet: {value:?}"
            ));
        };
        let mut iter = data.iter().copied().peekable();

        let day_in_packet = iter.next().ok_or_else(|| format!("Packet sized 7"))?;
        let mut samples = Vec::new();
        let now = OffsetDateTime::now_local().unwrap_or_else(|_| OffsetDateTime::now_utc());
        let today = now.date().midnight();
        for i in 0..day_in_packet {
            let days_ago = iter
                .next()
                .ok_or_else(|| format!("Error, days ago for day {i} was none"))?;
            let day = today - Duration::days(days_ago as u64);
            for j in 0..24 {
                let hour = day + Duration::hours(j);
                let min = iter.next().ok_or_else(|| {
                    format!("Error processing hour {j} in day {i} expected minimum found none")
                })?;
                let max = iter.next().ok_or_else(|| {
                    format!("Error processing hour {j} in day {i} expected maximum found none")
                })?;
                samples.push(OxygenMeasurement {
                    max,
                    min,
                    when: hour,
                });
                if iter.peek().is_none() {
                    break;
                }
            }
        }
        Ok(Self { samples })
    }
}

#[cfg(test)]
mod tests {
    use time::OffsetDateTime;

    #[test]
    fn platform_can_get_local_time() {
        unsafe {
            time::util::local_offset::set_soundness(time::util::local_offset::Soundness::Unsound);
        }
        dbg!(OffsetDateTime::now_local()).unwrap();
    }
}
