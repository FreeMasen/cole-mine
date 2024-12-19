use core::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use structsy::derive::{embedded_queries, PersistentEmbedded};
use time::{OffsetDateTime, PrimitiveDateTime};

use crate::Result;

#[derive(Debug, PartialEq, Eq, Clone, Copy, PersistentEmbedded, bon::Builder)]
pub struct DateTime {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    #[builder(default)]
    pub hour: u8,
    #[builder(default)]
    pub minute: u8,
    #[builder(default)]
    pub second: u8,
}

#[embedded_queries(DateTime)]
pub trait DateTimeQuery {
    fn with_year(self, year: u16) -> Self;
    fn with_month(self, month: u8) -> Self;
    fn with_day(self, day: u8) -> Self;
    fn with_ymd(self, year: u16, month: u8, day: u8) -> Self;
    fn with_hour(self, hour: u8) -> Self;
    fn with_minute(self, minute: u8) -> Self;
    fn with_second(self, minute: u8) -> Self;
    fn with_hms(self, hour: u8, minute: u8, second: u8) -> Self;
}

impl TryFrom<time::OffsetDateTime> for DateTime {
    type Error = Box<dyn std::error::Error>;

    fn try_from(value: time::OffsetDateTime) -> Result<Self> {
        Ok(Self {
            year: value.year().try_into()?,
            month: value.month().into(),
            day: value.day(),
            hour: value.hour(),
            minute: value.minute(),
            second: value.second(),
        })
    }
}

impl TryFrom<time::PrimitiveDateTime> for DateTime {
    type Error = Box<dyn std::error::Error>;

    fn try_from(value: time::PrimitiveDateTime) -> Result<Self> {
        Ok(Self {
            year: value.year().try_into()?,
            month: value.month().into(),
            day: value.day(),
            hour: value.hour(),
            minute: value.minute(),
            second: value.second(),
        })
    }
}

impl TryFrom<time::Date> for DateTime {
    type Error = Box<dyn std::error::Error>;

    fn try_from(value: time::Date) -> Result<Self> {
        Ok(Self {
            year: value.year().try_into()?,
            month: value.month().into(),
            day: value.day(),
            hour: 0,
            minute: 0,
            second: 0,
        })
    }
}

impl TryFrom<DateTime> for PrimitiveDateTime {
    type Error = Box<dyn std::error::Error>;

    fn try_from(value: DateTime) -> std::result::Result<Self, Self::Error> {
        Ok(PrimitiveDateTime::new(
            time::Date::from_calendar_date(
                value.year.try_into()?,
                value.month.try_into()?,
                value.day,
            )?,
            time::Time::from_hms(value.hour, value.minute, value.second)?,
        ))
    }
}

impl TryFrom<DateTime> for OffsetDateTime {
    type Error = Box<dyn std::error::Error>;

    fn try_from(value: DateTime) -> std::result::Result<Self, Self::Error> {
        Ok(PrimitiveDateTime::try_from(value)?.assume_utc())
    }
}

impl PartialEq<OffsetDateTime> for DateTime {
    fn eq(&self, other: &OffsetDateTime) -> bool {
        let Ok(year) = i32::try_from(self.year) else {
            return false;
        };
        let Ok(month) = time::Month::try_from(self.month) else {
            return false;
        };
        year == other.year()
            && month == other.month()
            && self.day == other.day()
            && self.hour == other.hour()
            && self.minute == other.minute()
            && self.second == other.second()
    }
}

impl PartialEq<DateTime> for OffsetDateTime {
    fn eq(&self, other: &DateTime) -> bool {
        other.eq(self)
    }
}

impl FromStr for DateTime {
    type Err = Box<dyn std::error::Error>;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        let mut dt_parts = s.split('T');
        let date_part = dt_parts
            .next()
            .ok_or_else(|| format!("String too short: `{s}`"))?;
        let mut date_parts = date_part.split('-');
        let year: u16 = date_parts
            .next()
            .ok_or_else(|| format!("date part too short: `{s}`"))?
            .parse()?;
        let month: u8 = date_parts
            .next()
            .ok_or_else(|| format!("date part missing month: `{s}`"))?
            .parse()?;
        let day: u8 = date_parts
            .next()
            .ok_or_else(|| format!("date part missing day: `{s}`"))?
            .parse()?;
        let Some(time_part) = dt_parts.next() else {
            return Ok(Self {
                year,
                month,
                day,
                hour: 0,
                minute: 0,
                second: 0,
            });
        };
        let mut time_parts = time_part.split(":");
        let hour = time_parts
            .next()
            .and_then(|s| s.parse().ok())
            .unwrap_or_default();
        let minute = time_parts
            .next()
            .and_then(|s| s.parse().ok())
            .unwrap_or_default();
        let second = time_parts
            .next()
            .and_then(|s| {
                let end_idx = s
                    .find(|ch: char| !ch.is_ascii_digit())
                    .unwrap_or_else(|| s.len());
                s.get(..end_idx)?.parse().ok()
            })
            .unwrap_or_default();
        Ok(Self {
            year,
            month,
            day,
            hour,
            minute,
            second,
        })
    }
}

impl fmt::Display for DateTime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("{:04}-", self.year))?;
        f.write_fmt(format_args!("{:02}-", self.month))?;
        f.write_fmt(format_args!("{:02}T", self.day))?;
        f.write_fmt(format_args!("{:02}:", self.hour))?;
        f.write_fmt(format_args!("{:02}:", self.minute))?;
        f.write_fmt(format_args!("{:02}.000Z", self.second))?;
        Ok(())
    }
}

impl Serialize for DateTime {
    fn serialize<S>(&self, s: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        s.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for DateTime {
    fn deserialize<D>(d: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        time::serde::rfc3339::deserialize(d)?
            .try_into()
            .map_err(|e| serde::de::Error::custom(e))
    }
}

impl PartialOrd for DateTime {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        use std::cmp::Ordering;
        Some(self.cmp(other))
    }
}

impl Ord for DateTime {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        use std::cmp::Ordering;
        fn try_prop<T>(l: T, r: T) -> Option<Ordering>
        where
            T: PartialOrd,
        {
            let ret = l.partial_cmp(&r)?;
            if matches!(ret, Ordering::Equal) {
                return None;
            }
            Some(ret)
        }
        if let Some(ret) = try_prop(self.year, other.year) {
            return ret;
        }
        if let Some(ret) = try_prop(self.month, other.month) {
            return ret;
        }
        if let Some(ret) = try_prop(self.day, other.day) {
            return ret;
        }
        if let Some(ret) = try_prop(self.hour, other.hour) {
            return ret;
        }
        if let Some(ret) = try_prop(self.minute, other.minute) {
            return ret;
        }
        if let Some(ret) = try_prop(self.second, other.second) {
            return ret;
        }
        Ordering::Equal
    }
}

#[cfg(test)]
mod tests {
    use rayon::iter::{IntoParallelIterator, ParallelIterator};
    use time::OffsetDateTime;

    use super::*;

    #[test]
    fn convert_revert() {
        do_convert_revert(65544);
    }

    #[test]
    fn convert_revert_many() {
        (0..=u16::MAX).into_iter().into_par_iter().for_each(|n| {
            do_convert_revert((n as i64) * 4);
        });
    }

    #[track_caller]
    fn do_convert_revert(timestamp: i64) {
        let dt = OffsetDateTime::from_unix_timestamp(timestamp).unwrap();
        let as_internal = DateTime::try_from(dt).unwrap();
        assert_eq!(dt, as_internal);
        let as_string = as_internal.to_string();
        let from_string: DateTime = as_string.parse().unwrap();
        assert_eq!(from_string, as_internal, "{from_string:?} != {as_string}");
        let from_internal = OffsetDateTime::try_from(from_string).unwrap();
        assert_eq!(dt, from_internal);
    }

    #[test]
    fn serde_round_trip() {
        do_serde_round_trip(66666);
    }

    #[test]
    fn serde_round_trip_many() {
        (0..=u16::MAX).into_iter().into_par_iter().for_each(|n| {
            do_serde_round_trip((n as i64) * 4);
        });
    }

    #[track_caller]
    fn do_serde_round_trip(timestamp: i64) {
        let dt = OffsetDateTime::from_unix_timestamp(timestamp).unwrap();
        let as_internal = DateTime::try_from(dt).unwrap();
        let json_string = serde_json::to_string(&as_internal).unwrap();
        let from_json: DateTime = serde_json::from_str(&json_string).unwrap();
        assert_eq!(as_internal, from_json);
    }
}
