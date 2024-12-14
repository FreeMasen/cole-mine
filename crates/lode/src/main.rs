use clap::{Parser, Subcommand};
use cole_mine::big_data::{OxygenMeasurement, SleepSession};
use cole_mine::client::Command;
use cole_mine::{incoming_messages::CommandReply, Client, DurationExt};

use cole_mine::BDAddr;
use std::convert::Infallible;
use std::future::Future;
use std::str::FromStr;
use std::time::Duration;
use time::macros::format_description;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Parser)]
enum Commands {
    /// Determine what BTLE adapters are available
    FindAdapters,
    /// Scan for devices.
    FindRings {
        /// If provided, all device addresses are printed to the terminal not just
        /// the first matching device by name prefix
        ///
        /// note: on MacOS addresses may be all zeros unless this is a signed .app
        #[arg(short = 'a', long = "all")]
        see_all: bool,
        /// If the scan should try and force devices to disconnect from a current
        /// connection
        #[arg(short = 'f', long = "force-disconnect")]
        force_disconnect: bool,
    },
    /// Read goals
    Goals { addr: BDAddr },
    /// Get the hardware and firmware information from a device
    DeviceDetails { id: DeviceIdentifier },
    #[clap(flatten)]
    SendCommand(SendCommand),
}

#[derive(Subcommand)]
enum SendCommand {
    Raw {
        id: DeviceIdentifier,
        // a hex encoded byte array with colons separating
        #[arg(short = 'c', long = "command")]
        commands: Vec<String>,
        // how long to wait for responses
        #[arg(short = 'l', long = "listen")]
        listen_seconds: Option<u64>,
    },
    Listen {
        id: DeviceIdentifier,
        // how long to wait for responses
        #[arg(short = 'l', long = "listen")]
        listen_seconds: Option<u64>,
    },
    /// Set the time
    ///
    /// optional minutes, hours, days, and years arguments adjust the current time
    SetTime {
        id: DeviceIdentifier,
        /// Minutes from now to add/remove
        #[arg(short = 'm', long = "minutes")]
        minutes: Option<isize>,
        /// Hours from now to add/remove
        #[arg(long = "hours")]
        hours: Option<isize>,
        /// Days from now to add/remove
        #[arg(short = 'd', long = "days")]
        days: Option<isize>,
        /// Years from now to add/remove
        #[arg(short = 'y', long = "years")]
        years: Option<isize>,
        /// Set the language to Chinese, defaults to English
        #[arg(short = 'c', long = "chinese")]
        chinese: bool,
    },
    ReadStress {
        id: DeviceIdentifier,
        #[arg(default_value_t = 0)]
        day_offset: u8,
    },
    ReadSportDetail {
        id: DeviceIdentifier,
        #[arg(default_value_t = 0)]
        day_offset: u8,
    },
    ReadHeartRate {
        id: DeviceIdentifier,
        #[arg(short = 'd', long = "date")]
        date: Option<String>,
    },
    ReadBatteryInfo {
        id: DeviceIdentifier,
    },
    GetHeartRateSettings {
        id: DeviceIdentifier,
    },
    SetHeartRateSettings {
        id: DeviceIdentifier,
        #[arg(short = 'e', long = "enable")]
        enabled: bool,
        #[arg(short = 'd', long = "disable")]
        disabled: bool,
        #[arg(short = 'i', long = "interval")]
        interval: Option<u8>,
    },
    Blink {
        id: DeviceIdentifier,
    },
    ReadSleep {
        id: DeviceIdentifier,
    },
    ReadOxygen {
        id: DeviceIdentifier,
    },
}

#[derive(Debug, Clone)]
enum DeviceIdentifier {
    Mac(BDAddr),
    Name(String),
}

impl FromStr for DeviceIdentifier {
    type Err = Infallible;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        if let Ok(addr) = BDAddr::from_str_delim(s) {
            return Ok(Self::Mac(addr));
        }
        if let Ok(addr) = BDAddr::from_str_no_delim(s) {
            return Ok(Self::Mac(addr));
        }
        Ok(Self::Name(s.to_string()))
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result {
    env_logger::init();
    if std::env::var("LODE_SET_UNSOUND_LOCAL_OFFSET")
        .map(|v| v == "1")
        .unwrap_or_default()
    {
        unsafe {
            time::util::local_offset::set_soundness(time::util::local_offset::Soundness::Unsound);
        }
    }
    match Commands::parse() {
        Commands::FindAdapters => find_adapters().await,
        Commands::FindRings {
            see_all,
            force_disconnect,
        } => find_rings(see_all, force_disconnect).await,
        Commands::Goals { addr } => read_goals(addr).await,
        Commands::DeviceDetails { id } => get_device_details(id).await,
        Commands::SendCommand(cmd) => send_command(cmd).await,
    }
}

async fn find_adapters() -> Result {
    use btleplug::api::{Central as _, Manager as _};
    use btleplug::platform::Manager;

    let manager = Manager::new().await?;
    let adapter_list = manager.adapters().await?;
    if adapter_list.is_empty() {
        println!("No Bluetooth adapters");
        return Ok(());
    }
    for (idx, adapter) in adapter_list.into_iter().enumerate() {
        let info = adapter.adapter_info().await?;
        let state = adapter.adapter_state().await?;
        println!("{idx}: {info} {state:?}");
    }
    Ok(())
}

async fn send_command(cmd: SendCommand) -> Result {
    match cmd {
        SendCommand::Raw {
            id,
            commands,
            listen_seconds,
        } => send_raw(id, commands, listen_seconds).await,
        SendCommand::ReadStress { id, day_offset } => read_stress(id, day_offset).await,
        SendCommand::Listen { id, listen_seconds } => connect_and_listen(id, listen_seconds).await,
        SendCommand::SetTime {
            id,
            minutes,
            hours,
            days,
            years,
            chinese,
        } => set_time(id, minutes, hours, days, years, chinese).await,
        SendCommand::ReadSportDetail { id, day_offset } => read_sport_details(id, day_offset).await,
        SendCommand::ReadHeartRate { id, date } => {
            let date = if let Some(date) = date {
                time::Date::parse(
                    &date,
                    time::macros::format_description!("[year]-[month]-[day]"),
                )?
            } else {
                OffsetDateTime::now_local()
                    .unwrap_or_else(|_| OffsetDateTime::now_utc())
                    .date()
            };
            read_heart_rate(id, date).await
        }
        SendCommand::ReadBatteryInfo { id } => read_battery_info(id).await,
        SendCommand::GetHeartRateSettings { id } => read_hr_config(id).await,
        SendCommand::SetHeartRateSettings {
            id,
            enabled,
            disabled,
            interval,
        } => write_hr_config(id, enabled, disabled, interval).await,
        SendCommand::Blink { id } => blink(id).await,
        SendCommand::ReadSleep { id } => read_sleep(id).await,
        SendCommand::ReadOxygen { id } => read_oxygen(id).await,
    }
}

async fn find_rings(see_all: bool, force_disconnect: bool) -> Result {
    use futures::StreamExt;
    log::info!("Finding rings");
    let mut stream = cole_mine::discover(see_all, force_disconnect).await?;
    while let Some(dev) = stream.next().await {
        print!("{}", dev.address());
        if let Some(name) = dev.local_name().await {
            print!(": {name}")
        }
        println!("");
    }
    Ok(())
}

async fn read_goals(addr: BDAddr) -> Result {
    log::info!("reading goals");
    let mut client = Client::new(addr).await?;
    client
        .send(Command::Raw(vec![
            0x21, 0x01, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ]))
        .await?;
    Ok(())
}

async fn set_time(
    id: DeviceIdentifier,
    minutes: Option<isize>,
    hours: Option<isize>,
    days: Option<isize>,
    years: Option<isize>,
    chinese: bool,
) -> Result {
    log::info!("setting time");
    const MINUTE: u64 = 60;
    const HOUR: u64 = MINUTE * 60;
    const DAY: u64 = HOUR * 24;
    let mut now = OffsetDateTime::now_local().unwrap_or_else(|_| OffsetDateTime::now_utc());
    if let Some(minutes) = minutes {
        let (dur, add) = get_duration(MINUTE, minutes);
        if add {
            now += dur;
        } else {
            now -= dur;
        }
    }
    if let Some(hours) = hours {
        let (dur, add) = get_duration(HOUR, hours);
        if add {
            now += dur;
        } else {
            now -= dur;
        }
    }
    if let Some(days) = days {
        let (dur, add) = get_duration(DAY, days);
        if add {
            now += dur;
        } else {
            now -= dur;
        }
    }
    if let Some(years) = years {
        let years = i32::try_from(years)?;
        let current_year = now.year();
        let target_year = current_year + years;
        now = now.replace_year(target_year)?;
    }
    if now.year() < 2000 {
        return Err(format!("Provided date offsets reached an unsupported date m: {minutes:?}, h: {hours:?}, d: {days:?}, y: {years:?}: {:?}", now.format(&Rfc3339)).into());
    }
    with_client(id, |mut client| async move {
        client
            .send(Command::SetTime {
                when: now,
                language: if chinese { 0 } else { 1 },
            })
            .await?;
        let _ = wait_for_reply(
            &mut client,
            |reply| matches!(reply, CommandReply::SetTime),
            "set time",
        )
        .await?;
        Ok(())
    })
    .await
}

async fn get_device_details(id: DeviceIdentifier) -> Result {
    with_client(id, |client| async move {
        log::info!("getting device details");
        let details = client.device_details().await?;
        println!(
            "Hardware: {}",
            details.hw.unwrap_or_else(|| "<not found>".to_string())
        );
        println!(
            "Firmware: {}",
            details.fw.unwrap_or_else(|| "<not found>".to_string())
        );
        Ok(())
    })
    .await
}

fn get_duration(mul: u64, unit: isize) -> (Duration, bool) {
    let add = unit > 0;
    let unit = unit.unsigned_abs() as u64;
    (Duration::from_secs(mul * unit), add)
}

async fn read_sport_details(id: DeviceIdentifier, day_offset: u8) -> Result {
    with_client(id, |mut client| async move {
        log::info!("getting sport details");
        client.send(Command::ReadSportDetail { day_offset }).await?;
        while let Ok(Ok(Some(event))) =
            tokio::time::timeout(std::time::Duration::from_secs(5), client.read_next()).await
        {
            if let CommandReply::SportDetail(details) = event {
                for detail in details {
                    println!(
                        "{}{:02}{:02}-{}",
                        detail.year, detail.month, detail.day, detail.time_index
                    );
                    println!("  Cals: {:>5.2}", detail.calories as f32 / 1000.0);
                    println!("  Stps: {:>8}", detail.steps);
                    let feet = detail.distance as f32 / 3.28084;
                    if feet > 5280.0 {
                        println!("  Dist: {:>8.2}mi", feet / 5280.0);
                    } else {
                        println!("  Dist: {:>8.2}ft", feet);
                    }
                }
            } else {
                eprintln!("Unexpected report from sport details: {event:?}");
            }
        }
        Ok(())
    })
    .await
}

async fn read_heart_rate(id: DeviceIdentifier, date: time::Date) -> Result {
    with_client(id, |mut client| async move {
        log::info!("getting heart rate");
        let target = date.midnight().assume_utc();
        let timestamp = target.unix_timestamp();
        client
            .send(Command::ReadHeartRate {
                timestamp: timestamp.try_into().unwrap(),
            })
            .await?;
        while let Some(CommandReply::HeartRate(hr)) = wait_for_reply(
            &mut client,
            |reply| matches!(reply, CommandReply::HeartRate(_)),
            "get heart rate info",
        )
        .await?
        {
            let time = if let Ok(now) = OffsetDateTime::now_local() {
                let local_offset = now.offset();
                target.replace_offset(local_offset)
            } else {
                target
            };
            println!(
                "Heart Rates {}-{:02}-{:02} {}",
                target.year(),
                target.month(),
                target.day(),
                hr.range
            );
            let mut minute = time;
            for rate in hr.rates {
                println!(
                    "  {:} {:>3}",
                    minute
                        .format(format_description!("[hour repr:12]:[minute] [period]"))
                        .unwrap(),
                    rate
                );
                minute += Duration::from_secs(60 * 5);
                if time.date() != minute.date() {
                    break;
                }
            }
        }
        Ok(())
    })
    .await
}

async fn read_battery_info(id: DeviceIdentifier) -> Result {
    with_client(id, |mut client| async move {
        log::info!("getting battery info");
        client.send(Command::BatteryInfo).await?;
        let Some(CommandReply::BatteryInfo { level, charging }) = wait_for_reply(
            &mut client,
            |reply| matches!(reply, CommandReply::BatteryInfo { .. }),
            "get battery info",
        )
        .await?
        else {
            return Err("no reply".into());
        };
        println!("{level}% {charging}");
        Ok(())
    })
    .await
}

async fn read_hr_config(id: DeviceIdentifier) -> Result {
    with_client(id, |mut client| async move {
        log::info!("getting hear rate config");
        let (enabled, interval) = get_current_config(&mut client).await?;
        println!("enabled: {enabled}, interval: {interval}");
        Ok(())
    })
    .await
}

async fn write_hr_config(
    id: DeviceIdentifier,
    set_enabled: bool,
    set_disabled: bool,
    set_interval: Option<u8>,
) -> Result {
    log::info!("setting heart rate config");
    with_client(id, |mut client| async move {
        let (mut enabled, mut interval) = get_current_config(&mut client).await?;
        if set_enabled {
            enabled = true;
        }
        if set_disabled {
            enabled = false;
        }
        if let Some(set_interval) = set_interval {
            interval = set_interval;
        }
        client
            .send(Command::SetHeartRateSettings { enabled, interval })
            .await?;
        let Some(CommandReply::HeartRateSettings { enabled, interval }) = wait_for_reply(
            &mut client,
            |reply| matches!(reply, CommandReply::HeartRateSettings { .. }),
            "set heart rate settings",
        )
        .await?
        else {
            unreachable!()
        };
        println!("Updated enabled: {enabled}, interval: {interval}");
        Ok(())
    })
    .await
}

async fn get_current_config(client: &mut Client) -> Result<(bool, u8)> {
    client.send(Command::GetHeartRateSettings).await?;
    if let Some(event) = wait_for_reply(
        client,
        |event| matches!(event, CommandReply::HeartRateSettings { .. }),
        "get heart rate settings",
    )
    .await?
    {
        let CommandReply::HeartRateSettings { enabled, interval } = event else {
            unreachable!()
        };
        return Ok((enabled, interval));
    }
    Err("Failed to read heart rate settings".into())
}

async fn wait_for_reply(
    client: &mut Client,
    matcher: impl Fn(&CommandReply) -> bool + 'static,
    name: &str,
) -> Result<Option<CommandReply>> {
    while let Ok(Ok(Some(event))) =
        tokio::time::timeout(Duration::from_secs(5), client.read_next()).await
    {
        if matcher(&event) {
            return Ok(Some(event));
        } else {
            eprintln!("Unexpected report from {name}: {event:?}");
        }
    }
    Ok(None)
}

async fn send_raw(
    id: DeviceIdentifier,
    commands: Vec<String>,
    listen_seconds: Option<u64>,
) -> Result {
    with_client(id, move |mut client| {
        let commands = commands.clone();
        async move {
            log::info!("sending raw packet");
            for command in commands
                .clone()
                .into_iter()
                .filter_map(|s| parse_raw_command(s.as_str()))
            {
                client.send(Command::Raw(command)).await?;
            }
            let listening_for = listen_seconds.unwrap_or(5);
            let to = Duration::from_secs(listening_for);
            tokio::time::timeout(to, async {
                while let Ok(Some(reply)) = client.read_next().await {
                    println!("{reply:?}");
                }
            })
            .await
            .ok();
            Ok(())
        }
    })
    .await
}

async fn connect_and_listen(id: DeviceIdentifier, listen_seconds: Option<u64>) -> Result {
    with_client(id, move |mut client| async move {
        let listening_for = listen_seconds.unwrap_or(120);
        let to = Duration::from_secs(listening_for);
        tokio::time::timeout(to, async {
            while let Ok(Some(reply)) = client.read_next().await {
                println!("{reply:?}");
            }
        })
        .await
        .ok();
        Ok(())
    })
    .await
}

fn parse_raw_command(s: &str) -> Option<Vec<u8>> {
    s.split(':')
        .map(|hex| Ok(u8::from_str_radix(hex, 16)?))
        .collect::<Result<Vec<u8>>>()
        .ok()
}

async fn blink(id: DeviceIdentifier) -> Result {
    with_client(id, |mut client| async move {
        log::info!("sending blink");
        client.send(Command::BlinkTwice).await?;
        let _ = wait_for_reply(
            &mut client,
            |reply| matches!(reply, CommandReply::BlinkTwice),
            "blink",
        )
        .await?;
        Ok(())
    })
    .await
}

async fn read_stress(id: DeviceIdentifier, mut day_offset: u8) -> Result {
    log::info!("getting stress details");
    with_client(id, |mut client| async move {
        let mut start = OffsetDateTime::now_local()
            .unwrap_or_else(|_| {
                log::warn!("Failed to get local time, falling back to UTC");
                OffsetDateTime::now_utc()
            })
            .date()
            .midnight();
        while day_offset > 0 {
            day_offset -= 1;
            start = start
                .date()
                .previous_day()
                .ok_or("time math....")?
                .midnight();
        }

        client.send(Command::ReadStress { day_offset }).await?;
        let Some(CommandReply::Stress {
            time_interval_sec,
            measurements,
        }) = wait_for_reply(
            &mut client,
            |r| matches!(r, CommandReply::Stress { .. }),
            "stress",
        )
        .await?
        else {
            return Err("Failed to get stress response".into());
        };
        let minutes_in_a_day = 24 * 60;
        let segments = time_interval_sec as u32 / minutes_in_a_day;
        for i in 0..segments as u64 {
            let time = start + Duration::from_secs(time_interval_sec as u64 * i);
            println!(
                "{}: {}",
                time.format(&time::format_description::well_known::Rfc3339)
                    .unwrap(),
                &measurements[i as usize]
            )
        }
        Ok(())
    })
    .await
}

async fn read_sleep(id: DeviceIdentifier) -> Result {
    with_client(id, |mut client| async move {
        client.send(Command::SyncSleep).await?;
        while let Some(packet) = client.read_next().await? {
            if let CommandReply::Sleep(sleep_data) = packet {
                for session in sleep_data.sessions {
                    report_sleep_session(session)?;
                }
                break;
            }
        }
        Ok(())
    })
    .await
}

async fn read_oxygen(id: DeviceIdentifier) -> Result {
    with_client(id, |mut client| async move {
        client.send(Command::SyncOxygen).await?;
        while let Some(packet) = client.read_next().await? {
            if let CommandReply::Oxygen(oxy) = packet {
                for sample in oxy.samples {
                    report_oxygen_info(sample);
                }
                break;
            }
        }
        Ok(())
    })
    .await
}

fn report_sleep_session(session: SleepSession) -> Result {
    let mut time = session.start;
    println!(
        "--{}--",
        time.date()
            .format(&time::macros::format_description!("[year]-[month]-[day]"))?
    );
    let fmt =
        time::macros::format_description!("[year]-[month]-[day] [hour repr:12]:[minute] [period]");
    for stage in session.stages {
        let (n, m) = match stage {
            cole_mine::SleepStage::Light(m) => ("Light", m as u64),
            cole_mine::SleepStage::Deep(m) => ("Deep", m as u64),
            cole_mine::SleepStage::Rem(m) => ("REM", m as u64),
            cole_mine::SleepStage::Awake(m) => ("Awake", m as u64),
        };
        let end = time + Duration::minutes(m);
        println!("{}-{} ({m}): {n}", time.format(fmt)?, end.format(fmt)?,);
        time = end;
    }
    Ok(())
}

fn report_oxygen_info(oxy: OxygenMeasurement) {
    if oxy.min == 0 && oxy.max == 0 {
        return;
    }
    print!(
        "{}:",
        oxy.when
            .format(time::macros::format_description!(
                "[year]-[month]-[day] [hour repr:12]:[minute] [period]"
            ))
            .unwrap()
    );
    if oxy.max == 0 || oxy.min == 0 {
        let v = oxy.max.max(oxy.min);
        print!("{v:>7} ±  0 ~{:.02}", v as f32);
    } else {
        print!(
            "{:>3}-{:<3} ±{:>3} ~{:.02}",
            oxy.min,
            oxy.max,
            oxy.min.max(oxy.max) - oxy.min.min(oxy.max),
            (oxy.min + oxy.max) as f32 / 2.0,
        );
    }
    println!("")
}

async fn with_client<'a, F, G>(id: DeviceIdentifier, cb: F) -> Result
where
    F: Fn(Client) -> G + 'a,
    G: Future<Output = Result> + 'a,
{
    log::trace!("Getting client for id: {id:?}");
    let mut client = get_client(id).await?;
    log::trace!("Connecting client");
    client.connect().await?;
    log::debug!("client connected");
    let device = client.device.clone();
    let ret = tokio::select! {
        ret = cb(client) => {
            ret
        }
        _ = tokio::signal::ctrl_c() => {
            Ok(())
        }
    };
    log::trace!("disconnecting client");
    device.disconnect().await?;
    log::trace!("operation success: {}", ret.is_ok());
    ret
}

async fn get_client(id: DeviceIdentifier) -> Result<Client> {
    match id {
        DeviceIdentifier::Mac(mac) => Client::new(mac).await,
        DeviceIdentifier::Name(name) => {
            let dev = find_device_by_name(&name).await?;
            Client::with_device(dev).await
        }
    }
}

async fn find_device_by_name(name: &str) -> Result<bleasy::Device> {
    use futures::StreamExt;

    let mut stream = cole_mine::discover_by_name(name.to_string()).await?;
    while let Some(dev) = stream.next().await {
        let Some(n) = dev.local_name().await else {
            continue;
        };
        if n == name {
            return Ok(dev);
        }
    }
    Err("Unable to find device by name".into())
}
