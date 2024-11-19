use std::pin::Pin;

use bleasy::{Characteristic, Device, ScanConfig, Service};
use futures::{channel::mpsc, SinkExt, Stream, StreamExt};

use crate::{
    heart_rate::{HeartRate, HeartRateState},
    sport_detail::{SportDetail, SportDetailState},
    Result,
};

pub struct Client {
    device: Device,
    rx: Option<ClientReceiver>,
    tx: Characteristic,
}

pub struct ClientReceiver(Pin<Box<dyn Stream<Item = CommandReply>>>);

#[derive(Default)]
pub struct DeviceDetails {
    hw: Option<String>,
    fw: Option<String>,
}

#[derive(Default)]
pub struct MultiPacketStates {
    sport_detail: Option<SportDetailState>,
    heart_rate_state: Option<HeartRateState>,
}

impl Client {
    pub async fn new(addr: impl Into<bleasy::BDAddr>) -> Result<Self> {
        let addr = addr.into();
        let mut s = bleasy::Scanner::new();
        s.start(
            ScanConfig::default()
                .filter_by_address(move |w| w == addr)
                .stop_after_first_match()
                .stop_after_timeout(std::time::Duration::from_secs(5)),
        )
        .await?;
        let device = s
            .device_stream()
            .next()
            .await
            .ok_or_else(|| format!("No device found"))?;
        let tx = Self::find_uart_tx_characteristic(&device).await?;
        Ok(Self {
            device,
            tx,
            rx: None,
        })
    }

    pub async fn connect(&mut self) -> Result {
        let charas = self.find_uart_rx_characteristic().await?;
        let mut incoming_stream = charas.subscribe().await?;
        self.rx = Some(ClientReceiver(
            async_stream::stream! {
                let mut partial_states = MultiPacketStates::default();
                while let Some(ev) = incoming_stream.next().await {
                    let Some(tag) = ev.get(0) else {
                        continue;
                    };
                    let mut packet = [0u8; 16];
                    packet.copy_from_slice(&ev);
                    let cmd = match *tag {
                        3 => CommandReply::BatteryInfo {
                            level: ev[1],
                            charging: ev[2] > 0,
                        },
                        30 => {
                            if let Some(mut s) = partial_states.heart_rate_state.take() {
                                if s.step(packet).is_err() {
                                    continue;
                                }
                                let HeartRateState::Complete { date, range, rates} = s else {
                                    partial_states.heart_rate_state = Some(s);
                                    continue;
                                };
                                CommandReply::HeartRate(HeartRate { range, rates, date })
                            } else {
                                partial_states.sport_detail = SportDetailState::new(packet).ok();
                                continue;
                            }
                        },
                        67 => {
                            if let Some(mut ss) = partial_states.sport_detail.take() {
                                if ss.step(packet).is_err() {
                                    continue;
                                }
                                let SportDetailState::Complete { packets } = ss else {
                                    partial_states.sport_detail = Some(ss);
                                    continue;
                                };
                                CommandReply::Steps(packets)
                            } else {
                                partial_states.sport_detail = SportDetailState::new(packet).ok();
                                continue;
                            }
                        }
                        105 => {
                            let ev = if packet[2] != 0 {
                                RealTimeEvent::Error(packet[2])
                            } else {
                                if packet[1] == 1 {
                                    RealTimeEvent::HeartRate(packet[3])
                                } else {
                                    RealTimeEvent::Oxygen(packet[3])
                                }
                            };
                            CommandReply::RealTimeData(ev)
                        }
                        _ => CommandReply::Unknown(ev),
                    };
                    yield cmd;
                }

            }
            .boxed_local(),
        ));
        Ok(())
    }

    pub async fn send(&mut self, command: Command) -> Result {
        let cmd_bytes: [u8; 16] = command.into();
        Ok(self.tx.write_command(&cmd_bytes).await?)
    }

    pub async fn read_next(&mut self) -> Result<Option<CommandReply>> {
        if self.rx.is_none() {
            self.connect().await?;
        }
        let Some(rx) = &mut self.rx else {
            return Err(format!("fatal error, rx was none after `connect`").into());
        };
        Ok(rx.0.next().await)
    }

    async fn find_uart_rx_characteristic(&self) -> Result<Characteristic> {
        let service = Self::find_uart_service(&self.device).await?;
        let char = service
            .characteristics()
            .into_iter()
            .find(|ch| ch.uuid() == crate::UART_RX_CHAR_UUID)
            .ok_or_else(|| format!("Unable to find RX characteristic"))?;
        Ok(char)
    }

    async fn find_uart_tx_characteristic(device: &Device) -> Result<Characteristic> {
        let service = Self::find_uart_service(device).await?;
        let char = service
            .characteristics()
            .into_iter()
            .find(|ch| ch.uuid() == crate::UART_TX_CHAR_UUID)
            .ok_or_else(|| format!("Unable to find TX characteristic"))?;
        Ok(char)
    }

    async fn find_uart_service(device: &Device) -> Result<Service> {
        Ok(device
            .services()
            .await?
            .into_iter()
            .find(|s| s.uuid() == crate::UART_SERVICE_UUID)
            .ok_or_else(|| format!("Unable to find UART service"))?)
    }

    pub async fn device_details(&self) -> Result<DeviceDetails> {
        let services = self.device.services().await?;
        let service = services
            .into_iter()
            .find(|s| s.uuid() == crate::DEVICE_INFO_UUID)
            .ok_or_else(|| format!("Unable to find service with device info uuid"))?;
        let mut ret = DeviceDetails::default();
        for ch in service.characteristics() {
            if ch.uuid() == crate::DEVICE_HW_UUID {
                if let Ok(bytes) = ch.read().await {
                    ret.hw = String::from_utf8(bytes).ok()
                }
            }
            if ch.uuid() == crate::DEVICE_FW_UUID {
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

pub enum Command {
    ReadSteps {
        day_offset: u8,
    },
    ReadHeartRate {
        timestamp: u32,
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
}

impl Into<[u8; 16]> for Command {
    fn into(self) -> [u8; 16] {
        let mut ret = [0u8; 16];
        match self {
            Self::ReadSteps { day_offset } => {
                ret.copy_from_slice(&[67, day_offset, 0x0f, 0x00, 0x5f, 0x01]);
            }
            Self::ReadHeartRate { timestamp } => {
                ret[0] = 21;
                (&mut ret[1..]).copy_from_slice(&timestamp.to_le_bytes());
            }
            Self::GetHeartRateSettings => {
                ret.copy_from_slice(&[22, 1]);
            }
            Self::SetHeartRateSettings { enabled, interval } => {
                ret[0] = 22;
                ret[1] = 2;
                ret[2] = enabled.then_some(1).unwrap_or(2);
                ret[3] = interval;
            }
            Self::StartRealTimeHeartRate => {
                ret.copy_from_slice(&[105, 1]);
            }
            Self::ContinueRealTimeHeartRate => {
                ret.copy_from_slice(&[30, 3]);
            }
            Self::StopRealTimeHeartRate => {
                ret.copy_from_slice(&[106, 1]);
            }
            Self::StartSpo2 => {
                ret.copy_from_slice(&[105, 0x03, 0x25]);
            }
            Self::StopSpo2 => {
                ret.copy_from_slice(&[106, 0x03]);
            }
            Self::Reboot => {
                ret.copy_from_slice(&[8, 1]);
            }
            Self::SetTime { when, language } => {
                ret.copy_from_slice(&[
                    // 2 digit year...
                    ((when.year().abs() as u32) % 2000) as u8,
                    when.month().into(),
                    when.day(),
                    when.hour(),
                    when.minute(),
                    when.second(),
                    language,
                ]);
            }
            Self::BlinkTwice => {
                ret[0] = 16;
            }
            Self::BatteryInfo => {
                ret[0] = 3;
            }
        }
        ret[15] = checksum(&ret);
        ret
    }
}

pub enum CommandReply {
    BatteryInfo { level: u8, charging: bool },
    HeartRateSettings { enabled: bool, interval: u8 },
    Steps(Vec<SportDetail>),
    HeartRate(HeartRate),
    RealTimeData(RealTimeEvent),
    Unknown(Vec<u8>),
}

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
