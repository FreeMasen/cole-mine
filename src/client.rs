use bleasy::{Characteristic, Device, ScanConfig};
use futures::{FutureExt, StreamExt};

use crate::{
    constants,
    incoming_messages::{ClientReceiver, CommandReply},
    Result,
};

pub struct Client {
    pub device: Device,
    rx: Option<ClientReceiver>,
    tx: Characteristic,
    tx2: Characteristic,
}

#[derive(Default, serde::Deserialize, serde::Serialize)]
pub struct DeviceDetails {
    pub hw: Option<String>,
    pub fw: Option<String>,
}

impl Client {
    pub async fn new(addr: impl Into<bleasy::BDAddr>) -> Result<Self> {
        let addr = addr.into();
        let mut s = bleasy::Scanner::new();
        s.start(ScanConfig::default().filter_by_address(move |w| w == addr))
            .await?;
        let device = s
            .device_stream()
            .next()
            .await
            .ok_or_else(|| "No device found".to_string())?;
        Self::with_device(device).await
    }

    pub async fn with_device(device: Device) -> Result<Self> {
        let (tx, tx2) = Self::find_tx_characteristics(&device)
            .await
            .map_err(|e| format!("Error looking up uart_rx characteristic: {e}"))?;
        Ok(Self {
            device,
            tx,
            tx2,
            rx: None,
        })
    }

    pub async fn connect(&mut self) -> Result {
        self.rx = Some(ClientReceiver::connect_device(&self.device).await?);
        Ok(())
    }

    pub async fn disconnect(&mut self) -> Result {
        self.device.disconnect().await?;
        if let Some(rx) = self.rx.take() {
            rx.disconnect().await?
        }
        Ok(())
    }

    pub async fn send(&mut self, command: Command) -> Result {
        log::trace!("sending {command:?}");
        let cmd_bytes: [u8; 16] = command.into();
        log::trace!("serialized: {cmd_bytes:?}");
        if cmd_bytes[0] == crate::constants::CMD_BIG_DATA_V2
        || cmd_bytes[0] == crate::constants::CMD_NOTIFICATION {
            self.tx2.write_command(&cmd_bytes).await?;
        } else {
            self.tx.write_command(&cmd_bytes).await?;
        }
        Ok(())
    }

    pub async fn read_next(&mut self) -> Result<Option<CommandReply>> {
        if self.rx.is_none() {
            self.connect().await?;
        }
        let Some(rx) = &mut self.rx else {
            return Err("fatal error, rx was none after `connect`"
                .to_string()
                .into());
        };
        Ok(rx
            .next()
            .map(|rply| {
                log::trace!("reply: {rply:?}");
                rply
            })
            .await)
    }

    async fn find_tx_characteristics(device: &Device) -> Result<(Characteristic, Characteristic)> {
        let mut one = None;
        let mut two = None;
        let services = device.services().await?;
        'services: for service in services {
            if one.is_some() && two.is_some() {
                break;
            }
            if service.uuid() == crate::constants::UART_SERVICE_UUID {
                for ch in service.characteristics() {
                    if ch.uuid() == crate::constants::UART_RX_CHAR_UUID {
                        one = Some(ch);
                        continue 'services;
                    }
                }
            }
            if service.uuid() == crate::constants::CHARACTERISTIC_SERVICE_V2 {
                for ch in service.characteristics() {
                    if ch.uuid() == crate::constants::CHARACTERISTIC_COMMAND {
                        two = Some(ch);
                        continue 'services;
                    }
                }
            }
        }
        match (one, two) {
            (Some(one), Some(two)) => Ok((one, two)),
            (Some(_), None) => Err("failed to find v2 characteristic".into()),
            (None, Some(_)) => Err("failed to find uart characteristic".into()),
            (None, None) => Err("no characteristics found".into()),
        }
    }

    pub async fn device_details(&self) -> Result<DeviceDetails> {
        let services = self.device.services().await?;
        let service = services
            .into_iter()
            .find(|s| s.uuid() == crate::constants::DEVICE_INFO_UUID)
            .ok_or_else(|| "Unable to find service with device info uuid".to_string())?;
        let mut ret = DeviceDetails::default();
        for ch in service.characteristics() {
            if ch.uuid() == crate::constants::DEVICE_HW_UUID {
                if let Ok(bytes) = ch.read().await {
                    ret.hw = String::from_utf8(bytes).ok()
                }
            }
            if ch.uuid() == crate::constants::DEVICE_FW_UUID {
                if let Ok(bytes) = ch.read().await {
                    ret.fw = String::from_utf8(bytes).ok()
                }
            }
            if ret.fw.is_some() && ret.hw.is_some() {
                break;
            }
        }

        Ok(ret)
    }
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(tag = "command", content = "data", rename_all = "camelCase")]
pub enum Command {
    ReadSportDetail {
        day_offset: u8,
    },
    ReadHeartRate {
        timestamp: u32,
    },
    ReadStress {
        day_offset: u8,
    },
    GetHeartRateSettings,
    SetHeartRateSettings {
        enabled: bool,
        interval: u8,
    },
    StartRealTimeHeartRate,
    ContinueRealTimeHeartRate,
    StopRealTimeHeartRate,
    StartSpo2,
    StopSpo2,
    Reboot,
    SetTime {
        when: time::OffsetDateTime,
        language: u8,
    },
    BlinkTwice,
    BatteryInfo,
    SyncOxygen,
    SyncSleep,
    Raw(Vec<u8>),
}

impl From<Command> for [u8; 16] {
    fn from(cmd: Command) -> [u8; 16] {
        let mut ret = [0u8; 16];
        match cmd {
            Command::ReadSportDetail { day_offset } => {
                ret[0..6].copy_from_slice(&[67, day_offset, 0x0f, 0x00, 0x5f, 0x01]);
            }
            Command::ReadHeartRate { timestamp } => {
                ret[0] = 21;
                ret[1..5].copy_from_slice(&timestamp.to_le_bytes());
            }
            Command::ReadStress { day_offset } => {
                ret[0] = 55;
                ret[1] = day_offset;
            }
            Command::GetHeartRateSettings => {
                ret[0..2].copy_from_slice(&[22, 1]);
            }
            Command::SetHeartRateSettings { enabled, interval } => {
                ret[0] = 22;
                ret[1] = 2;
                ret[2] = if enabled { 1 } else { 2 };
                ret[3] = interval;
            }
            Command::StartRealTimeHeartRate => {
                ret[0..2].copy_from_slice(&[105, 1]);
            }
            Command::ContinueRealTimeHeartRate => {
                ret[0..2].copy_from_slice(&[30, 3]);
            }
            Command::StopRealTimeHeartRate => {
                ret[0..2].copy_from_slice(&[106, 1]);
            }
            Command::StartSpo2 => {
                ret[0..3].copy_from_slice(&[105, 0x03, 0x25]);
            }
            Command::StopSpo2 => {
                ret[0..2].copy_from_slice(&[106, 0x03]);
            }
            Command::Reboot => {
                ret[0..2].copy_from_slice(&[8, 1]);
            }
            Command::SetTime { when, language } => {
                ret[0..8].copy_from_slice(&[
                    constants::CMD_SET_DATE_TIME,
                    // 2 digit year...
                    (when.year().unsigned_abs() % 2000) as u8,
                    when.month().into(),
                    when.day(),
                    when.hour(),
                    when.minute(),
                    when.second(),
                    language,
                ]);
            }
            Command::BlinkTwice => {
                ret[0] = 16;
            }
            Command::BatteryInfo => {
                ret[0] = 3;
            }
            Command::SyncSleep => {
                ret[0] = constants::CMD_BIG_DATA_V2;
                ret[1] = constants::BIG_DATA_TYPE_SLEEP;
                ret[2] = 1;
                ret[3] = 0;
                ret[4] = 0xff;
                ret[5] = 0;
                ret[6] = 0xff;
            }
            Command::SyncOxygen => {
                ret[0] = constants::CMD_BIG_DATA_V2;
                ret[1] = constants::BIG_DATA_TYPE_SPO2;
                ret[2] = 1;
                ret[3] = 0;
                ret[4] = 0xff;
                ret[5] = 0;
                ret[6] = 0xff;
            }
            Command::Raw(mut bytes) => {
                if bytes.len() > 15 {
                    log::warn!("truncating message longer than 15 bytes");
                }
                bytes.resize(16, 0);
                ret[0..15].copy_from_slice(&bytes[0..15]);
            }
        }
        ret[15] = checksum(&ret);
        ret
    }
}

fn checksum(packet: &[u8]) -> u8 {
    let sum: u32 = packet.iter().copied().map(|v| v as u32).sum();
    let trunc = sum & 255;
    trunc as u8
}

#[cfg(test)]
mod tests {

    use std::collections::VecDeque;

    use time::macros::date;

    use crate::incoming_messages::{
        big_data::{BigDataPacket, BigDataState, SleepData},
        RawPacket,
    };

    use super::*;

    #[test]
    fn commands_serialize() {
        use Command::*;
        let commands: Vec<[u8; 16]> = [
            ReadSportDetail { day_offset: 0 },
            ReadHeartRate { timestamp: 0 },
            GetHeartRateSettings,
            SetHeartRateSettings {
                enabled: false,
                interval: 0,
            },
            StartRealTimeHeartRate,
            ContinueRealTimeHeartRate,
            StopRealTimeHeartRate,
            StartSpo2,
            StopSpo2,
            Reboot,
            SetTime {
                when: time::OffsetDateTime::from_unix_timestamp(0).unwrap(),
                language: 0,
            },
            BlinkTwice,
            BatteryInfo,
        ]
        .into_iter()
        .map(|cmd| {
            let bytes: [u8; 16] = cmd.into();
            bytes
        })
        .collect();
        insta::assert_debug_snapshot!(commands);
    }

    #[tokio::test]
    async fn parse_reply_battery_not_charging() {
        let expected = CommandReply::BatteryInfo {
            charging: false,
            level: 1,
        };

        let mut packet = [0u8; 16];
        packet[0] = 3;
        packet[1] = 1;
        let mut rx = ClientReceiver::from_stream(Box::pin(futures::stream::once(async move {
            RawPacket::Uart(packet.to_vec())
        })));
        let parsed = rx.next().await.unwrap();
        assert_eq!(parsed, expected);
    }

    #[tokio::test]
    async fn parse_reply_battery_charging() {
        let expected = CommandReply::BatteryInfo {
            charging: true,
            level: 2,
        };

        let mut packet = [0u8; 16];
        packet[0] = 3;
        packet[1] = 2;
        packet[2] = 1;
        let mut rx = ClientReceiver::from_stream(Box::pin(futures::stream::once(async move {
            RawPacket::Uart(packet.to_vec())
        })));
        let parsed = rx.next().await.unwrap();
        assert_eq!(parsed, expected);
    }

    #[tokio::test]
    async fn parse_reply_hear_rate_settings_disabled() {
        let expected = CommandReply::HeartRateSettings {
            enabled: false,
            interval: 0,
        };
        let stream = futures::stream::iter([RawPacket::Uart(make_packet(&[22, 0, 2]))]);
        let mut rx = ClientReceiver::from_stream(Box::pin(stream));
        let parsed = rx.next().await.unwrap();
        assert_eq!(parsed, expected);
    }

    #[tokio::test]
    async fn parse_reply_hear_rate_settings_enabled() {
        let expected = CommandReply::HeartRateSettings {
            enabled: true,
            interval: 127,
        };
        let stream = futures::stream::iter([RawPacket::Uart(make_packet(&[22, 0, 1, 127]))]);
        let mut rx = ClientReceiver::from_stream(Box::pin(stream));
        let parsed = rx.next().await.unwrap();
        assert_eq!(parsed, expected);
    }

    #[tokio::test]
    async fn big_data_sleep() {
        let mut packets = VecDeque::from_iter([
            [
                188, 39, 71, 0, 202, 141, 2, 2, 26, 177, 0, 11, 2, 2, 67, 3, 35, 2, 15, 4,
            ]
            .to_vec(),
            [
                34, 2, 95, 3, 16, 2, 1, 5, 13, 2, 49, 3, 18, 2, 3, 0, 40, 9, 0, 224,
            ]
            .to_vec(),
            [
                1, 2, 61, 3, 31, 2, 15, 4, 33, 3, 31, 2, 31, 4, 34, 3, 33, 2, 17, 4,
            ]
            .to_vec(),
            [15, 2, 10, 0, 1, 2, 29, 5, 6, 2, 55, 5, 12, 2, 50, 2, 7].to_vec(),
        ]);
        let initial = packets.pop_front().unwrap();
        let mut state = BigDataState::new(&initial).unwrap();
        for packet in packets {
            state.step(packet.as_slice()).unwrap();
        }
        let packet = match state {
            BigDataState::Complete(packet) => packet,
            BigDataState::Partial {
                target_length,
                packet,
            } => {
                panic!(
                    "Expected complete, found {target_length} {}/{}",
                    packet.len(),
                    packet.capacity()
                );
            }
        };
        let mut sleep_data: SleepData = packet.try_into().unwrap();
        sleep_data.sessions[0].start = sleep_data.sessions[0]
            .start
            .replace_date(date!(2024 - 11 - 26));
        sleep_data.sessions[0].end = sleep_data.sessions[0]
            .end
            .replace_date(date!(2024 - 11 - 27));
        insta::assert_debug_snapshot!(sleep_data);
    }

    #[tokio::test]
    async fn big_data_sleep2() {
        env_logger::builder().is_test(true).try_init().ok();
        let expected_dates = [
            date!(2024 - 11 - 22),
            date!(2024 - 11 - 23),
            date!(2024 - 11 - 24),
            date!(2024 - 11 - 25),
            date!(2024 - 11 - 25),
            date!(2024 - 11 - 26),
            date!(2024 - 11 - 27),
            date!(2024 - 11 - 27),
        ];
        let packet = vec![
            5u8, 6, 26, 177, 0, 11, 2, 2, 67, 3, 35, 2, 15, 4, 34, 2, 95, 3, 16, 2, 1, 5, 13, 2,
            49, 3, 18, 2, 3, 4, 40, 9, 0, 224, 1, 2, 61, 3, 31, 2, 15, 4, 33, 3, 31, 2, 31, 4, 34,
            3, 33, 2, 17, 4, 15, 2, 10, 0, 1, 2, 29, 5, 6, 2, 55, 5, 12, 2, 50, 2, 7, 3, 32, 0, 0,
            251, 1, 2, 73, 3, 18, 2, 18, 4, 31, 3, 33, 2, 31, 4, 33, 2, 16, 3, 18, 2, 15, 4, 17, 2,
            34, 3, 33, 2, 137, 2, 36, 159, 5, 4, 2, 2, 71, 3, 16, 2, 35, 4, 18, 3, 34, 2, 30, 4,
            33, 2, 101, 3, 32, 2, 17, 4, 15, 2, 32, 3, 18, 2, 29, 5, 13, 2, 23, 1, 12, 66, 0, 214,
            0, 2, 72, 3, 30, 2, 17, 4, 29,
        ];
        let mut dates = expected_dates.iter().copied();
        let mut sleep_data: SleepData = BigDataPacket::Sleep(packet).try_into().unwrap();
        for session in sleep_data.sessions.iter_mut() {
            let date = dates.next().unwrap();
            session.start = session.start.replace_date(date);
            let date = dates.next().unwrap();
            session.end = session.end.replace_date(date);
        }
        insta::assert_debug_snapshot!(&sleep_data)
    }

    fn make_packet(bytes: &[u8]) -> Vec<u8> {
        let mut ret = bytes.to_vec();
        ret.resize(16, 0);
        ret[15] = checksum(&ret);
        ret
    }
}
