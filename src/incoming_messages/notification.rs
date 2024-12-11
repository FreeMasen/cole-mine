use crate::constants;

#[derive(Debug, Clone, Copy, PartialEq, serde::Deserialize, serde::Serialize)]
pub enum Notification {
    NewData(DataName),
    Activity(LiveActivity),
    Battery(u8),
}

impl TryFrom<&[u8]> for Notification {
    type Error = String;
    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        let tag = value
            .first()
            .copied()
            .ok_or_else(|| format!("0 size buffer for notification packet"))?;

        if tag != constants::CMD_NOTIFICATION {
            return Err(format!("Invalid notification packet, tag byte not {}: {value:?}", constants::CMD_NOTIFICATION))
        }
        Ok(match value[1] {
            constants::NOTIFICATION_NEW_HR_DATA => Notification::NewData(DataName::HeartRate),
            constants::NOTIFICATION_NEW_SPO2_DATA => Notification::NewData(DataName::Oxygen),
            constants::NOTIFICATION_NEW_STEPS_DATA => Notification::NewData(DataName::Steps),
            constants::NOTIFICATION_BATTERY_LEVEL => {
                Notification::Battery(value[2])
            },
            constants::NOTIFICATION_LIVE_ACTIVITY => {
                Notification::Activity(LiveActivity::try_from(value)?)
            }
            _ => return Err(format!("Unknown notification type {}: {:?}", value[1], value)),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Deserialize, serde::Serialize)]
pub enum DataName {
    HeartRate,
    Oxygen,
    Steps,
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct LiveActivity {
    pub steps: u32,
    pub calories: f32,
    pub distance: u32,
}

impl TryFrom<&[u8]> for LiveActivity {
    type Error = String;

    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        if value.len() < 11 {
            return Err(format!("LiveActivity packet too short ({}): {value:?}", value.len()))
        }
        let steps = [value[4], value[3], value[2], 0];
        let steps = u32::from_le_bytes(steps);
        let calories = [value[7], value[6], value[5], 0];
        let calories = u32::from_le_bytes(calories);
        let distance = [value[10], value[9], value[8], 0];
        let distance = u32::from_le_bytes(distance);
        Ok(Self {
            steps,
            calories: (calories as f32) / 10.0,
            distance,
        })
    }
}
