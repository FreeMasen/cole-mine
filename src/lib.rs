use bleasy::{Device, ScanConfig};
use futures::{Stream, StreamExt};
use std::{pin::Pin, time::Duration};

type Result<T = (), E = Box<dyn std::error::Error>> = std::result::Result<T, E>;

pub mod client;
mod constants;
pub mod incoming_messages;
mod util;

pub use crate::{
    client::Client,
    incoming_messages::{
        big_data::{self, SleepStage},
        heart_rate, sport_detail, stress,
    },
    util::DurationExt,
};

pub use bleasy::BDAddr;

pub async fn discover(all: bool) -> Result<Pin<Box<dyn Stream<Item = Device>>>> {
    log::trace!("discover({all})");
    let mut config = ScanConfig::default();
    if !all {
        config = config.filter_by_name(|n| {
            crate::constants::DEVICE_NAME_PREFIXES
                .iter()
                .any(|p| n.starts_with(*p))
        });
    }
    discover_(config).await
}

pub async fn discover_by_name(name: String) -> Result<Pin<Box<dyn Stream<Item = Device>>>> {
    let config = ScanConfig::default()
        .filter_by_name(move |n| n == name)
        .force_disconnect(true);
    discover_(config).await
}

async fn discover_(mut config: ScanConfig) -> Result<Pin<Box<dyn Stream<Item = Device>>>> {
    let mut scanner = bleasy::Scanner::new();
    if let Some(max_op_secs) = std::env::var("COLE_MINE_MAX_TIMEOUT_SECS")
        .ok()
        .and_then(|a| a.parse::<u64>().ok())
    {
        log::debug!("Scanning for {max_op_secs} seconds");
        config = config.stop_after_timeout(Duration::from_secs(max_op_secs))
    }
    log::trace!("starting scan");
    scanner.start(config).await?;
    Ok(async_stream::stream! {
        let mut stream = scanner.device_stream();
        while let Some(dev) = stream.next().await {
            log::debug!("Stream returned device");
            yield dev;
        }
    }
    .boxed_local())
}
