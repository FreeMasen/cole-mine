use bleasy::{Device, ScanConfig};
use futures::{Stream, StreamExt};
use std::{pin::Pin, time::Duration};
use uuid::Uuid;

type Result<T = (), E = Box<dyn std::error::Error>> = std::result::Result<T, E>;

pub mod client;
pub mod heart_rate;
pub mod sport_detail;

pub use bleasy::BDAddr;

pub(crate) const UART_SERVICE_UUID: Uuid = uuid::uuid!("6E40FFF0-B5A3-F393-E0A9-E50E24DCCA9E");
pub(crate) const UART_RX_CHAR_UUID: Uuid = uuid::uuid!("6E400002-B5A3-F393-E0A9-E50E24DCCA9E");
pub(crate) const UART_TX_CHAR_UUID: Uuid = uuid::uuid!("6E400003-B5A3-F393-E0A9-E50E24DCCA9E");
pub(crate) const DEVICE_INFO_UUID: Uuid = uuid::uuid!("0000180A-0000-1000-8000-00805F9B34FB");
pub(crate) const DEVICE_HW_UUID: Uuid = uuid::uuid!("00002A27-0000-1000-8000-00805F9B34FB");
pub(crate) const DEVICE_FW_UUID: Uuid = uuid::uuid!("00002A26-0000-1000-8000-00805F9B34FB");
pub(crate) const DEVICE_NAME_PREFIXES: &[&str] = &[
    "R01",
    "R02",
    "R03",
    "R04",
    "R05",
    "R06",
    "R07",
    "R10", // maybe compatible?
    "VK-5098",
    "MERLIN",
    "Hello Ring",
    "RING1",
    "boAtring",
    "TR-R02",
    "SE",
    "EVOLVEO",
    "GL-SR2",
    "Blaupunkt",
    "KSIX RING",
];

pub async fn discover(all: bool) -> Result<Pin<Box<dyn Stream<Item = Device>>>> {
    log::trace!("discover({all})");
    let mut scanner = bleasy::Scanner::new();
    let mut config = ScanConfig::default();
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
            if all {
                yield dev;
            } else if let Some(name) = dev.local_name().await {
                if DEVICE_NAME_PREFIXES
                .iter()
                .any(|pre| name.trim().starts_with(*pre)) {
                    yield dev;
                } else {
                    log::debug!("Skipping device for name mismatch `{name}`");
                }
            } else {
                log::debug!("Skipping device with no local name {}", dev.address());
            }
        }
    }
    .boxed_local())
}
