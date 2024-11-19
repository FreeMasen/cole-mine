use cole_mine::discover;
use futures::StreamExt;
use std::{
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};
use tokio::time::timeout;

#[tokio::main]
async fn main() {
    let max_op_secs = std::env::var("COLE_MIN_MAX_TIMEOUT_SECS")
        .ok()
        .and_then(|a| a.parse::<u64>().ok())
        .unwrap_or(5);
    let mut unknown_ct = 0;
    let mut stream = discover(true).await.unwrap();
    while let Some(dev) = stream.next().await {
        let name = dev.local_name().await.unwrap_or_else(|| {
            unknown_ct += 1;
            format!("dev-{unknown_ct}")
        });
        let rssi = dev.rssi().await.unwrap_or_default();
        let service_count = if let Ok(Ok(srv_ct)) =
            timeout(Duration::from_secs(max_op_secs), dev.service_count()).await
        {
            srv_ct
        } else {
            0
        };
        let characteristics: BTreeSet<String> =
            timeout(Duration::from_secs(1), dev.characteristics())
                .await
                .unwrap_or_else(|_| Ok(Vec::new()))
                .unwrap_or_default()
                .into_iter()
                .map(|c| c.uuid().to_string())
                .collect();

        let mut srvs = BTreeMap::new();
        if let Ok(Ok(services)) = timeout(Duration::from_secs(max_op_secs), dev.services()).await {
            // println!("  srvs:");
            for s in services {
                let key = s.uuid().as_simple().to_string();
                let value: BTreeSet<String> = s
                    .characteristics()
                    .into_iter()
                    .map(|c| c.uuid().as_simple().to_string())
                    .collect();
                srvs.insert(key, value);
            }
        }
        if !characteristics.is_empty()
            || (!srvs.is_empty()
            && srvs.iter().any(|(_, cs)| !cs.is_empty()))
        {
            println!("found device {}", dev.address());
            println!("  name: {name}");
            println!("  rssi: {rssi}");
            println!("  char_ct: {}", characteristics.len());
            if !characteristics.is_empty() {
                println!("  chars:");
            }
            for ch in characteristics {
                println!("    {ch}");
            }
            println!("  srv_ct: {service_count}");
            if !srvs.is_empty() {
                println!("  srvs:");
                for (id, charas) in &srvs {
                    println!("    srv: {id}");
                    for ch in charas.iter() {
                        println!("      ch: {ch}");
                    }
                }
            }
        }
    }
}
