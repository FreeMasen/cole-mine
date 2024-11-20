use bleasy::Device;
use cole_mine::discover;
use futures::StreamExt;
use uuid::Uuid;
use std::{
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};
use tokio::time::timeout;

const MANU: Uuid = uuid::uuid!("00002a29-0000-1000-8000-00805f9b34fb");
const MODEL: Uuid = uuid::uuid!("00002a24-0000-1000-8000-00805f9b34fb");
const DEV_INF: Uuid = uuid::uuid!("0000180a-0000-1000-8000-00805f9b34fb");

#[tokio::main]
async fn main() {
    env_logger::init();
    let max_op_secs = std::env::var("COLE_MINE_SCAN_MORE_TIMEOUT_SECS")
        .ok()
        .and_then(|a| a.parse::<u64>().ok())
        .unwrap_or(5);
    let mut stream = discover(true).await.unwrap();
    while let Some(dev) = stream.next().await {
        log::trace!("looking up local name");
        let name = dev.local_name().await;
        log::trace!("looking up rssi");
        let rssi = dev.rssi().await.unwrap_or_default();
        log::trace!("looking up service count");
        let service_count = if let Ok(Ok(srv_ct)) =
            timeout(Duration::from_secs(max_op_secs), dev.service_count()).await
        {
            srv_ct
        } else {
            0
        };
        log::trace!("looking up characteristics");
        let characteristics: BTreeSet<String> =
            timeout(Duration::from_secs(max_op_secs), dev.characteristics())
                .await
                .unwrap_or_else(|_| {
                    log::debug!("timed out looking up characteristics");
                    Ok(Vec::new())
                })
                .unwrap_or_default()
                .into_iter()
                .map(|c| c.uuid().to_string())
                .collect();
        log::trace!("looking up manu/model");
        let (mut manu, mut model) = timeout(Duration::from_secs(max_op_secs), manu_model(&dev)).await.unwrap_or_else(|_| {
            log::debug!("timed out looking for manu/model");
            (None, None)
        });
        let mut srvs = BTreeMap::new();
        log::trace!("looking up services");
        if let Ok(Ok(services)) = timeout(Duration::from_secs(max_op_secs), dev.services()).await.inspect_err(|_| {
            log::debug!("timed out looking for services");
        }) {
            for s in services {
                let chars = s.characteristics();
                if s.uuid() == DEV_INF {
                    for ch in &chars {
                        if manu.is_none() && ch.uuid() == MANU {
                            log::trace!("reading device info-manu");
                            if let Ok(bytes) = ch.read().await {
                                manu = Some(String::from_utf8_lossy(&bytes).to_string());
                            }
                        }
                        if model.is_none() && ch.uuid() == MODEL {
                            log::trace!("reading device info-model");
                            if let Ok(bytes) = ch.read().await {
                                model = Some(String::from_utf8_lossy(&bytes).to_string());
                            }
                        }
                    }
                }
                let key = s.uuid().to_string();
                let value: BTreeSet<String> = chars
                    .into_iter()
                    .map(|c| c.uuid().to_string())
                    .collect();
                srvs.insert(key, value);
            }
        }
        log::debug!("{name:?} {}", dev.address());
        if name.is_some() || !characteristics.is_empty() || !srvs.is_empty() || manu.is_some() || model.is_some() {
            println!("found device {}", dev.address());
            if let Some(name) = name {
                println!("  name: {name}");
            }
            if let Some(manu) = manu {
                println!("  manu: {manu}");
            }
            if let Some(model) = model {
                println!("  model: {model}");
            }
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


async fn manu_model(dev: &Device) -> (Option<String>, Option<String>) {
    let manu = read_char(dev, MANU).await;
    let model = read_char(dev, MODEL).await;
    (manu, model)
}

async fn read_char(dev: &Device, id: Uuid) -> Option<String> {
    let ch = dev.characteristic(id).await.ok()??;
    let bytes = ch.read().await.ok()?;
    Some(String::from_utf8_lossy(&bytes).to_string())
}
