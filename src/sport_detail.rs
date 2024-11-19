use crate::Result;
use typed_builder::TypedBuilder;

#[derive(Default, TypedBuilder)]
#[builder(field_defaults(default))]
pub struct SportDetail {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub time_index: u8,
    pub calories: u16,
    pub steps: u16,
    pub distance: u16,
}

impl TryFrom<&[u8]> for SportDetail {
    type Error = String;
    fn try_from(value: &[u8]) -> std::result::Result<Self, Self::Error> {
        if value.len() < 12 {
            return Err(format!(
                "SportDetail must be at least 12 bytes found {}",
                value.len()
            ));
        }
        let mut byte_iter = value.iter().copied();
        let bcd_to_decimal = |b: u8| (((b >> 4) & 15) * 10) + (b & 15);
        Ok(Self::builder()
            .year(bcd_to_decimal(byte_iter.next().unwrap_or_default()) as u16 + 2000)
            .month(bcd_to_decimal(byte_iter.next().unwrap_or_default()))
            .day(bcd_to_decimal(byte_iter.next().unwrap_or_default()))
            .time_index(byte_iter.next().unwrap_or_default())
            .calories(
                byte_iter.next().unwrap_or_default() as u16
                    | (byte_iter.next().unwrap_or_default() as u16) << 8,
            )
            .steps(
                byte_iter.next().unwrap_or_default() as u16
                    | (byte_iter.next().unwrap_or_default() as u16) << 8,
            )
            .distance(
                byte_iter.next().unwrap_or_default() as u16
                    | (byte_iter.next().unwrap_or_default() as u16) << 8,
            )
            .build())
    }
}

impl SportDetail {
    pub fn apply_new_calories(&mut self) {
        self.calories *= 10;
    }
}

pub enum SportDetailState {
    Initial {
        new_cal_proto: bool,
    },
    Recieving {
        new_cal_proto: bool,
        packets: Vec<SportDetail>,
    },
    Complete {
        packets: Vec<SportDetail>,
    },
}

impl SportDetailState {
    pub fn new(packet: [u8; 16]) -> Result<Self> {
        if packet[0] != 67 {
            return Err(format!("Invalid prefix for sport detail state {}", packet[0]).into());
        }
        if packet[1] == 255 {
            return Ok(Self::Complete {
                packets: Vec::new(),
            });
        }
        if packet[1] == 240 {
            return Ok(Self::Initial {
                new_cal_proto: true,
            });
        }
        Ok(Self::Recieving {
            new_cal_proto: false,
            packets: vec![SportDetail::try_from(&packet[1..])?],
        })
    }

    pub fn step(&mut self, packet: [u8; 16]) -> Result {
        match self {
            Self::Initial { new_cal_proto } => {
                let packet = SportDetail::try_from(&packet[1..])?;
                *self = Self::Recieving {
                    new_cal_proto: *new_cal_proto,
                    packets: vec![packet],
                };
            }
            Self::Recieving {
                packets,
                new_cal_proto,
            } => {
                if packet[5] == packet[6] - 1 {
                    let packet = SportDetail::try_from(&packet[1..])?;
                    let mut packets = core::mem::take(packets);
                    packets.push(packet);
                    *self = Self::Complete { packets: packets };
                    return Ok(());
                }
                let packet = SportDetail::try_from(&packet[1..])?;
                packets.push(packet);
            }
            Self::Complete { packets } => {
                return Err(format!("step after complete: {}", packets.len()).into());
            }
        }

        Ok(())
    }
}
