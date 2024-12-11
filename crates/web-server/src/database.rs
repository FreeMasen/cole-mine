use std::{path::Path, ops::RangeBounds};

use crate::Result;
use serde::{Deserialize, Serialize};
use structsy::{derive::queries, internal::{EmbeddedDescription, FieldDescription, SimpleValueTypeBuilder, StructDescription, ValueTypeBuilder}, Filter, PersistentEmbedded, Structsy, StructsyTx};
use time::OffsetDateTime;

#[derive(Clone)]
pub struct Database(Structsy);

impl Database {
    
    pub fn new(path: impl AsRef<Path>) -> Result<Self> {
        let inner =
            Structsy::open(path.as_ref()).map_err(|e| format!("Error opening database: {e}"))?;
        let ret = Self(inner);
        ret.init()?;
        Ok(ret)
    }

    #[cfg(test)]
    fn test() -> Result<Self> {
        let inner = Structsy::memory()?;
        let ret = Self(inner);
        ret.init()?;
        Ok(ret)
    }

    fn init(&self) -> Result {
        self.0.define::<Ring>()?;
        self.0.define::<RingEvent>()?;
        Ok(())
    }

    pub fn get_rings(&self) -> Vec<Ring> {
        self.0.query::<Ring>().into_iter().map(|(_, e)| e).collect()
    }

    pub fn get_ring(&self, mac: &str) -> Result<Ring> {
        let (_, ret) = self
            .0
            .query()
            .with_mac(mac)
            .fetch()
            .next()
            .ok_or_else(|| format!("unable to find ring with {mac}"))?;
        Ok(ret)
    }

    pub fn add_ring(&self, ring: &Ring) -> Result {
        let mut tx = self.0.begin()?;
        tx.insert(ring)?;
        tx.commit()?;
        Ok(())
    }

    pub fn update_ring(&self, ring: &Ring) -> Result {
        let mut tx = self.0.begin()?;
        let db = tx
            .query()
            .with_mac(&ring.mac)
            .fetch()
            .next()
            .ok_or_else(|| format!("unable to find ring with {}", ring.mac))?;
        tx.update(&db.0, ring)?;
        tx.commit()?;
        Ok(())
    }

    pub fn get_events_for_ring(
        &self,
        mac: &str,
        when: OffsetDateTime,
    ) -> Result<Vec<RingEvent>> {
        let min = when.date().midnight().assume_utc();
        let max = min.date().next_day().ok_or_else(|| {
            format!("Missing next day {min}")
        })?.midnight().assume_utc();

        let mut q = self.0.query::<RingEvent>()
            .with_ring_mac(mac)
            .between_time(DateTime(min)..DateTime(max));
        
        Ok(q.into_iter().map(|(_, event)| event).collect())
    }

    pub fn add_events(&self, events: &[RingEvent]) -> Result<()> {
        let mut tx = self.0.begin()?;
        
        for event in events {
            let existing = tx
                .query::<RingEvent>()
                .with_ring_mac(&event.mac)
                // .with_year(event.when.year())
                // .with_month(event.when.day())
                // .with_day(event.when.month())
                .into_iter()
                .filter(|(_r, e)| {
                    std::mem::discriminant(&e.value) == std::mem::discriminant(&event.value)
                })
                .next();
            if let Some((r, _e)) = existing {
                tx.update(&r, event)?;
            } else {
                tx.insert(event)?;
            }
        }
        Ok(())
    }
}

#[derive(Debug, structsy::derive::Persistent, Serialize, Deserialize, PartialEq)]
pub struct Ring {
    pub nickname: Option<String>,
    pub name: String,
    #[index(mode = "exclusive")]
    pub mac: String,
}

#[queries(Ring)]
trait FindRingByMac {
    // here is our condition method, to notice that the name of the parameter has to be exactly the same of the struct field.
    fn with_mac(self, mac: &str) -> Self;
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
#[serde(transparent)]
pub struct DateTime(OffsetDateTime);

impl PersistentEmbedded for DateTime {
    fn write(&self, write: &mut dyn std::io::Write) -> structsy::SRes<()> {
        let timestamp = self.0.unix_timestamp();
        let bytes = timestamp.to_be_bytes();
        Ok(write.write_all(bytes.as_slice())?)
    }

    fn read(read: &mut dyn std::io::Read) -> structsy::SRes<Self>
    where
        Self: Sized {
        let mut bytes = [0u8;8];
        read.read_exact(&mut bytes).map_err(|e| {
            structsy::StructsyError::TypeError("EOF".to_string())
        })?;
        let timestamp = i64::from_be_bytes(bytes);
        Ok(Self(OffsetDateTime::from_unix_timestamp(timestamp).map_err(|e| {
            structsy::StructsyError::TypeError(format!("invalid timestamp: {timestamp}"))
        })?))
    }
}

impl EmbeddedDescription for DateTime {
    fn get_description() -> structsy::internal::Description {
        structsy::internal::Description::Struct(StructDescription::new("DateTime", &[
            FieldDescription::new::<u16>(0, "timestamp", Some(structsy::ValueMode::Cluster)),
        ]))
    }
}

#[derive(Debug, structsy::derive::Persistent, Serialize, Deserialize, PartialEq)]
pub struct RingEvent {
    #[index(mode = "cluster")]
    pub mac: String,
    pub when: DateTime,
    pub value: EventData,
}



#[derive(Debug, structsy::derive::PersistentEmbedded, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", content = "data")]
pub enum EventData {
    HeartRate(u16),
    Sleep(u16),
    Stress(u16),
    Oxygen(u16),
    Activity(Activity),
}

#[derive(Debug, structsy::derive::PersistentEmbedded, Serialize, Deserialize, PartialEq)]
pub struct Activity {
    pub steps: u8,
    pub calories: f64,
    pub distance: u8,
}


#[queries(RingEvent)]
trait FindEventByMac {
    fn with_ring_mac(self, mac: &str) -> Self;
    fn between_time<R:RangeBounds<DateTime>>(self, when:R) -> Self;
}

#[cfg(test)]
mod tests {
    use time::{Time, Date};

    use super::*;

    #[test]
    fn add_rings() {
        let db = Database::test().unwrap();
        let ring1 = Ring {
            mac: "00:00:00:00:00:00".to_string(),
            nickname: None,
            name: "ring1".to_string()
        };
        let ring2 = Ring {
            mac: "ff:00:00:00:00:00".to_string(),
            nickname: None,
            name: "ring2".to_string()
        };
        db.add_ring(&ring1).unwrap();
        db.add_ring(&ring2).unwrap();
        let from_db = db.get_rings();
        assert_eq!(from_db.len(), 2, "Invalid length of rings {from_db:?}");
        assert_eq!(from_db.as_slice(), [ring1, ring2].as_slice());
    }

    #[test]
    fn add_ring() {
        let db = Database::test().unwrap();
        let ring = Ring {
            mac: "00:00:00:00:00:00".to_string(),
            nickname: None,
            name: "name".to_string()
        };
        db.add_ring(&ring).unwrap();
        let from_db = db.get_ring(&ring.mac).unwrap();
        assert_eq!(from_db, ring);
    }

    #[test]
    fn serde_events() {
        let events = [
            RingEvent {
                mac: "00:00:00:00:00:00".to_string(),
                when: DateTime(OffsetDateTime::new_utc(Date::from_calendar_date(2001, time::Month::January, 31).unwrap(), Time::from_hms(0, 0, 0).unwrap())),
                value: super::EventData::Activity(Activity {
                    steps: 11,
                    calories: 222.0,
                    distance: 88,
                }),
            },
            RingEvent {
                mac: "00:00:00:00:00:00".to_string(),
                when: DateTime(OffsetDateTime::new_utc(Date::from_calendar_date(2001, time::Month::January, 31).unwrap(), Time::from_hms(0, 0, 0).unwrap())),
                value: super::EventData::HeartRate(90),
            },
            RingEvent {
                mac: "00:00:00:00:00:00".to_string(),
                when: DateTime(OffsetDateTime::new_utc(Date::from_calendar_date(2001, time::Month::January, 31).unwrap(), Time::from_hms(0, 0, 0).unwrap())),
                value: super::EventData::Oxygen(11),
            },
            RingEvent {
                mac: "00:00:00:00:00:00".to_string(),
                when: DateTime(OffsetDateTime::new_utc(Date::from_calendar_date(2001, time::Month::January, 31).unwrap(), Time::from_hms(0, 0, 0).unwrap())),
                value: super::EventData::Sleep(99),
            },
            RingEvent {
                mac: "00:00:00:00:00:00".to_string(),
                when: DateTime(OffsetDateTime::new_utc(Date::from_calendar_date(2001, time::Month::January, 31).unwrap(), Time::from_hms(0, 0, 0).unwrap())),
                value: super::EventData::Stress(0),
            },
        ];
        let json = serde_json::to_string_pretty(&events).unwrap();
        let back: Vec<RingEvent> = serde_json::from_str(&json).unwrap();
        assert_eq!(events.as_slice(), back.as_slice());
        insta::assert_snapshot!(json);
    }
}
