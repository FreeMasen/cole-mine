#![allow(unused)]
use uuid::Uuid;

pub const UART_SERVICE_UUID: Uuid = uuid::uuid!("6e40fff0-b5a3-f393-e0a9-e50e24dcca9e");
pub const CHARACTERISTIC_SERVICE_V2: Uuid = uuid::uuid!("de5bf728-d711-4e47-af26-65e3012a5dc7");
pub const UART_RX_CHAR_UUID: Uuid = uuid::uuid!("6e400002-b5a3-f393-e0a9-e50e24dcca9e");
pub const CHARACTERISTIC_COMMAND: Uuid = uuid::uuid!("de5bf72a-d711-4e47-af26-65e3012a5dc7");
pub const UART_TX_CHAR_UUID: Uuid = uuid::uuid!("6e400003-b5a3-f393-e0a9-e50e24dcca9e");
pub const CHARACTERISTIC_NOTIFY_V2: Uuid = uuid::uuid!("de5bf729-d711-4e47-af26-65e3012a5dc7");

pub const CMD_SET_DATE_TIME: u8 = 0x01;
pub const CMD_BATTERY: u8 = 0x03;
pub const CMD_PHONE_NAME: u8 = 0x04;
pub const CMD_POWER_OFF: u8 = 0x08;
pub const CMD_BLINK: u8 = 0x10;
pub const CMD_PREFERENCES: u8 = 0x0a;
pub const CMD_SYNC_HEART_RATE: u8 = 0x15;
pub const CMD_AUTO_HR_PREF: u8 = 0x16;
pub const CMD_GOALS: u8 = 0x21;
pub const CMD_AUTO_SPO2_PREF: u8 = 0x2c;
pub const CMD_PACKET_SIZE: u8 = 0x2f;
pub const CMD_AUTO_STRESS_PREF: u8 = 0x36;
pub const CMD_SYNC_STRESS: u8 = 0x37;
pub const CMD_AUTO_HRV_PREF: u8 = 0x38;
pub const CMD_SYNC_HRV: u8 = 0x39;
pub const CMD_SYNC_ACTIVITY: u8 = 0x43;
pub const CMD_FIND_DEVICE: u8 = 0x50;
pub const CMD_MANUAL_HEART_RATE: u8 = 0x69;
pub const CMD_NOTIFICATION: u8 = 0x73;
pub const CMD_BIG_DATA_V2: u8 = 0xbc;
pub const CMD_FACTORY_RESET: u8 = 0xff;
pub const PREF_READ: u8 = 0x01;
pub const PREF_WRITE: u8 = 0x02;
pub const PREF_DELETE: u8 = 0x03;
pub const NOTIFICATION_NEW_HR_DATA: u8 = 0x01;
pub const NOTIFICATION_NEW_SPO2_DATA: u8 = 0x03;
pub const NOTIFICATION_NEW_STEPS_DATA: u8 = 0x04;
pub const NOTIFICATION_BATTERY_LEVEL: u8 = 0x0c;
pub const NOTIFICATION_LIVE_ACTIVITY: u8 = 0x12;
pub const BIG_DATA_TYPE_SLEEP: u8 = 0x27;
pub const BIG_DATA_TYPE_SPO2: u8 = 0x2a;
pub const SLEEP_TYPE_LIGHT: u8 = 0x02;
pub const SLEEP_TYPE_DEEP: u8 = 0x03;
pub const SLEEP_TYPE_REM: u8 = 0x04;
pub const SLEEP_TYPE_AWAKE: u8 = 0x05;

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
    "COLMI R"
];
