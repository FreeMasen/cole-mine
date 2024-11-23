use clap::{Parser, Subcommand};
use cole_mine::{
    client::Client,
    client::{Command, CommandReply},
};

#[cfg(not(target_os = "macos"))]
use cole_mine::BDAddr;
use std::time::Duration;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Parser)]
enum Commands {
    /// Scan for devices.
    FindRings {
        /// If provided, all device addresses are printed to the terminal not just
        /// the first matching device by name prefix
        ///
        /// note: on MacOS addresses may be all zeros unless this is a signed .app
        #[arg(short = 'a', long = "all")]
        see_all: bool,
    },
    /// Get the hardware and firmware information from a device
    DeviceDetails {
        #[cfg(target_os = "macos")]
        name: String,
        #[cfg(not(target_os = "macos"))]
        address: BDAddr,
    },
    #[clap(flatten)]
    SendCommand(SendCommand),
}

#[derive(Subcommand)]
enum SendCommand {
    Listen {
        #[cfg(target_os = "macos")]
        name: String,
        #[cfg(not(target_os = "macos"))]
        address: BDAddr,
    },
    /// Set the time
    ///
    /// optional minutes, hours, days, and years arguments adjust the current time
    SetTime {
        #[cfg(target_os = "macos")]
        name: String,
        #[cfg(not(target_os = "macos"))]
        address: BDAddr,
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
    ReadSportDetail {
        #[cfg(target_os = "macos")]
        name: String,
        #[cfg(not(target_os = "macos"))]
        addr: BDAddr,
        #[arg(default_value_t = 0)]
        day_offset: u8,
    },
    ReadHeartRate {
        #[cfg(target_os = "macos")]
        name: String,
        #[cfg(not(target_os = "macos"))]
        addr: BDAddr,
    },
    ReadBatteryInfo {
        #[cfg(target_os = "macos")]
        name: String,
        #[cfg(not(target_os = "macos"))]
        addr: BDAddr,
    },
    GetHeartRateSettings {
        #[cfg(target_os = "macos")]
        name: String,
        #[cfg(not(target_os = "macos"))]
        addr: BDAddr,
    },
    SetHeartRateSettings {
        #[cfg(target_os = "macos")]
        name: String,
        #[cfg(not(target_os = "macos"))]
        addr: BDAddr,
        #[arg(short = 'e', long = "enable")]
        enabled: bool,
        #[arg(short = 'd', long = "disable")]
        disabled: bool,
        #[arg(short = 'i', long = "interval")]
        interval: Option<u8>,
    },
}

#[tokio::main]
async fn main() -> Result {
    env_logger::init();
    match Commands::parse() {
        Commands::FindRings { see_all } => find_rings(see_all).await,
        Commands::DeviceDetails {
            #[cfg(target_os = "macos")]
            name,
            #[cfg(not(target_os = "macos"))]
            address,
        } => {
            #[cfg(target_os = "macos")]
            {
                get_device_details(name).await
            }
            #[cfg(not(target_os = "macos"))]
            {
                get_device_details(address).await
            }
        }
        Commands::SendCommand(cmd) => send_command(cmd).await,
    }
}

async fn send_command(cmd: SendCommand) -> Result {
    match cmd {
        SendCommand::Listen {
            #[cfg(target_os = "macos")]
            name,
            #[cfg(not(target_os = "macos"))]
            address,
        } => {
            #[cfg(target_os = "macos")]
            {
                listen(name).await
            }
            #[cfg(not(target_os = "macos"))]
            {
                listen(address).await
            }
        },
        SendCommand::SetTime {
            #[cfg(target_os = "macos")]
            name,
            #[cfg(not(target_os = "macos"))]
            address,
            minutes,
            hours,
            days,
            years,
            chinese,
        } => {
            #[cfg(target_os = "macos")]
            {
                set_time(name, minutes, hours, days, years, chinese).await
            }
            #[cfg(not(target_os = "macos"))]
            {
                set_time(address, minutes, hours, days, years, chinese).await
            }
        }
        SendCommand::ReadSportDetail {
            #[cfg(target_os = "macos")]
            name,
            #[cfg(not(target_os = "macos"))]
            addr,
            day_offset,
        } => {
            #[cfg(target_os = "macos")]
            {
                read_sport_details(name, day_offset).await
            }
            #[cfg(not(target_os = "macos"))]
            {
                read_sport_details(addr, day_offset).await
            }
        }
        SendCommand::ReadHeartRate {
            #[cfg(target_os = "macos")]
            name,
            #[cfg(not(target_os = "macos"))]
            addr,
        } => {
            #[cfg(target_os = "macos")]
            {
                read_heart_rate(
                    name,
                    OffsetDateTime::now_utc().date().previous_day().unwrap(),
                )
                .await
            }
            #[cfg(not(target_os = "macos"))]
            {
                read_heart_rate(
                    addr,
                    OffsetDateTime::now_utc().date().previous_day().unwrap(),
                )
                .await
            }
        }
        SendCommand::ReadBatteryInfo {
            #[cfg(target_os = "macos")]
            name,
            #[cfg(not(target_os = "macos"))]
            addr,
        } => {
            #[cfg(target_os = "macos")]
            {
                read_battery_info(name).await
            }
            #[cfg(not(target_os = "macos"))]
            {
                read_battery_info(addr).await
            }
        }
        SendCommand::GetHeartRateSettings {
            #[cfg(target_os = "macos")]
            name,
            #[cfg(not(target_os = "macos"))]
            addr,
        } => {
            #[cfg(target_os = "macos")]
            {
                read_hr_config(name).await
            }
            #[cfg(not(target_os = "macos"))]
            {
                read_hr_config(addr).await
            }
        }
        SendCommand::SetHeartRateSettings {
            #[cfg(target_os = "macos")]
            name,
            #[cfg(not(target_os = "macos"))]
            addr,
            enabled,
            disabled,
            interval,
        } => {
            #[cfg(target_os = "macos")]
            {
                write_hr_config(name, enabled, disabled, interval).await
            }
            #[cfg(not(target_os = "macos"))]
            {
                write_hr_config(addr, enabled, disabled, interval).await
            }
        }
    }
}

async fn find_rings(see_all: bool) -> Result {
    use futures::StreamExt;
    let mut stream = cole_mine::discover(see_all).await?;
    while let Some(dev) = stream.next().await {
        println!("{}", dev.address());
    }
    Ok(())
}

#[cfg(target_os = "macos")]
async fn set_time(
    name: String,
    minutes: Option<isize>,
    hours: Option<isize>,
    days: Option<isize>,
    years: Option<isize>,
    chinese: bool,
) -> Result {
    let dev = find_device_by_name(&name).await?;
    let mut client = Client::with_device(dev).await?;
    set_time_(&mut client, minutes, hours, days, years, chinese).await
}
#[cfg(not(target_os = "macos"))]
async fn set_time(
    addr: BDAddr,
    minutes: Option<isize>,
    hours: Option<isize>,
    days: Option<isize>,
    years: Option<isize>,
    chinese: bool,
) -> Result {
    let mut client = Client::new(addr).await?;
    set_time_(&mut client, minutes, hours, days, years, chinese).await
}

async fn set_time_(
    client: &mut Client,
    minutes: Option<isize>,
    hours: Option<isize>,
    days: Option<isize>,
    years: Option<isize>,
    chinese: bool,
) -> Result {
    const MINUTE: u64 = 60;
    const HOUR: u64 = MINUTE * 60;
    const DAY: u64 = HOUR * 24;
    let mut now = OffsetDateTime::now_utc();
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
    client.connect().await?;
    client
        .send(Command::SetTime {
            when: now,
            language: if chinese { 0 } else { 1 },
        })
        .await?;
    let _ = wait_for_reply(client, |reply| matches!(reply, CommandReply::SetTime), "set time").await?;
    client.disconnect().await
}

#[cfg(target_os = "macos")]
async fn get_device_details(name: String) -> Result {
    let dev = find_device_by_name(&name).await?;
    let mut client = Client::with_device(dev).await?;
    get_device_details_(&mut client).await
}

#[cfg(not(target_os = "macos"))]
async fn get_device_details(addr: BDAddr) -> Result {
    let mut client = Client::new(addr).await?;
    get_device_details_(&mut client).await
}

async fn get_device_details_(client: &mut Client) -> Result {
    let details = client.device_details().await?;
    println!(
        "Hardware: {}",
        details.hw.unwrap_or_else(|| "<not found>".to_string())
    );
    println!(
        "Firmware: {}",
        details.fw.unwrap_or_else(|| "<not found>".to_string())
    );
    client.disconnect().await
}

fn get_duration(mul: u64, unit: isize) -> (Duration, bool) {
    let add = unit > 0;
    let unit = unit.unsigned_abs() as u64;
    (Duration::from_secs(mul * unit), add)
}

#[cfg(target_os = "macos")]
async fn read_sport_details(name: String, day_offset: u8) -> Result {
    let dev = find_device_by_name(&name).await?;
    let mut client = Client::with_device(dev).await?;
    read_sport_details_(&mut client, day_offset).await
}

#[cfg(not(target_os = "macos"))]
async fn read_sport_details(addr: BDAddr, day_offset: u8) -> Result {
    let mut client = Client::new(addr).await?;
    read_sport_details_(&mut client, day_offset).await
}
async fn read_sport_details_(client: &mut Client, day_offset: u8) -> Result {
    client.connect().await?;
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
    client.disconnect().await
}

#[cfg(target_os = "macos")]
async fn read_heart_rate(name: String, date: time::Date) -> Result {
    let device = find_device_by_name(&name).await?;
    let mut client = Client::with_device(device).await?;
    read_heart_rate_(&mut client, date).await
}

#[cfg(not(target_os = "macos"))]
async fn read_heart_rate(addr: BDAddr, date: time::Date) -> Result {
    let mut client = Client::new(addr).await?;
    read_heart_rate_(&mut client, date).await
}
async fn read_heart_rate_(client: &mut Client, date: time::Date) -> Result {
    let target = date.midnight().assume_utc();
    let timestamp = target.unix_timestamp();
    client.connect().await?;
    client
        .send(Command::ReadHeartRate {
            timestamp: timestamp.try_into().unwrap(),
        })
        .await?;
    while let Some(CommandReply::HeartRate(hr)) = wait_for_reply(
        client,
        |reply| matches!(reply, CommandReply::HeartRate(_)),
        "get heart rate info",
    )
    .await?
    {
        let mut time = target;
        println!(
            "Heart Rates {}-{:02}-{:02} {}",
            target.year(),
            target.month(),
            target.day(),
            hr.range
        );
        for rate in hr.rates {
            println!("  {:02}:{:02} {:>3}", time.hour(), time.minute(), rate);
            time += Duration::from_secs(60 * 5);
        }
    }
    client.disconnect().await
}

#[cfg(not(target_os = "macos"))]
async fn read_battery_info(addr: BDAddr) -> Result {
    let mut client = Client::new(addr).await?;
    read_battery_info_(&mut client).await
}

#[cfg(target_os = "macos")]
async fn read_battery_info(name: String) -> Result {
    let dev = find_device_by_name(&name).await?;
    let mut client = Client::with_device(dev).await?;
    read_battery_info_(&mut client).await
}

#[cfg(target_os = "macos")]
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
    Err("Undable to find device by name".to_string().into())
}

async fn read_battery_info_(client: &mut Client) -> Result {
    client.connect().await?;
    client.send(Command::BatteryInfo).await?;
    let Some(CommandReply::BatteryInfo { level, charging }) = wait_for_reply(
        client,
        |reply| matches!(reply, CommandReply::BatteryInfo { .. }),
        "get battery info",
    )
    .await?
    else {
        return Err("no reply".into());
    };
    println!("{level}% {charging}");
    client.disconnect().await
}

#[cfg(target_os = "macos")]
async fn read_hr_config(name: String) -> Result {
    let dev = find_device_by_name(&name).await?;
    let mut client = Client::with_device(dev).await?;
    read_hr_config_(&mut client).await
}

#[cfg(not(target_os = "macos"))]
async fn read_hr_config(name: BDAddr) -> Result {
    let mut client = Client::new(name).await?;
    read_hr_config_(&mut client).await
}

async fn read_hr_config_(client: &mut Client) -> Result {
    let (enabled, interval) = get_current_config(client).await?;
    println!("enabled: {enabled}, interval: {interval}");
    client.disconnect().await
}

#[cfg(target_os = "macos")]
async fn write_hr_config(
    name: String,
    enabled: bool,
    disabled: bool,
    interval: Option<u8>,
) -> Result {
    let dev = find_device_by_name(&name).await?;
    let mut client = Client::with_device(dev).await?;
    write_hr_config_(&mut client, enabled, disabled, interval).await
}

#[cfg(not(target_os = "macos"))]
async fn write_hr_config(
    name: BDAddr,
    enabled: bool,
    disabled: bool,
    interval: Option<u8>,
) -> Result {
    let mut client = Client::new(name).await?;
    write_hr_config_(&mut client, enabled, disabled, interval).await
}

async fn write_hr_config_(
    client: &mut Client,
    set_enabled: bool,
    set_disabled: bool,
    set_interval: Option<u8>,
) -> Result {
    let (mut enabled, mut interval) = get_current_config(client).await?;
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
        client,
        |reply| matches!(reply, CommandReply::HeartRateSettings { .. }),
        "set heart rate settings",
    )
    .await?
    else {
        client.disconnect().await?;
        unreachable!()
    };
    println!("Updated enabled: {enabled}, interval: {interval}");
    client.disconnect().await
}

async fn get_current_config(client: &mut Client) -> Result<(bool, u8)> {
    client.connect().await?;
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

#[cfg(target_os = "macos")]
async fn listen(name: String) -> Result {
    let dev = find_device_by_name(&name).await?;
    let mut client = Client::with_device(dev).await?;
    let ret = listen_(&mut client).await;
    client.disconnect().await?;
    ret
}
#[cfg(not(target_os = "macos"))]
async fn listen(addr: BDAddr) -> Result {
    let mut client = Client::new(addr).await?;
    let ret = listen_(&mut client).await;
    client.disconnect().await?;
    ret
}

async fn listen_(client: &mut Client) -> Result {
    client.connect().await?;
    while let Ok(Ok(Some(event))) =
        tokio::time::timeout(Duration::from_secs(5), client.read_next()).await
    {
        println!("[{}]: {event:?}", time::OffsetDateTime::now_utc().format(&time::format_description::well_known::Rfc3339).unwrap());
    }
    Ok(())
}
