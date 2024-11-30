use std::{
    ops::Range,
    ops::{Index, RangeTo},
    pin::Pin,
};

use big_data::{BigDataState, OxygenData, SleepData};
use bleasy::{Characteristic, Device};
use futures::{Stream, StreamExt};
use heart_rate::{HeartRate, HeartRateState};
use sport_detail::{SportDetail, SportDetailState};
use stress::StressState;

pub mod big_data;
pub mod heart_rate;
pub mod sport_detail;
pub mod stress;

use crate::{constants, Result};

pub struct ClientReceiver {
    stream: Pin<Box<dyn Stream<Item = RawPacket>>>,
    parser: PacketParser,
    charas: Vec<Characteristic>,
}

#[derive(Debug, Default)]
struct PacketParser {
    multi_packet_states: MultiPacketStates,
}

impl PacketParser {
    fn handle_packet(&mut self, packet: &RawPacket) -> Option<CommandReply> {
        match packet {
            RawPacket::Uart(inner) => self.handle_uart(inner),
            RawPacket::V2(inner) => self.handle_v2(inner),
        }
        .inspect_err(|e| {
            log::warn!("Error parsing packet: {e}");
        })
        .ok()?
    }

    fn handle_uart(&mut self, packet: &[u8]) -> Result<Option<CommandReply>> {
        log::trace!("uart packet: {packet:?}");
        Ok(Some(match packet[0] {
            constants::CMD_SET_DATE_TIME => {
                log::debug!("SetTime Reply");
                CommandReply::SetTime
            }
            constants::CMD_BATTERY => {
                log::debug!("Battery Info Reply {}, {}", packet[1], packet[2]);
                CommandReply::BatteryInfo {
                    level: packet[1],
                    charging: packet[2] > 0,
                }
            }
            constants::CMD_POWER_OFF => {
                log::debug!("Reboot Reply");
                CommandReply::Reboot
            }
            constants::CMD_BLINK => {
                log::debug!("BlinkTwice Reply");
                CommandReply::BlinkTwice
            }
            constants::CMD_SYNC_HEART_RATE => {
                return self.handle_heart_rate(packet);
            }
            constants::CMD_AUTO_HR_PREF if packet[2] == 1 || packet[2] == 2 => {
                log::debug!("HeartRateSettings reply");
                CommandReply::HeartRateSettings {
                    enabled: packet[2] == 1,
                    interval: packet[3],
                }
            }
            constants::CMD_SYNC_STRESS => return self.handle_stress(packet),
            constants::CMD_SYNC_ACTIVITY => return self.handle_sport_detail(packet),
            constants::CMD_MANUAL_HEART_RATE => self.handle_real_time(packet),
            106 => {
                log::debug!("StopRealTime reply");
                CommandReply::StopRealTime
            }
            _ => {
                log::debug!("Unknown reply");
                CommandReply::Unknown(packet.to_vec())
            }
        }))
    }

    fn handle_v2(&mut self, packet: &[u8]) -> Result<Option<CommandReply>> {
        if let Some(s) = &mut self.multi_packet_states.partial_big_data {
            s.step(packet)?;
        } else {
            self.multi_packet_states.partial_big_data = Some(BigDataState::new(packet)?);
        }
        self.check_for_complete_big_data()
    }

    fn handle_real_time(&mut self, packet: &[u8]) -> CommandReply {
        log::debug!("RealTime Reply");
        let ev = if packet[2] != 0 {
            RealTimeEvent::Error(packet[2])
        } else if packet[1] == 1 {
            RealTimeEvent::HeartRate(packet[3])
        } else {
            RealTimeEvent::Oxygen(packet[3])
        };
        CommandReply::RealTimeData(ev)
    }

    fn handle_sport_detail(&mut self, packet: &[u8]) -> Result<Option<CommandReply>> {
        log::debug!("Sport Detail reply");
        if let Some(mut ss) = self.multi_packet_states.sport_detail.take() {
            ss.step(packet)?;
            let SportDetailState::Complete { packets } = ss else {
                self.multi_packet_states.sport_detail = Some(ss);
                return Ok(None);
            };
            Ok(Some(CommandReply::SportDetail(packets)))
        } else {
            self.multi_packet_states.sport_detail = SportDetailState::new(packet).ok();
            return Ok(None);
        }
    }

    fn handle_stress(&mut self, packet: &[u8]) -> Result<Option<CommandReply>> {
        log::debug!("Stress reply {:?}", self.multi_packet_states.stress_state);
        if let Some(mut ss) = self.multi_packet_states.stress_state.take() {
            ss.step(packet)?;
            let StressState::Complete {
                measurements,
                minutes_appart,
            } = ss
            else {
                self.multi_packet_states.stress_state = Some(ss);
                return Ok(None);
            };
            Ok(Some(CommandReply::Stress {
                time_interval_sec: minutes_appart,
                measurements,
            }))
        } else {
            self.multi_packet_states.stress_state = StressState::new(packet).ok();
            Ok(None)
        }
    }

    fn handle_heart_rate(&mut self, packet: &[u8]) -> Result<Option<CommandReply>> {
        log::debug!("Heart Rate Reply");
        Ok(Some(
            if let Some(mut s) = self.multi_packet_states.heart_rate_state.take() {
                log::debug!("Stepping heart rate state");
                // We need to trim the checksum byte here because the packet will be offset
                // if we don't
                if let Err(e) = s.step(&packet[..packet.len() - 1]) {
                    log::warn!("failed to step heart rate: {e}");
                    return Ok(None);
                }
                let HeartRateState::Complete { date, range, rates } = s else {
                    log::debug!("heart rate incomplete, waiting for remaining data: {s:?}");
                    self.multi_packet_states.heart_rate_state = Some(s);
                    return Ok(None);
                };
                log::debug!("hear rate state complete");
                CommandReply::HeartRate(HeartRate { range, rates, date })
            } else {
                log::debug!("Initial heart rate packet");
                match HeartRateState::try_from(packet) {
                    Ok(HeartRateState::Complete { date, range, rates }) => {
                        log::trace!("First packet was only packet for heart rate data");
                        CommandReply::HeartRate(HeartRate { range, rates, date })
                    }
                    Ok(other) => {
                        log::trace!("First packet incomplete, waiting for remaining bytes: {other:?}");
                        self.multi_packet_states.heart_rate_state = Some(other);
                        return Ok(None);
                    }
                    Err(e) => {
                        log::error!("failed to convert heart rate packet to state {packet:?}: {e}");
                        return Err(e);
                    }
                }
            },
        ))
    }

    fn check_for_complete_big_data(&mut self) -> Result<Option<CommandReply>> {
        match self.multi_packet_states.partial_big_data.take() {
            Some(BigDataState::Complete(packet)) => {
                let kind = packet.get_data_ref().get(1).copied().ok_or_else(|| {
                    format!("Error in big data packet, not long enough: {packet:?}")
                })?;
                match kind {
                    constants::BIG_DATA_TYPE_SLEEP => {
                        let sleep_data: SleepData = packet.try_into()?;
                        Ok(Some(CommandReply::Sleep(sleep_data)))
                    }
                    constants::BIG_DATA_TYPE_SPO2 => {
                        Ok(Some(CommandReply::Unknown(packet.get_data_ref().to_vec())))
                    }
                    _ => Err(format!("Unknown big data tag: {packet:?}").into()),
                }
            }
            state => {
                self.multi_packet_states.partial_big_data = state;
                Ok(None)
            }
        }
    }
}

impl futures::Stream for ClientReceiver {
    type Item = CommandReply;

    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        let std::task::Poll::Ready(inner) = self.stream.poll_next_unpin(cx) else {
            return std::task::Poll::Pending;
        };
        let Some(packet) = inner else {
            return std::task::Poll::Ready(None);
        };
        if let Some(packet) = self.parser.handle_packet(&packet) {
            return std::task::Poll::Ready(Some(packet))
        }
        std::task::Poll::Pending
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
    Sleep(SleepData),
    Oxygen(OxygenData),
    Unknown(Vec<u8>),
}

#[derive(Debug, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(tag = "event", content = "value", rename_all = "camelCase")]
pub enum RealTimeEvent {
    HeartRate(u8),
    Oxygen(u8),
    Error(u8),
}

impl ClientReceiver {
    pub async fn connect_device(device: &Device) -> Result<Self> {
        let mut streams = Vec::with_capacity(2);
        let mut charas = Vec::with_capacity(2);
        for s in device.services().await? {
            if s.uuid() == crate::constants::UART_SERVICE_UUID {
                for ch in s.characteristics() {
                    if ch.uuid() == crate::constants::UART_TX_CHAR_UUID {
                        let stream: Pin<Box<dyn Stream<Item = Vec<u8>>>> = ch.subscribe().await?;
                        let stream: Pin<Box<dyn Stream<Item = RawPacket>>> =
                            Box::pin(stream.map(RawPacket::Uart));
                        streams.push(stream);
                        charas.push(ch);
                    }
                }
            }
            if s.uuid() == crate::constants::CHARACTERISTIC_SERVICE_V2 {
                for ch in s.characteristics() {
                    if ch.uuid() == crate::constants::CHARACTERISTIC_NOTIFY_V2 {
                        let stream: Pin<Box<dyn Stream<Item = Vec<u8>>>> = ch.subscribe().await?;
                        let stream: Pin<Box<dyn Stream<Item = RawPacket>>> =
                            Box::pin(stream.map(RawPacket::V2));
                        streams.push(stream);
                        charas.push(ch);
                    }
                }
            }
        }
        let mut ret = Self::from_stream(Box::pin(futures::stream::select_all(
            streams,
        )));
        ret.charas = charas;
        Ok(ret)
    }

    pub fn from_stream(stream: Pin<Box<dyn Stream<Item = RawPacket>>>) -> Self {
        ClientReceiver {
            stream,
            parser: PacketParser::default(),
            charas: Default::default(),
        }
    }

    pub async fn disconnect(&self) -> Result {
        for ch in &self.charas {
            ch.unsubscribe().await?;
        }
        Ok(())
    }
}

#[derive(Debug, Default)]
pub struct MultiPacketStates {
    sport_detail: Option<SportDetailState>,
    heart_rate_state: Option<HeartRateState>,
    stress_state: Option<StressState>,
    partial_big_data: Option<BigDataState>,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, PartialEq)]
#[serde(untagged)]
pub enum RawPacket {
    Uart(Vec<u8>),
    V2(Vec<u8>),
}

impl Index<usize> for RawPacket {
    type Output = u8;

    fn index(&self, index: usize) -> &Self::Output {
        match self {
            RawPacket::Uart(vec) | RawPacket::V2(vec) => vec.index(index),
        }
    }
}

impl Index<Range<usize>> for RawPacket {
    type Output = [u8];

    fn index(&self, index: Range<usize>) -> &Self::Output {
        match self {
            RawPacket::Uart(vec) | RawPacket::V2(vec) => vec.index(index),
        }
    }
}

impl Index<RangeTo<usize>> for RawPacket {
    type Output = [u8];

    fn index(&self, index: RangeTo<usize>) -> &Self::Output {
        match self {
            RawPacket::Uart(vec) | RawPacket::V2(vec) => vec.index(index),
        }
    }
}

impl AsRef<[u8]> for RawPacket {
    fn as_ref(&self) -> &[u8] {
        match self {
            Self::Uart(vec) | Self::V2(vec) => vec.as_ref(),
        }
    }
}
