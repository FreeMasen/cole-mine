use crate::Result;
use typed_builder::TypedBuilder;

#[derive(Default, TypedBuilder, PartialEq, Debug)]
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
        let bcd_to_decimal = |b: u8| (((b >> 4) & 15) * 10) + (b & 15);
        let year = bcd_to_decimal(value[0]) as u16 + 2000;
        let month = bcd_to_decimal(value[1]);
        let day = bcd_to_decimal(value[2]);
        let time_index = value[3];
        let calories = (value[6] as u16) | ((value[7] as u16) << 8);
        let steps = (value[9] as u16) << 8 | value[8] as u16;
        let distance = (value[11] as u16) << 8 | value[10] as u16;

        Ok(Self {
            year,
            month,
            day,
            time_index,
            calories,
            steps,
            distance,
        })
    }
}

impl SportDetail {
    pub fn apply_new_calories(&mut self) {
        self.calories *= 10;
    }
}

#[derive(PartialEq, Debug)]
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
                let done = packet[5] == packet[6] - 1;
                let mut packet = SportDetail::try_from(&packet[1..])?;
                if *new_cal_proto {
                    packet.apply_new_calories();
                }
                *self = if done {
                    Self::Complete {
                        packets: vec![packet],
                    }
                } else {
                    Self::Recieving {
                        new_cal_proto: *new_cal_proto,
                        packets: vec![packet],
                    }
                };
            }
            Self::Recieving {
                packets,
                new_cal_proto,
            } => {
                if packet[5] == packet[6] - 1 {
                    let mut packet = SportDetail::try_from(&packet[1..])?;
                    if *new_cal_proto {
                        packet.apply_new_calories();
                    }
                    let mut packets = core::mem::take(packets);
                    packets.push(packet);
                    *self = Self::Complete { packets };
                    return Ok(());
                }
                let mut packet = SportDetail::try_from(&packet[1..])?;
                if *new_cal_proto {
                    packet.apply_new_calories();
                }
                packets.push(packet);
            }
            Self::Complete { packets } => {
                return Err(format!("step after complete: {}", packets.len()).into());
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::*;

    #[test]
    fn test_parse_simple() {
        let mut state =
            SportDetailState::new(*b"C\xf0\x01\x01\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x005")
                .unwrap();
        assert_eq!(
            state,
            SportDetailState::Initial {
                new_cal_proto: true
            }
        );
        state
            .step(*b"C$\x10\x15\\\x00\x01y\x00\x15\x00\x10\x00\x00\x00\x87")
            .unwrap();
        assert_eq!(
            state,
            SportDetailState::Complete {
                packets: vec![SportDetail::builder()
                    .year(2024)
                    .month(10)
                    .day(15)
                    .time_index(92)
                    .calories(1210)
                    .steps(21)
                    .distance(16)
                    .build()]
            }
        );
    }

    #[test]
    fn test_parse_multi() {
        let mut packets = VecDeque::from_iter(
            [
                *b"C\xf0\x05\x01\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x009",
                *b"C#\x08\x13\x10\x00\x05\xc8\x000\x00\x1b\x00\x00\x00\xa9",
                *b"C#\x08\x13\x14\x01\x05\xb6\x18\xaa\x04i\x03\x00\x00\x83",
                *b"C#\x08\x13\x18\x02\x058\x04\xe1\x00\x95\x00\x00\x00R",
                *b"C#\x08\x13\x1c\x03\x05\x05\x02l\x00H\x00\x00\x00`",
                *b"C#\x08\x13L\x04\x05\xef\x01c\x00D\x00\x00\x00m",
            ]
            .into_iter(),
        );
        let expected = [
            SportDetail {
                year: 2023,
                month: 8,
                day: 13,
                time_index: 16,
                calories: 2000,
                steps: 48,
                distance: 27,
            },
            SportDetail {
                year: 2023,
                month: 8,
                day: 13,
                time_index: 20,
                calories: 63260,
                steps: 1194,
                distance: 873,
            },
            SportDetail {
                year: 2023,
                month: 8,
                day: 13,
                time_index: 24,
                calories: 10800,
                steps: 225,
                distance: 149,
            },
            SportDetail {
                year: 2023,
                month: 8,
                day: 13,
                time_index: 28,
                calories: 5170,
                steps: 108,
                distance: 72,
            },
            SportDetail {
                year: 2023,
                month: 8,
                day: 13,
                time_index: 76,
                calories: 4950,
                steps: 99,
                distance: 68,
            },
        ];

        let mut state = SportDetailState::new(packets.pop_front().unwrap()).unwrap();
        for packet in packets {
            state.step(packet).unwrap();
        }
        let SportDetailState::Complete { packets } = state else {
            panic!("Unexpected state: {state:?}");
        };
        assert_eq!(packets, expected);
    }

    #[test]
    fn test_no_data_parse() {
        let resp = *b"C\xff\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00B";
        let state = SportDetailState::new(resp).unwrap();
        let SportDetailState::Complete { packets } = state else {
            panic!("Expected complete found {state:?}");
        };

        assert_eq!(packets, Vec::new())
    }
}
