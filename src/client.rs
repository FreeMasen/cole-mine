use std::pin::Pin;

use bleasy::{Characteristic, Device, ScanConfig};
use futures::{FutureExt, Stream, StreamExt};

use crate::{
    heart_rate::{HeartRate, HeartRateState},
    sport_detail::{SportDetail, SportDetailState},
    stress::StressState,
    Result,
};

pub struct Client {
    device: Device,
    rx: Option<ClientReceiver>,
    tx: Characteristic,
    tx2: Characteristic,
}

pub struct ClientReceiver(Pin<Box<dyn Stream<Item = CommandReply>>>);

impl futures::Stream for ClientReceiver {
    type Item = CommandReply;

    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        self.0.poll_next_unpin(cx)
    }
}

impl ClientReceiver {
    pub async fn connect_device(device: &Device) -> Result<Self> {
        let mut streams = Vec::with_capacity(2);
        for s in device.services().await? {
            if s.uuid() == crate::constants::UART_SERVICE_UUID {
                for ch in s.characteristics() {
                    if ch.uuid() == crate::constants::UART_TX_CHAR_UUID {
                        streams.push(ch.subscribe().await?)
                    }
                }
            }
            if s.uuid() == crate::constants::CHARACTERISTIC_SERVICE_V2 {
                for ch in s.characteristics() {
                    if ch.uuid() == crate::constants::CHARACTERISTIC_NOTIFY_V2 {
                        streams.push(ch.subscribe().await?)
                    }
                }
            }
        }

        Ok(Self::from_stream(Box::pin(futures::stream::select_all(
            streams,
        ))))
    }

    pub fn from_stream(mut stream: Pin<Box<dyn Stream<Item = Vec<u8>>>>) -> Self {
        ClientReceiver(
            async_stream::stream! {
                let mut partial_states = MultiPacketStates::default();
                while let Some(ev) = stream.next().await {
                    log::trace!("raw packet: {ev:?}");
                    let Some(tag) = ev.first() else {
                        continue;
                    };
                    
                    let cmd = match *tag {
                        1 => {
                            log::debug!("SetTime Reply");
                            CommandReply::SetTime
                        },
                        3 =>{
                            log::debug!("Battery Info Reply {}, {}", ev[1], ev[2]);
                            CommandReply::BatteryInfo {
                                level: ev[1],
                                charging: ev[2] > 0,
                            }
                        },
                        8 => {
                            log::debug!("Reboot Reply");
                            CommandReply::Reboot
                        },
                        16 => {
                            log::debug!("BlinkTwice Reply");
                            CommandReply::BlinkTwice
                        },
                        21 => {
                            log::debug!("Heart Rate Reply");
                            if let Some(mut s) = partial_states.heart_rate_state.take() {
                                log::debug!("Stepping heart rate state");
                                if s.step(&ev[..ev.len()-1]).is_err() {
                                    continue;
                                }
                                let HeartRateState::Complete { date, range, rates} = s else {
                                    partial_states.heart_rate_state = Some(s);
                                    continue;
                                };
                                log::debug!("hear rate state complete");
                                CommandReply::HeartRate(HeartRate { range, rates, date })
                            } else {
                                
                                match HeartRateState::try_from(&ev[1..ev.len()-1]) {
                                    Ok(HeartRateState::Complete { date, range, rates}) => {
                                        CommandReply::HeartRate(HeartRate { range, rates, date })
                                    },
                                    Ok(other) => {
                                        partial_states.heart_rate_state = Some(other);
                                        continue;
                                    },
                                    Err(e) => {
                                        log::error!("failed to convert heart rate packet to state {ev:?}: {e}");
                                        CommandReply::Unknown(ev)
                                    }
                                }
                            }
                        },
                        22 if ev[2] == 1 || ev[2] == 2 => {
                            log::debug!("HeartRateSettings reply");
                            CommandReply::HeartRateSettings { enabled: ev[2] == 1, interval: ev[3] }
                        },
                        55 => {
                            log::debug!("Stress reply");
                            if let Some(mut ss) = partial_states.stress_state.take() {
                                if ss.step(&ev).is_err() {
                                    continue;
                                }
                                let StressState::Complete { measurements, minutes_appart } = ss else {
                                    partial_states.stress_state = Some(ss);
                                    continue;
                                };
                                CommandReply::Stress {
                                    time_interval_sec: minutes_appart,
                                    measurements,
                                }
                            } else {
                                partial_states.stress_state = StressState::new(&ev).ok();
                                continue;
                            }
                        }
                        67 => {
                            log::debug!("Sport Detail reply");
                            if let Some(mut ss) = partial_states.sport_detail.take() {
                                if ss.step(&ev).is_err() {
                                    continue;
                                }
                                let SportDetailState::Complete { packets } = ss else {
                                    partial_states.sport_detail = Some(ss);
                                    continue;
                                };
                                CommandReply::SportDetail(packets)
                            } else {
                                partial_states.sport_detail = SportDetailState::new(&ev).ok();
                                continue;
                            }
                        },
                        105 => {
                            log::debug!("RealTime Reply");
                            let ev = if ev[2] != 0 {
                                RealTimeEvent::Error(ev[2])
                            } else if ev[1] == 1 {
                                RealTimeEvent::HeartRate(ev[3])
                            } else {
                                RealTimeEvent::Oxygen(ev[3])
                            };
                            CommandReply::RealTimeData(ev)
                        }
                        106 => {
                            log::debug!("StopRealTime reply");
                            CommandReply::StopRealTime
                        },
                        188 => {
                            log::debug!("BigData Reply: {ev:?}");
                            CommandReply::Unknown(ev)
                        }
                        _ => {
                            log::debug!("Unknown reply");
                            CommandReply::Unknown(ev)
                        },
                    };
                    yield cmd;
                }

            }
            .boxed_local(),
        )
    }
}

#[derive(Default, serde::Deserialize, serde::Serialize)]
pub struct DeviceDetails {
    pub hw: Option<String>,
    pub fw: Option<String>,
}

#[derive(Default)]
pub struct MultiPacketStates {
    sport_detail: Option<SportDetailState>,
    heart_rate_state: Option<HeartRateState>,
    stress_state: Option<StressState>,
}

impl Client {
    pub async fn new(addr: impl Into<bleasy::BDAddr>) -> Result<Self> {
        let addr = addr.into();
        let mut s = bleasy::Scanner::new();
        s.start(
            ScanConfig::default()
                .filter_by_address(move |w| w == addr)
                .stop_after_first_match(),
        )
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
        self.rx = None;
        Ok(())
    }

    pub async fn send(&mut self, command: Command) -> Result {
        log::trace!("sending {command:?}");
        let cmd_bytes: [u8; 16] = command.into();
        log::trace!("serialized: {cmd_bytes:?}");
        if cmd_bytes[0] == crate::constants::CMD_BIG_DATA_V2 {
            self.tx2.write_command(&cmd_bytes).await?;
        } else {
            self
            .tx
            .write_command(&cmd_bytes)
            .await?;
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
            .0
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
        for service in &services {
            println!("{}", service.uuid());
            for char in service.characteristics() {
                println!("  {}", char.uuid());
            }
        }
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
                    1,
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
            Command::Raw(mut bytes) => {
                if bytes.len() > 15 {
                    log::warn!("truncating message longer than 15 bytes");
                    bytes.resize(16, 0);
                }
                ret[0..15].copy_from_slice(&bytes[0..15]);
            }
        }
        ret[15] = checksum(&ret);
        ret
    }
}

#[derive(Debug, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(tag = "command", content = "data", rename_all = "camelCase")]
pub enum CommandReply {
    BatteryInfo {
        level: u8,
        charging: bool,
    },
    HeartRateSettings {
        enabled: bool,
        interval: u8,
    },
    SportDetail(Vec<SportDetail>),
    HeartRate(HeartRate),
    RealTimeData(RealTimeEvent),
    BlinkTwice,
    SetTime,
    Reboot,
    StopRealTime,
    SetHrSettings,
    Stress {
        time_interval_sec: u8,
        measurements: Vec<u8>,
    },
    Unknown(Vec<u8>),
}

#[derive(Debug, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(tag = "event", content = "value", rename_all = "camelCase")]
pub enum RealTimeEvent {
    HeartRate(u8),
    Oxygen(u8),
    Error(u8),
}

fn checksum(packet: &[u8]) -> u8 {
    let sum: u32 = packet.iter().copied().map(|v| v as u32).sum();
    let trunc = sum & 255;
    trunc as u8
}

#[cfg(test)]
mod tests {

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
        let mut rx =
            ClientReceiver::from_stream(Box::pin(futures::stream::once(
                async move { packet.to_vec() },
            )));
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
        let mut rx =
            ClientReceiver::from_stream(Box::pin(futures::stream::once(
                async move { packet.to_vec() },
            )));
        let parsed = rx.next().await.unwrap();
        assert_eq!(parsed, expected);
    }

    #[tokio::test]
    async fn parse_reply_hear_rate_settings_disabled() {
        let expected = CommandReply::HeartRateSettings {
            enabled: false,
            interval: 0,
        };
        let stream = futures::stream::iter([make_packet(&[22, 0, 2])]);
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
        let stream = futures::stream::iter([make_packet(&[22, 0, 1, 127])]);
        let mut rx = ClientReceiver::from_stream(Box::pin(stream));
        let parsed = rx.next().await.unwrap();
        assert_eq!(parsed, expected);
    }

    fn make_packet(bytes: &[u8]) -> Vec<u8> {
        let mut ret = bytes.to_vec();
        ret.resize(16, 0);
        ret[15] = checksum(&ret);
        ret
    }
}
