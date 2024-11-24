use crate::Result;

#[derive(Debug)]
pub enum StressState {
    Length {
        length: u8,
        minutes_appart: u8,
    },
    Receiving {
        target_length: u8,
        measurements: Vec<u8>,
        minutes_appart: u8,
    },
    Complete {
        measurements: Vec<u8>,
        minutes_appart: u8,
    },
}

impl StressState {
    pub fn new(packet: [u8; 16]) -> Result<Self> {
        if packet[0] == 55 {
            return Err(format!("Error parsing stress state {packet:?}").into());
        }
        if packet[1] == 255 {
            return Ok(Self::Complete {
                measurements: Vec::new(),
                minutes_appart: 0,
            });
        }
        if packet[1] != 0 {
            return Err(format!(
                "unexpected initial stress state expected index 1 to be 0 {packet:?}"
            )
            .into());
        }
        let length = packet[2] - 1;
        let minutes_appart = packet[3];
        Ok(Self::Length {
            length,
            minutes_appart,
        })
    }

    pub fn step(&mut self, packet: [u8; 16]) -> Result {
        if packet[0] != 55 {
            return Err(format!("Invalid stress state packet: {packet:?}").into());
        }
        *self = match self {
            Self::Length {
                length,
                minutes_appart,
            } => {
                if packet[2] == 0 {
                    Self::Complete {
                        measurements: Vec::new(),
                        minutes_appart: *minutes_appart,
                    }
                } else {
                    let mut measurements = Vec::with_capacity(48);
                    measurements[0..13].copy_from_slice(&packet[3..packet.len() - 1]);
                    Self::Receiving {
                        target_length: *length - 1,
                        measurements,
                        minutes_appart: *minutes_appart,
                    }
                }
            }
            Self::Receiving {
                target_length,
                measurements,
                minutes_appart,
            } => {
                if packet[1] == 1 {
                    measurements.extend_from_slice(&packet[3..packet.len() - 1]);
                    return Ok(());
                } else {
                    measurements.extend_from_slice(&packet[2..packet.len() - 1]);
                    if *target_length == packet[1] {
                        let measurements = std::mem::take(measurements);
                        Self::Complete {
                            measurements,
                            minutes_appart: *minutes_appart,
                        }
                    } else {
                        return Ok(());
                    }
                }
            }
            Self::Complete { .. } => return Err(format!("Step after complete: {self:?}").into()),
        };
        Ok(())
    }
}
