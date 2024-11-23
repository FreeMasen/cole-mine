use clap::{Parser, Subcommand};
use cole_mine::{
    client::Client,
    client::{Command, CommandReply},
    BDAddr,
};
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
                read_heart_rate(name, OffsetDateTime::now_utc().date()).await
            }
            #[cfg(not(target_os = "macos"))]
            {
                read_heart_rate(addr, OffsetDateTime::now_utc().date()).await
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
    while let Ok(Ok(Some(event))) = tokio::time::timeout(std::time::Duration::from_secs(5), client.read_next()).await {
        if !matches!(event, CommandReply::SetTime) {
            eprintln!("Unexpected report from set time: {event:?}");
            continue;
        }
        break;
    }
    Ok(())
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
    print!(
        "Hardware:{}",
        details.hw.unwrap_or_else(|| "<not found>".to_string())
    );
    print!(
        "Firmware:{}",
        details.fw.unwrap_or_else(|| "<not found>".to_string())
    );
    Ok(())
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
    while let Ok(Ok(Some(event))) = tokio::time::timeout(std::time::Duration::from_secs(5), client.read_next()).await {
        if let CommandReply::SportDetail(details) = event {
            for detail in details {
                println!(
                    "{}{:02}{:02}-{}",
                    detail.year, detail.month, detail.day, detail.time_index
                );
                println!("  Cals: {:>8}", detail.calories);
                println!("  Stps: {:>8}", detail.steps);
                println!("  Dist: {:>8}", detail.distance);
            }
        } else {
            eprintln!("Unexpected report from sport details: {event:?}");
        }
    }
    Ok(())
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
    while let Ok(Ok(Some(event))) = tokio::time::timeout(std::time::Duration::from_secs(5), client.read_next()).await {
        if let CommandReply::HeartRate(hr) = event {
            let time = target;
            println!(
                "Heart Rates {}-{:02}-{:02} {}",
                target.year(),
                target.month(),
                target.day(),
                hr.range
            );
            for rate in hr.rates {
                println!("  {:02}:{:02} {:>3}", time.hour(), time.minute(), rate);
            }
        } else {
            eprintln!("Unexpected report from heart rate: {event:?}");
        }
    }
    Ok(())
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
    while let Ok(Some(event)) = client.read_next().await {
        if let CommandReply::BatteryInfo { level, charging } = event {
            println!("{level}% {charging}");
        } else {
            eprintln!("Unexpected report from battery info: {event:?}");
        }
        break;
    }
    Ok(())
}
