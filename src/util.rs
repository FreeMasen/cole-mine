use std::time::Duration;

pub fn try_u16_from_le_slice(slice: &[u8]) -> Option<u16> {
    let mut bytes = [0u8; 2];
    bytes.copy_from_slice(slice.get(0..2)?);
    Some(u16::from_le_bytes(bytes))
}

pub fn try_u16_from_iter(slice: &mut dyn Iterator<Item = u8>) -> Option<u16> {
    let mut bytes = [0u8; 2];
    bytes[0] = slice.next()?;
    bytes[1] = slice.next()?;
    Some(u16::from_le_bytes(bytes))
}

pub trait DurationExt {
    fn minutes(value: u64) -> Duration;
    fn hours(value: u64) -> Duration;
    fn days(value: u64) -> Duration;
}

impl DurationExt for Duration {
    fn minutes(value: u64) -> Duration {
        Duration::from_secs(value * 60)
    }

    fn hours(value: u64) -> Duration {
        Duration::minutes(value * 60)
    }

    fn days(value: u64) -> Duration {
        Duration::hours(value * 24)
    }
}
