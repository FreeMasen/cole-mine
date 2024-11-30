use std::{
    fmt::Display,
    ops::{Index, Range, RangeTo},
    pin::Pin,
    time::Duration,
};

use bleasy::{Characteristic, Device, ScanConfig};
use futures::{FutureExt, Stream, StreamExt};
use time::{OffsetDateTime, UtcOffset};

use crate::{
    constants,
    heart_rate::{HeartRate, HeartRateState},
    sport_detail::{SportDetail, SportDetailState},
    stress::StressState,
    Result,
};

pub struct Client {
    pub device: Device,
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
                        let stream: Pin<Box<dyn Stream<Item = Vec<u8>>>> = ch.subscribe().await?;
                        let stream: Pin<Box<dyn Stream<Item = RawPacket>>> =
                            Box::pin(stream.map(RawPacket::Uart));
                        streams.push(stream);
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
                    }
                }
            }
        }

        Ok(Self::from_stream(Box::pin(futures::stream::select_all(
            streams,
        ))))
    }

    pub fn from_stream(mut stream: Pin<Box<dyn Stream<Item = RawPacket>>>) -> Self {
        ClientReceiver(
            async_stream::stream! {
                let mut partial_states = MultiPacketStates::default();
                while let Some(ev) = stream.next().await {
                    log::trace!("raw packet: {ev:?}");
                    let Some(tag) = ev.tag_byte().ok() else {
                        continue;
                    };
                    let cmd = match ev {
                        RawPacket::Uart(ev) => {
                            match tag {
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
                                    log::debug!("Stress reply {:?}", partial_states.stress_state);
                                    if let Some(mut ss) = partial_states.stress_state.take() {
                                        if ss.step(ev.as_ref()).is_err() {
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
                                        partial_states.stress_state = StressState::new(ev.as_ref()).ok();
                                        continue;
                                    }
                                }
                                67 => {
                                    log::debug!("Sport Detail reply");
                                    if let Some(mut ss) = partial_states.sport_detail.take() {
                                        if ss.step(ev.as_ref()).is_err() {
                                            continue;
                                        }
                                        let SportDetailState::Complete { packets } = ss else {
                                            partial_states.sport_detail = Some(ss);
                                            continue;
                                        };
                                        CommandReply::SportDetail(packets)
                                    } else {
                                        partial_states.sport_detail = SportDetailState::new(ev.as_ref()).ok();
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
                                _ => {
                                    log::debug!("Unknown reply");
                                    CommandReply::Unknown(ev)
                                },
                            }
                        },
                        RawPacket::V2(ev) => {
                            log::trace!("V2 Packet");
                            if tag == constants::CMD_BIG_DATA_V2 {
                                log::trace!("Big Data tag");
                                if !(ev[1] == constants::BIG_DATA_TYPE_SLEEP || ev[1] == constants::BIG_DATA_TYPE_SPO2) {
                                    log::warn!("Ignoring unknown big data packet {ev:?}");
                                    continue;
                                }
                                let Ok(state) = BigDataState::new(&ev).inspect_err(|e| {
                                    log::warn!("Faild to parse initial big data packet: {e}");
                                }) else {
                                    continue;
                                };
                                log::debug!("new big data: {state:?}");
                                if let BigDataState::Complete(packet) = state {
                                    match SleepData::try_from(packet) {
                                        Ok(p) => CommandReply::Sleep(p),
                                        Err(e) => {
                                            eprintln!("error converting big sleep: {e}");
                                            continue;
                                        }
                                    }
                                } else {
                                    partial_states.partial_big_data = Some(state);
                                    continue;
                                }
                            } else if let Some(mut state) = partial_states.partial_big_data.take() {
                                if let Err(e) = state.step(&ev) {
                                    log::warn!("failed to step big data state: {e}");
                                    continue;
                                }
                                log::debug!("updated big data: {state:?}");
                                if let BigDataState::Complete(packet) = state {
                                    match SleepData::try_from(packet) {
                                        Ok(p) => CommandReply::Sleep(p),
                                        Err(e) => {
                                            eprintln!("error converting big sleep: {e}");
                                            continue;
                                        }
                                    }
                                } else {
                                    partial_states.partial_big_data = Some(state);
                                    continue;
                                }
                            } else {
                                CommandReply::Unknown(ev)
                            }
                        }
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
    partial_big_data: Option<BigDataState>,
}

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
    pub start: OffsetDateTime,
    pub end: OffsetDateTime,
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
        let today = OffsetDateTime::now_utc().date();
        for i in 1..days {
            let days_ago = iter.next().ok_or_else(too_short_error(i, "days ago"))?;
            log::trace!("handling day {days_ago} days in the past");
            let day = today - Duration::days(days_ago as _);
            log::trace!("{day:?}");
            let day_bytes = iter.next().ok_or_else(too_short_error(i, "day bytes"))?;
            log::trace!("day bytes: {day_bytes}");
            let start = try_u16_from_iter(&mut iter).ok_or_else(too_short_error(i, "start"))?;
            let end = try_u16_from_iter(&mut iter).ok_or_else(too_short_error(i, "end"))?;
            let start = if start > end {
                day.midnight().assume_utc() + Duration::minutes(start as _)
            } else {
                day.previous_day()
                    .ok_or("Invalid day")?
                    .midnight()
                    .assume_utc()
                    + Duration::minutes(start as _)
            };
            let end = day.midnight().assume_utc() + Duration::minutes(end as _);
            log::debug!(
                "sleep session {:?}-{:?}",
                start.to_offset(UtcOffset::from_hms(-6, 0, 0).unwrap()),
                end.to_offset(UtcOffset::from_hms(-6, 0, 0).unwrap())
            );
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
    fn new(bytes: &[u8]) -> Result<Self> {
        if bytes[0] != crate::constants::CMD_BIG_DATA_V2 {
            return Err(format!("Invalid bytes for bigdata state: {bytes:?}").into());
        }
        println!("with bytes {}", bytes.len());
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
                panic!("Unknown big data type: {bytes:?}")
            },
        };
        ret.step(&bytes[6..])?;
        Ok(ret)
    }

    fn step(&mut self, bytes: &[u8]) -> Result {
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
    fn extend_from_slice(&mut self, slice: &[u8]) {
        self.get_data_mut().extend_from_slice(slice);
    }

    fn len(&self) -> usize {
        self.get_data_ref().len()
    }

    #[allow(unused)]
    fn is_empty(&self) -> bool {
        self.get_data_ref().is_empty()
    }

    #[allow(unused)]
    fn capacity(&self) -> usize {
        self.get_data_ref().capacity()
    }

    fn get_data_ref(&self) -> &Vec<u8> {
        match self {
            Self::Oxygen(data) | Self::Sleep(data) => data,
        }
    }

    fn get_data_mut(&mut self) -> &mut Vec<u8> {
        match self {
            Self::Oxygen(data) | Self::Sleep(data) => data,
        }
    }
}

fn try_u16_from_le_slice(slice: &[u8]) -> Option<u16> {
    let mut bytes = [0u8; 2];
    bytes.copy_from_slice(slice.get(0..2)?);
    Some(u16::from_le_bytes(bytes))
}

fn try_u16_from_iter(slice: &mut dyn Iterator<Item = u8>) -> Option<u16> {
    let mut bytes = [0u8; 2];
    bytes[0] = slice.next()?;
    bytes[1] = slice.next()?;
    Some(u16::from_le_bytes(bytes))
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
                }
                bytes.resize(16, 0);
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
    Sleep(SleepData),
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

impl RawPacket {
    pub fn tag_byte(&self) -> Result<u8> {
        match self {
            RawPacket::Uart(vec) | RawPacket::V2(vec) => vec.first(),
        }
        .copied()
        .ok_or_else(|| "0 sized packet".into())
    }

    pub fn len(&self) -> usize {
        match self {
            RawPacket::Uart(vec) | RawPacket::V2(vec) => vec.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        match self {
            RawPacket::Uart(vec) | RawPacket::V2(vec) => vec.is_empty(),
        }
    }
}

pub trait DurationExt {
    fn minutes(value: u64) -> Duration;
    fn hours(value: u64) -> Duration;
    fn days(value: u64) -> Duration;
}

impl DurationExt for Duration {
    fn minutes(value: u64) -> Duration {
        Duration::from_secs(value * 60)
    }

    fn hours(value: u64) -> Duration {
        Duration::minutes(value * 60)
    }

    fn days(value: u64) -> Duration {
        Duration::hours(value * 24)
    }
}

#[cfg(test)]
mod tests {

    use std::collections::VecDeque;

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
        let sleep_data: SleepData = packet.try_into().unwrap();
        insta::assert_debug_snapshot!(sleep_data);
    }

    #[tokio::test]
    async fn big_data_sleep2() {
        env_logger::builder().is_test(true).try_init().ok();
        let packet = vec![
            5u8, 6, 26, 177, 0, 11, 2, 2, 67, 3, 35, 2, 15, 4, 34, 2, 95, 3, 16, 2, 1, 5, 13, 2,
            49, 3, 18, 2, 3, 4, 40, 9, 0, 224, 1, 2, 61, 3, 31, 2, 15, 4, 33, 3, 31, 2, 31, 4, 34,
            3, 33, 2, 17, 4, 15, 2, 10, 0, 1, 2, 29, 5, 6, 2, 55, 5, 12, 2, 50, 2, 7, 3, 32, 0, 0,
            251, 1, 2, 73, 3, 18, 2, 18, 4, 31, 3, 33, 2, 31, 4, 33, 2, 16, 3, 18, 2, 15, 4, 17, 2,
            34, 3, 33, 2, 137, 2, 36, 159, 5, 4, 2, 2, 71, 3, 16, 2, 35, 4, 18, 3, 34, 2, 30, 4,
            33, 2, 101, 3, 32, 2, 17, 4, 15, 2, 32, 3, 18, 2, 29, 5, 13, 2, 23, 1, 12, 66, 0, 214,
            0, 2, 72, 3, 30, 2, 17, 4, 29,
        ];
        let sleep_data: SleepData = BigDataPacket::Sleep(packet).try_into().unwrap();
        insta::assert_debug_snapshot!(sleep_data);
    }

    fn make_packet(bytes: &[u8]) -> Vec<u8> {
        let mut ret = bytes.to_vec();
        ret.resize(16, 0);
        ret[15] = checksum(&ret);
        ret
    }
}
