use std::{collections::{BTreeMap, BTreeSet}, time::Duration};
use tokio::time::timeout;
use cole_mine::discover;

#[derive(Debug)]
struct Device {
    addr: String,
    rssi: i16,
    service_count: usize,
    service_names: BTreeMap<String, BTreeSet<String>>,
    characteristics: BTreeSet<String>,
}

#[tokio::main]
async fn main() {
    let mut unknown_ct = 0;
    let mut devices = BTreeMap::new();
    for dev in discover(true).await.unwrap() {
        let name = dev.local_name().await.unwrap_or_else(|| {
            unknown_ct += 1;
            format!("dev-{unknown_ct}")
        });
        let rssi = dev.rssi().await.unwrap_or_default();
        let service_count = if let Ok(Ok(srv_ct)) = timeout(Duration::from_secs(3), dev.service_count()).await {
            srv_ct
        } else {
            0
        };
        let characteristics: BTreeSet<String> = dev.characteristics().await.unwrap_or_default().into_iter().map(|c| c.uuid().to_string()).collect();
        let mut service_names = BTreeMap::new();
        if let Ok(Ok(services)) = timeout(Duration::from_secs(3), dev.services()).await {
            for s in services {
                let key = s.uuid();
                let value = s.characteristics().into_iter().map(|c| c.uuid().to_string()).collect();
                service_names.insert(key.to_string(), value);
            }
        }

        devices.insert(name, Device {
            rssi,
            addr: dev.address().to_string(),
            characteristics,
            service_count,
            service_names,
        });
    }
    println!(
        "{:#?}",
        devices
    );
}
