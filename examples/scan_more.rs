use cole_mine::discover;
use std::{
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};
use tokio::time::timeout;

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
    eprintln!("performing discovery");
    for dev in discover(true).await.unwrap() {
        eprintln!("found device {}", dev.address());
        let name = dev.local_name().await.unwrap_or_else(|| {
            unknown_ct += 1;
            format!("dev-{unknown_ct}")
        });
        eprintln!("  with name: {name}");
        let rssi = dev.rssi().await.unwrap_or_default();
        eprintln!("  with rssi: {rssi}");
        let service_count =
            if let Ok(Ok(srv_ct)) = timeout(Duration::from_secs(1), dev.service_count()).await {
                srv_ct
            } else {
                0
            };
        eprintln!("  with srv_ct: {service_count}");
        let characteristics: BTreeSet<String> =
            timeout(Duration::from_secs(1), dev.characteristics())
                .await
                .unwrap_or_else(|_| Ok(Vec::new()))
                .unwrap_or_default()
                .into_iter()
                .map(|c| c.uuid().to_string())
                .collect();
        eprintln!("  with char_ct: {}", characteristics.len());
        let mut service_names = BTreeMap::new();
        if let Ok(Ok(services)) = timeout(Duration::from_secs(1), dev.services()).await {
            for s in services {
                let key = format!("{}", s.uuid().as_simple());
                let value = s
                    .characteristics()
                    .into_iter()
                    .map(|c| c.uuid().to_string())
                    .collect();
                service_names.insert(key.to_string(), value);
            }
        }

        devices.insert(
            name,
            Device {
                rssi,
                addr: dev.address().to_string(),
                characteristics,
                service_count,
                service_names,
            },
        );
    }
    println!("{:#?}", devices);
}
