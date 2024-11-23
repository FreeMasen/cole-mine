use bleasy::Device;
use cole_mine::discover;
use futures::StreamExt;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::OnceLock,
    time::Duration,
};
use tokio::time::timeout;
use uuid::Uuid;

const MANU: Uuid = uuid::uuid!("00002a29-0000-1000-8000-00805f9b34fb");
const MODEL: Uuid = uuid::uuid!("00002a24-0000-1000-8000-00805f9b34fb");
const DEV_INF: Uuid = uuid::uuid!("0000180a-0000-1000-8000-00805f9b34fb");
static SERVICE_NAMES: OnceLock<BTreeMap<u16, &'static str>> = OnceLock::new();
static CHARAS_NAMES: OnceLock<BTreeMap<u16, &'static str>> = OnceLock::new();

#[tokio::main]
async fn main() {
    env_logger::init();
    MANU.as_bytes();
    let max_op_secs = std::env::var("COLE_MINE_SCAN_MORE_TIMEOUT_SECS")
        .ok()
        .and_then(|a| a.parse::<u64>().ok())
        .unwrap_or(5);
    let force_all = std::env::var("COLE_MINE_SCAN_MORE_FORCE_ALL")
        .map(|v| v == "1")
        .unwrap_or(false);
    let mut stream = discover(false).await.unwrap();
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
        let characteristics: BTreeSet<Uuid> =
            timeout(Duration::from_secs(max_op_secs), dev.characteristics())
                .await
                .unwrap_or_else(|_| {
                    log::debug!("timed out looking up characteristics");
                    Ok(Vec::new())
                })
                .unwrap_or_default()
                .into_iter()
                .map(|c| c.uuid())
                .collect();
        log::trace!("looking up manu/model");
        let (mut manu, mut model) = timeout(Duration::from_secs(max_op_secs), manu_model(&dev))
            .await
            .unwrap_or_else(|_| {
                log::debug!("timed out looking for manu/model");
                (None, None)
            });
        let mut srvs = BTreeMap::new();
        log::trace!("looking up services");
        if let Ok(Ok(services)) = timeout(Duration::from_secs(max_op_secs), dev.services())
            .await
            .inspect_err(|_| {
                log::debug!("timed out looking for services");
            })
        {
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
                let key = s.uuid();
                let value: BTreeSet<Uuid> = chars.into_iter().map(|c| c.uuid()).collect();
                srvs.insert(key, value);
            }
        }
        log::debug!("{name:?} {}", dev.address());
        if force_all
            || name.is_some()
            || !characteristics.is_empty()
            || !srvs.is_empty()
            || manu.is_some()
            || model.is_some()
        {
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
                print!("    {ch}");
                if let Some(name) = charas_name_from(ch) {
                    print!("-{name}");
                }
                println!();
            }
            println!("  srv_ct: {service_count}");
            if !srvs.is_empty() {
                println!("  srvs:");
                for (id, charas) in &srvs {
                    print!("    srv: {id}");
                    if let Some(name) = service_name_from(*id) {
                        print!("-{name}")
                    }
                    println!();
                    for ch in charas.iter() {
                        print!("      ch: {ch}");
                        if let Some(name) = charas_name_from(*ch) {
                            print!("-{name}");
                        }
                        println!();
                    }
                }
            }
        }
    }
}

fn service_name_from(id: Uuid) -> Option<&'static str> {
    let id = uuid_to_id(id)?;
    SERVICE_NAMES
        .get_or_init(generate_service_map)
        .get(&id)
        .map(|s| *s)
}
fn charas_name_from(id: Uuid) -> Option<&'static str> {
    let id = uuid_to_id(id)?;
    CHARAS_NAMES
        .get_or_init(generate_charas_map)
        .get(&id)
        .map(|s| *s)
}

fn uuid_to_id(id: Uuid) -> Option<u16> {
    let bytes = id.as_bytes();
    if !bytes.ends_with(&[0x10, 0, 0x80, 0, 0, 0x80, 0x5f, 0x9b, 0x34, 0xfb]) {
        return None;
    }
    let mut id_bytes = [0u8; 2];
    id_bytes.copy_from_slice(&bytes[2..4]);
    Some(u16::from_be_bytes(id_bytes))
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

fn generate_service_map() -> BTreeMap<u16, &'static str> {
    let mut map = BTreeMap::new();
    map.insert(0x1800, "GAP");
    map.insert(0x1801, "GATT");
    map.insert(0x1802, "Immediate Alert");
    map.insert(0x1803, "Link Loss");
    map.insert(0x1804, "Tx Power");
    map.insert(0x1805, "Current Time");
    map.insert(0x1806, "Reference Time Update");
    map.insert(0x1807, "Next DST Change");
    map.insert(0x1808, "Glucose");
    map.insert(0x1809, "Health Thermometer");
    map.insert(0x180A, "Device Information");
    map.insert(0x180D, "Heart Rate");
    map.insert(0x180E, "Phone Alert Status");
    map.insert(0x180F, "Battery");
    map.insert(0x1810, "Blood Pressure");
    map.insert(0x1811, "Alert Notification");
    map.insert(0x1812, "Human Interface Device");
    map.insert(0x1813, "Scan Parameters");
    map.insert(0x1814, "Running Speed and Cadence");
    map.insert(0x1815, "Automation IO");
    map.insert(0x1816, "Cycling Speed and Cadence");
    map.insert(0x1818, "Cycling Power");
    map.insert(0x1819, "Location and Navigation");
    map.insert(0x181A, "Environmental Sensing");
    map.insert(0x181B, "Body Composition");
    map.insert(0x181C, "User Data");
    map.insert(0x181D, "Weight Scale");
    map.insert(0x181E, "Bond Management");
    map.insert(0x181F, "Continuous Glucose Monitoring");
    map.insert(0x1820, "Internet Protocol Support");
    map.insert(0x1821, "Indoor Positioning");
    map.insert(0x1822, "Pulse Oximeter");
    map.insert(0x1823, "HTTP Proxy");
    map.insert(0x1824, "Transport Discovery");
    map.insert(0x1825, "Object Transfer");
    map.insert(0x1826, "Fitness Machine");
    map.insert(0x1827, "Mesh Provisioning");
    map.insert(0x1828, "Mesh Proxy");
    map.insert(0x1829, "Reconnection Configuration");
    map.insert(0x183A, "Insulin Delivery");
    map.insert(0x183B, "Binary Sensor");
    map.insert(0x183C, "Emergency Configuration");
    map.insert(0x183D, "Authorization Control");
    map.insert(0x183E, "Physical Activity Monitor");
    map.insert(0x183F, "Elapsed Time");
    map.insert(0x1840, "Generic Health Sensor");
    map.insert(0x1843, "Audio Input Control");
    map.insert(0x1844, "Volume Control");
    map.insert(0x1845, "Volume Offset Control");
    map.insert(0x1846, "Coordinated Set Identification");
    map.insert(0x1847, "Device Time");
    map.insert(0x1848, "Media Control");
    map.insert(0x1849, "Generic Media Control");
    map.insert(0x184A, "Constant Tone Extension");
    map.insert(0x184B, "Telephone Bearer");
    map.insert(0x184C, "Generic Telephone Bearer");
    map.insert(0x184D, "Microphone Control");
    map.insert(0x184E, "Audio Stream Control");
    map.insert(0x184F, "Broadcast Audio Scan");
    map.insert(0x1850, "Published Audio Capabilities");
    map.insert(0x1851, "Basic Audio Announcement");
    map.insert(0x1852, "Broadcast Audio Announcement");
    map.insert(0x1853, "Common Audio");
    map.insert(0x1854, "Hearing Access");
    map.insert(0x1855, "Telephony and Media Audio");
    map.insert(0x1856, "Public Broadcast Announcement");
    map.insert(0x1857, "Electronic Shelf Label");
    map.insert(0x1858, "Gaming Audio");
    map.insert(0x1859, "Mesh Proxy Solicitation");
    map.insert(0x185A, "Industrial Measurement Device");
    map
}
fn generate_charas_map() -> BTreeMap<u16, &'static str> {
    let mut map = BTreeMap::new();
    map.insert(0x2A00, "Device Name");
    map.insert(0x2A01, "Appearance");
    map.insert(0x2A02, "Peripheral Privacy Flag");
    map.insert(0x2A03, "Reconnection Address");
    map.insert(0x2A04, "Peripheral Preferred Connection Parameters");
    map.insert(0x2A05, "Service Changed");
    map.insert(0x2A06, "Alert Level");
    map.insert(0x2A07, "Tx Power Level");
    map.insert(0x2A08, "Date Time");
    map.insert(0x2A09, "Day of Week");
    map.insert(0x2A0A, "Day Date Time");
    map.insert(0x2A0C, "Exact Time 256");
    map.insert(0x2A0D, "DST Offset");
    map.insert(0x2A0E, "Time Zone");
    map.insert(0x2A0F, "Local Time Information");
    map.insert(0x2A11, "Time with DST");
    map.insert(0x2A12, "Time Accuracy");
    map.insert(0x2A13, "Time Source");
    map.insert(0x2A14, "Reference Time Information");
    map.insert(0x2A16, "Time Update Control Point");
    map.insert(0x2A17, "Time Update State");
    map.insert(0x2A18, "Glucose Measurement");
    map.insert(0x2A19, "Battery Level");
    map.insert(0x2A1C, "Temperature Measurement");
    map.insert(0x2A1D, "Temperature Type");
    map.insert(0x2A1E, "Intermediate Temperature");
    map.insert(0x2A21, "Measurement Interval");
    map.insert(0x2A22, "Boot Keyboard Input Report");
    map.insert(0x2A23, "System ID");
    map.insert(0x2A24, "Model Number String");
    map.insert(0x2A25, "Serial Number String");
    map.insert(0x2A26, "Firmware Revision String");
    map.insert(0x2A27, "Hardware Revision String");
    map.insert(0x2A28, "Software Revision String");
    map.insert(0x2A29, "Manufacturer Name String");
    map.insert(
        0x2A2A,
        "IEEE 11073-20601 Regulatory Certification Data List",
    );
    map.insert(0x2A2B, "Current Time");
    map.insert(0x2A2C, "Magnetic Declination");
    map.insert(0x2A31, "Scan Refresh");
    map.insert(0x2A32, "Boot Keyboard Output Report");
    map.insert(0x2A33, "Boot Mouse Input Report");
    map.insert(0x2A34, "Glucose Measurement Context");
    map.insert(0x2A35, "Blood Pressure Measurement");
    map.insert(0x2A36, "Intermediate Cuff Pressure");
    map.insert(0x2A37, "Heart Rate Measurement");
    map.insert(0x2A38, "Body Sensor Location");
    map.insert(0x2A39, "Heart Rate Control Point");
    map.insert(0x2A3F, "Alert Status");
    map.insert(0x2A40, "Ringer Control Point");
    map.insert(0x2A41, "Ringer Setting");
    map.insert(0x2A42, "Alert Category ID Bit Mask");
    map.insert(0x2A43, "Alert Category ID");
    map.insert(0x2A44, "Alert Notification Control Point");
    map.insert(0x2A45, "Unread Alert Status");
    map.insert(0x2A46, "New Alert");
    map.insert(0x2A47, "Supported New Alert Category");
    map.insert(0x2A48, "Supported Unread Alert Category");
    map.insert(0x2A49, "Blood Pressure Feature");
    map.insert(0x2A4A, "HID Information");
    map.insert(0x2A4B, "Report Map");
    map.insert(0x2A4C, "HID Control Point");
    map.insert(0x2A4D, "Report");
    map.insert(0x2A4E, "Protocol Mode");
    map.insert(0x2A4F, "Scan Interval Window");
    map.insert(0x2A50, "PnP ID");
    map.insert(0x2A51, "Glucose Feature");
    map.insert(0x2A52, "Record Access Control Point");
    map.insert(0x2A53, "RSC Measurement");
    map.insert(0x2A54, "RSC Feature");
    map.insert(0x2A55, "SC Control Point");
    map.insert(0x2A5A, "Aggregate");
    map.insert(0x2A5B, "CSC Measurement");
    map.insert(0x2A5C, "CSC Feature");
    map.insert(0x2A5D, "Sensor Location");
    map.insert(0x2A5E, "PLX Spot-Check Measurement");
    map.insert(0x2A5F, "PLX Continuous Measurement");
    map.insert(0x2A60, "PLX Features");
    map.insert(0x2A63, "Cycling Power Measurement");
    map.insert(0x2A64, "Cycling Power Vector");
    map.insert(0x2A65, "Cycling Power Feature");
    map.insert(0x2A66, "Cycling Power Control Point");
    map.insert(0x2A67, "Location and Speed");
    map.insert(0x2A68, "Navigation");
    map.insert(0x2A69, "Position Quality");
    map.insert(0x2A6A, "LN Feature");
    map.insert(0x2A6B, "LN Control Point");
    map.insert(0x2A6C, "Elevation");
    map.insert(0x2A6D, "Pressure");
    map.insert(0x2A6E, "Temperature");
    map.insert(0x2A6F, "Humidity");
    map.insert(0x2A70, "True Wind Speed");
    map.insert(0x2A71, "True Wind Direction");
    map.insert(0x2A72, "Apparent Wind Speed");
    map.insert(0x2A73, "Apparent Wind Direction");
    map.insert(0x2A74, "Gust Factor");
    map.insert(0x2A75, "Pollen Concentration");
    map.insert(0x2A76, "UV Index");
    map.insert(0x2A77, "Irradiance");
    map.insert(0x2A78, "Rainfall");
    map.insert(0x2A79, "Wind Chill");
    map.insert(0x2A7A, "Heat Index");
    map.insert(0x2A7B, "Dew Point");
    map.insert(0x2A7D, "Descriptor Value Changed");
    map.insert(0x2A7E, "Aerobic Heart Rate Lower Limit");
    map.insert(0x2A7F, "Aerobic Threshold");
    map.insert(0x2A80, "Age");
    map.insert(0x2A81, "Anaerobic Heart Rate Lower Limit");
    map.insert(0x2A82, "Anaerobic Heart Rate Upper Limit");
    map.insert(0x2A83, "Anaerobic Threshold");
    map.insert(0x2A84, "Aerobic Heart Rate Upper Limit");
    map.insert(0x2A85, "Date of Birth");
    map.insert(0x2A86, "Date of Threshold Assessment");
    map.insert(0x2A87, "Email Address");
    map.insert(0x2A88, "Fat Burn Heart Rate Lower Limit");
    map.insert(0x2A89, "Fat Burn Heart Rate Upper Limit");
    map.insert(0x2A8A, "First Name");
    map.insert(0x2A8B, "Five Zone Heart Rate Limits");
    map.insert(0x2A8C, "Gender");
    map.insert(0x2A8D, "Heart Rate Max");
    map.insert(0x2A8E, "Height");
    map.insert(0x2A8F, "Hip Circumference");
    map.insert(0x2A90, "Last Name");
    map.insert(0x2A91, "Maximum Recommended Heart Rate");
    map.insert(0x2A92, "Resting Heart Rate");
    map.insert(0x2A93, "Sport Type for Aerobic and Anaerobic Thresholds");
    map.insert(0x2A94, "Three Zone Heart Rate Limits");
    map.insert(0x2A95, "Two Zone Heart Rate Limits");
    map.insert(0x2A96, "VO2 Max");
    map.insert(0x2A97, "Waist Circumference");
    map.insert(0x2A98, "Weight");
    map.insert(0x2A99, "Database Change Increment");
    map.insert(0x2A9A, "User Index");
    map.insert(0x2A9B, "Body Composition Feature");
    map.insert(0x2A9C, "Body Composition Measurement");
    map.insert(0x2A9D, "Weight Measurement");
    map.insert(0x2A9E, "Weight Scale Feature");
    map.insert(0x2A9F, "User Control Point");
    map.insert(0x2AA0, "Magnetic Flux Density - 2D");
    map.insert(0x2AA1, "Magnetic Flux Density - 3D");
    map.insert(0x2AA2, "Language");
    map.insert(0x2AA3, "Barometric Pressure Trend");
    map.insert(0x2AA4, "Bond Management Control Point");
    map.insert(0x2AA5, "Bond Management Feature");
    map.insert(0x2AA6, "Central Address Resolution");
    map.insert(0x2AA7, "CGM Measurement");
    map.insert(0x2AA8, "CGM Feature");
    map.insert(0x2AA9, "CGM Status");
    map.insert(0x2AAA, "CGM Session Start Time");
    map.insert(0x2AAB, "CGM Session Run Time");
    map.insert(0x2AAC, "CGM Specific Ops Control Point");
    map.insert(0x2AAD, "Indoor Positioning Configuration");
    map.insert(0x2AAE, "Latitude");
    map.insert(0x2AAF, "Longitude");
    map.insert(0x2AB0, "Local North Coordinate");
    map.insert(0x2AB1, "Local East Coordinate");
    map.insert(0x2AB2, "Floor Number");
    map.insert(0x2AB3, "Altitude");
    map.insert(0x2AB4, "Uncertainty");
    map.insert(0x2AB5, "Location Name");
    map.insert(0x2AB6, "URI");
    map.insert(0x2AB7, "HTTP Headers");
    map.insert(0x2AB8, "HTTP Status Code");
    map.insert(0x2AB9, "HTTP Entity Body");
    map.insert(0x2ABA, "HTTP Control Point");
    map.insert(0x2ABB, "HTTPS Security");
    map.insert(0x2ABC, "TDS Control Point");
    map.insert(0x2ABD, "OTS Feature");
    map.insert(0x2ABE, "Object Name");
    map.insert(0x2ABF, "Object Type");
    map.insert(0x2AC0, "Object Size");
    map.insert(0x2AC1, "Object First-Created");
    map.insert(0x2AC2, "Object Last-Modified");
    map.insert(0x2AC3, "Object ID");
    map.insert(0x2AC4, "Object Properties");
    map.insert(0x2AC5, "Object Action Control Point");
    map.insert(0x2AC6, "Object List Control Point");
    map.insert(0x2AC7, "Object List Filter");
    map.insert(0x2AC8, "Object Changed");
    map.insert(0x2AC9, "Resolvable Private Address Only");
    map.insert(0x2ACC, "Fitness Machine Feature");
    map.insert(0x2ACD, "Treadmill Data");
    map.insert(0x2ACE, "Cross Trainer Data");
    map.insert(0x2ACF, "Step Climber Data");
    map.insert(0x2AD0, "Stair Climber Data");
    map.insert(0x2AD1, "Rower Data");
    map.insert(0x2AD2, "Indoor Bike Data");
    map.insert(0x2AD3, "Training Status");
    map.insert(0x2AD4, "Supported Speed Range");
    map.insert(0x2AD5, "Supported Inclination Range");
    map.insert(0x2AD6, "Supported Resistance Level Range");
    map.insert(0x2AD7, "Supported Heart Rate Range");
    map.insert(0x2AD8, "Supported Power Range");
    map.insert(0x2AD9, "Fitness Machine Control Point");
    map.insert(0x2ADA, "Fitness Machine Status");
    map.insert(0x2ADB, "Mesh Provisioning Data In");
    map.insert(0x2ADC, "Mesh Provisioning Data Out");
    map.insert(0x2ADD, "Mesh Proxy Data In");
    map.insert(0x2ADE, "Mesh Proxy Data Out");
    map.insert(0x2AE0, "Average Current");
    map.insert(0x2AE1, "Average Voltage");
    map.insert(0x2AE2, "Boolean");
    map.insert(0x2AE3, "Chromatic Distance from Planckian");
    map.insert(0x2AE4, "Chromaticity Coordinates");
    map.insert(0x2AE5, "Chromaticity in CCT and Duv Values");
    map.insert(0x2AE6, "Chromaticity Tolerance");
    map.insert(0x2AE7, "CIE 13.3-1995 Color Rendering Index");
    map.insert(0x2AE8, "Coefficient");
    map.insert(0x2AE9, "Correlated Color Temperature");
    map.insert(0x2AEA, "Count 16");
    map.insert(0x2AEB, "Count 24");
    map.insert(0x2AEC, "Country Code");
    map.insert(0x2AED, "Date UTC");
    map.insert(0x2AEE, "Electric Current");
    map.insert(0x2AEF, "Electric Current Range");
    map.insert(0x2AF0, "Electric Current Specification");
    map.insert(0x2AF1, "Electric Current Statistics");
    map.insert(0x2AF2, "Energy");
    map.insert(0x2AF3, "Energy in a Period of Day");
    map.insert(0x2AF4, "Event Statistics");
    map.insert(0x2AF5, "Fixed String 16");
    map.insert(0x2AF6, "Fixed String 24");
    map.insert(0x2AF7, "Fixed String 36");
    map.insert(0x2AF8, "Fixed String 8");
    map.insert(0x2AF9, "Generic Level");
    map.insert(0x2AFA, "Global Trade Item Number");
    map.insert(0x2AFB, "Illuminance");
    map.insert(0x2AFC, "Luminous Efficacy");
    map.insert(0x2AFD, "Luminous Energy");
    map.insert(0x2AFE, "Luminous Exposure");
    map.insert(0x2AFF, "Luminous Flux");
    map.insert(0x2B00, "Luminous Flux Range");
    map.insert(0x2B01, "Luminous Intensity");
    map.insert(0x2B02, "Mass Flow");
    map.insert(0x2B03, "Perceived Lightness");
    map.insert(0x2B04, "Percentage 8");
    map.insert(0x2B05, "Power");
    map.insert(0x2B06, "Power Specification");
    map.insert(0x2B07, "Relative Runtime in a Current Range");
    map.insert(0x2B08, "Relative Runtime in a Generic Level Range");
    map.insert(0x2B09, "Relative Value in a Voltage Range");
    map.insert(0x2B0A, "Relative Value in an Illuminance Range");
    map.insert(0x2B0B, "Relative Value in a Period of Day");
    map.insert(0x2B0C, "Relative Value in a Temperature Range");
    map.insert(0x2B0D, "Temperature 8");
    map.insert(0x2B0E, "Temperature 8 in a Period of Day");
    map.insert(0x2B0F, "Temperature 8 Statistics");
    map.insert(0x2B10, "Temperature Range");
    map.insert(0x2B11, "Temperature Statistics");
    map.insert(0x2B12, "Time Decihour 8");
    map.insert(0x2B13, "Time Exponential 8");
    map.insert(0x2B14, "Time Hour 24");
    map.insert(0x2B15, "Time Millisecond 24");
    map.insert(0x2B16, "Time Second 16");
    map.insert(0x2B17, "Time Second 8");
    map.insert(0x2B18, "Voltage");
    map.insert(0x2B19, "Voltage Specification");
    map.insert(0x2B1A, "Voltage Statistics");
    map.insert(0x2B1B, "Volume Flow");
    map.insert(0x2B1C, "Chromaticity Coordinate");
    map.insert(0x2B1D, "RC Feature");
    map.insert(0x2B1E, "RC Settings");
    map.insert(0x2B1F, "Reconnection Configuration Control Point");
    map.insert(0x2B20, "IDD Status Changed");
    map.insert(0x2B21, "IDD Status");
    map.insert(0x2B22, "IDD Annunciation Status");
    map.insert(0x2B23, "IDD Features");
    map.insert(0x2B24, "IDD Status Reader Control Point");
    map.insert(0x2B25, "IDD Command Control Point");
    map.insert(0x2B26, "IDD Command Data");
    map.insert(0x2B27, "IDD Record Access Control Point");
    map.insert(0x2B28, "IDD History Data");
    map.insert(0x2B29, "Client Supported Features");
    map.insert(0x2B2A, "Database Hash");
    map.insert(0x2B2B, "BSS Control Point");
    map.insert(0x2B2C, "BSS Response");
    map.insert(0x2B2D, "Emergency ID");
    map.insert(0x2B2E, "Emergency Text");
    map.insert(0x2B2F, "ACS Status");
    map.insert(0x2B30, "ACS Data In");
    map.insert(0x2B31, "ACS Data Out Notify");
    map.insert(0x2B32, "ACS Data Out Indicate");
    map.insert(0x2B33, "ACS Control Point");
    map.insert(0x2B34, "Enhanced Blood Pressure Measurement");
    map.insert(0x2B35, "Enhanced Intermediate Cuff Pressure");
    map.insert(0x2B36, "Blood Pressure Record");
    map.insert(0x2B37, "Registered User");
    map.insert(0x2B38, "BR-EDR Handover Data");
    map.insert(0x2B39, "Bluetooth SIG Data");
    map.insert(0x2B3A, "Server Supported Features");
    map.insert(0x2B3B, "Physical Activity Monitor Features");
    map.insert(0x2B3C, "General Activity Instantaneous Data");
    map.insert(0x2B3D, "General Activity Summary Data");
    map.insert(0x2B3E, "CardioRespiratory Activity Instantaneous Data");
    map.insert(0x2B3F, "CardioRespiratory Activity Summary Data");
    map.insert(0x2B40, "Step Counter Activity Summary Data");
    map.insert(0x2B41, "Sleep Activity Instantaneous Data");
    map.insert(0x2B42, "Sleep Activity Summary Data");
    map.insert(0x2B43, "Physical Activity Monitor Control Point");
    map.insert(0x2B44, "Physical Activity Current Session");
    map.insert(0x2B45, "Physical Activity Session Descriptor");
    map.insert(0x2B46, "Preferred Units");
    map.insert(0x2B47, "High Resolution Height");
    map.insert(0x2B48, "Middle Name");
    map.insert(0x2B49, "Stride Length");
    map.insert(0x2B4A, "Handedness");
    map.insert(0x2B4B, "Device Wearing Position");
    map.insert(0x2B4C, "Four Zone Heart Rate Limits");
    map.insert(0x2B4D, "High Intensity Exercise Threshold");
    map.insert(0x2B4E, "Activity Goal");
    map.insert(0x2B4F, "Sedentary Interval Notification");
    map.insert(0x2B50, "Caloric Intake");
    map.insert(0x2B51, "TMAP Role");
    map.insert(0x2B77, "Audio Input State");
    map.insert(0x2B78, "Gain Settings Attribute");
    map.insert(0x2B79, "Audio Input Type");
    map.insert(0x2B7A, "Audio Input Status");
    map.insert(0x2B7B, "Audio Input Control Point");
    map.insert(0x2B7C, "Audio Input Description");
    map.insert(0x2B7D, "Volume State");
    map.insert(0x2B7E, "Volume Control Point");
    map.insert(0x2B7F, "Volume Flags");
    map.insert(0x2B80, "Volume Offset State");
    map.insert(0x2B81, "Audio Location");
    map.insert(0x2B82, "Volume Offset Control Point");
    map.insert(0x2B83, "Audio Output Description");
    map.insert(0x2B84, "Set Identity Resolving Key");
    map.insert(0x2B85, "Coordinated Set Size");
    map.insert(0x2B86, "Set Member Lock");
    map.insert(0x2B87, "Set Member Rank");
    map.insert(0x2B88, "Encrypted Data Key Material");
    map.insert(0x2B89, "Apparent Energy 32");
    map.insert(0x2B8A, "Apparent Power");
    map.insert(0x2B8B, "Live Health Observations");
    map.insert(0x2B8C, "CO2 Concentration");
    map.insert(0x2B8D, "Cosine of the Angle");
    map.insert(0x2B8E, "Device Time Feature");
    map.insert(0x2B8F, "Device Time Parameters");
    map.insert(0x2B90, "Device Time");
    map.insert(0x2B91, "Device Time Control Point");
    map.insert(0x2B92, "Time Change Log Data");
    map.insert(0x2B93, "Media Player Name");
    map.insert(0x2B94, "Media Player Icon Object ID");
    map.insert(0x2B95, "Media Player Icon URL");
    map.insert(0x2B96, "Track Changed");
    map.insert(0x2B97, "Track Title");
    map.insert(0x2B98, "Track Duration");
    map.insert(0x2B99, "Track Position");
    map.insert(0x2B9A, "Playback Speed");
    map.insert(0x2B9B, "Seeking Speed");
    map.insert(0x2B9C, "Current Track Segments Object ID");
    map.insert(0x2B9D, "Current Track Object ID");
    map.insert(0x2B9E, "Next Track Object ID");
    map.insert(0x2B9F, "Parent Group Object ID");
    map.insert(0x2BA0, "Current Group Object ID");
    map.insert(0x2BA1, "Playing Order");
    map.insert(0x2BA2, "Playing Orders Supported");
    map.insert(0x2BA3, "Media State");
    map.insert(0x2BA4, "Media Control Point");
    map.insert(0x2BA5, "Media Control Point Opcodes Supported");
    map.insert(0x2BA6, "Search Results Object ID");
    map.insert(0x2BA7, "Search Control Point");
    map.insert(0x2BA8, "Energy 32");
    map.insert(0x2BAD, "Constant Tone Extension Enable");
    map.insert(0x2BAE, "Advertising Constant Tone Extension Minimum Length");
    map.insert(
        0x2BAF,
        "Advertising Constant Tone Extension Minimum Transmit Count",
    );
    map.insert(
        0x2BB0,
        "Advertising Constant Tone Extension Transmit Duration",
    );
    map.insert(0x2BB1, "Advertising Constant Tone Extension Interval");
    map.insert(0x2BB2, "Advertising Constant Tone Extension PHY");
    map.insert(0x2BB3, "Bearer Provider Name");
    map.insert(0x2BB4, "Bearer UCI");
    map.insert(0x2BB5, "Bearer Technology");
    map.insert(0x2BB6, "Bearer URI Schemes Supported List");
    map.insert(0x2BB7, "Bearer Signal Strength");
    map.insert(0x2BB8, "Bearer Signal Strength Reporting Interval");
    map.insert(0x2BB9, "Bearer List Current Calls");
    map.insert(0x2BBA, "Content Control ID");
    map.insert(0x2BBB, "Status Flags");
    map.insert(0x2BBC, "Incoming Call Target Bearer URI");
    map.insert(0x2BBD, "Call State");
    map.insert(0x2BBE, "Call Control Point");
    map.insert(0x2BBF, "Call Control Point Optional Opcodes");
    map.insert(0x2BC0, "Termination Reason");
    map.insert(0x2BC1, "Incoming Call");
    map.insert(0x2BC2, "Call Friendly Name");
    map.insert(0x2BC3, "Mute");
    map.insert(0x2BC4, "Sink ASE");
    map.insert(0x2BC5, "Source ASE");
    map.insert(0x2BC6, "ASE Control Point");
    map.insert(0x2BC7, "Broadcast Audio Scan Control Point");
    map.insert(0x2BC8, "Broadcast Receive State");
    map.insert(0x2BC9, "Sink PAC");
    map.insert(0x2BCA, "Sink Audio Locations");
    map.insert(0x2BCB, "Source PAC");
    map.insert(0x2BCC, "Source Audio Locations");
    map.insert(0x2BCD, "Available Audio Contexts");
    map.insert(0x2BCE, "Supported Audio Contexts");
    map.insert(0x2BCF, "Ammonia Concentration");
    map.insert(0x2BD0, "Carbon Monoxide Concentration");
    map.insert(0x2BD1, "Methane Concentration");
    map.insert(0x2BD2, "Nitrogen Dioxide Concentration");
    map.insert(
        0x2BD3,
        "Non-Methane Volatile Organic Compounds Concentration",
    );
    map.insert(0x2BD4, "Ozone Concentration");
    map.insert(0x2BD5, "Particulate Matter - PM1 Concentration");
    map.insert(0x2BD6, "Particulate Matter - PM2.5 Concentration");
    map.insert(0x2BD7, "Particulate Matter - PM10 Concentration");
    map.insert(0x2BD8, "Sulfur Dioxide Concentration");
    map.insert(0x2BD9, "Sulfur Hexafluoride Concentration");
    map.insert(0x2BDA, "Hearing Aid Features");
    map.insert(0x2BDB, "Hearing Aid Preset Control Point");
    map.insert(0x2BDC, "Active Preset Index");
    map.insert(0x2BDD, "Stored Health Observations");
    map.insert(0x2BDE, "Fixed String 64");
    map.insert(0x2BDF, "High Temperature");
    map.insert(0x2BE0, "High Voltage");
    map.insert(0x2BE1, "Light Distribution");
    map.insert(0x2BE2, "Light Output");
    map.insert(0x2BE3, "Light Source Type");
    map.insert(0x2BE4, "Noise");
    map.insert(
        0x2BE5,
        "Relative Runtime in a Correlated Color Temperature Range",
    );
    map.insert(0x2BE6, "Time Second 32");
    map.insert(0x2BE7, "VOC Concentration");
    map.insert(0x2BE8, "Voltage Frequency");
    map.insert(0x2BE9, "Battery Critical Status");
    map.insert(0x2BEA, "Battery Health Status");
    map.insert(0x2BEB, "Battery Health Information");
    map.insert(0x2BEC, "Battery Information");
    map.insert(0x2BED, "Battery Level Status");
    map.insert(0x2BEE, "Battery Time Status");
    map.insert(0x2BEF, "Estimated Service Date");
    map.insert(0x2BF0, "Battery Energy Status");
    map.insert(0x2BF1, "Observation Schedule Changed");
    map.insert(0x2BF2, "Current Elapsed Time");
    map.insert(0x2BF3, "Health Sensor Features");
    map.insert(0x2BF4, "GHS Control Point");
    map.insert(0x2BF5, "LE GATT Security Levels");
    map.insert(0x2BF6, "ESL Address");
    map.insert(0x2BF7, "AP Sync Key Material");
    map.insert(0x2BF8, "ESL Response Key Material");
    map.insert(0x2BF9, "ESL Current Absolute Time");
    map.insert(0x2BFA, "ESL Display Information");
    map.insert(0x2BFB, "ESL Image Information");
    map.insert(0x2BFC, "ESL Sensor Information");
    map.insert(0x2BFD, "ESL LED Information");
    map.insert(0x2BFE, "ESL Control Point");
    map.insert(0x2BFF, "UDI for Medical Devices");
    map.insert(0x2C00, "GMAP Role");
    map.insert(0x2C01, "UGG Features");
    map.insert(0x2C02, "UGT Features");
    map.insert(0x2C03, "BGS Features");
    map.insert(0x2C04, "BGR Features");
    map.insert(0x2C05, "Percentage 8 Steps");
    map.insert(0x2C06, "Acceleration");
    map.insert(0x2C07, "Force");
    map.insert(0x2C08, "Linear Position");
    map.insert(0x2C09, "Rotational Speed");
    map.insert(0x2C0A, "Length");
    map.insert(0x2C0B, "Torque");
    map.insert(0x2C0C, "IMD Status");
    map.insert(0x2C0D, "IMDS Descriptor Value Changed");
    map.insert(0x2C0E, "First Use Date");
    map.insert(0x2C0F, "Life Cycle Data");
    map.insert(0x2C10, "Work Cycle Data");
    map.insert(0x2C11, "Service Cycle Data");
    map.insert(0x2C12, "IMD Control");
    map.insert(0x2C13, "IMD Historical Data");
    map
}
