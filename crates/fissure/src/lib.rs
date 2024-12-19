//! Database Abstractions
//! 

use std::{ops::RangeBounds, path::Path};

use date::DateTime;
use serde::{Deserialize, Serialize};
use structsy::{
    derive::queries,
    Filter, Operators, Structsy, StructsyTx,
};
use time::OffsetDateTime;
use crate::date::DateTimeQuery;

mod date;

type Result<T = (), E = Box<dyn std::error::Error>> = std::result::Result<T, E>;

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

    pub fn get_events_for_ring(&self, mac: &str, when: OffsetDateTime) -> Result<Vec<RingEvent>> {
        let min = when.date().midnight().assume_utc();
        let max = min
            .date()
            .next_day()
            .ok_or_else(|| format!("Missing next day {min}"))?
            .midnight()
            .assume_utc();
        let min = DateTime::try_from(min)?;
        let max = DateTime::try_from(max)?;
        let q = self
            .0
            .query::<RingEvent>()
            .with_ring_mac(mac)
            .and(|and| and.between_time(min..max));

        Ok(q.into_iter().map(|(_, event)| event).collect())
    }

    pub fn add_events(&self, events: &[RingEvent]) -> Result<()> {
        let mut tx = self.0.begin()?;

        for event in events {
            let existing = tx
                .query::<RingEvent>()
                .with_ring_mac(&event.mac)
                .and(|and| {
                    let filter = Filter::<DateTime>::new()
                        .with_ymd(event.when.year, event.when.month, event.when.day)
                        .with_hms(event.when.hour, event.when.minute, event.when.second);
                    and.with_when(filter)
                })
                .into_iter()
                .filter(|(_r, e)| {
                    std::mem::discriminant(&e.value) == std::mem::discriminant(&event.value)
                })
                .next();
            if let Some((r, _e)) = existing {
                println!("found matching event\n{event:?}\n{_e:?}");
                tx.update(&r, event)?;
            } else {
                tx.insert(event)?;
            }
        }
        tx.commit()?;
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

#[derive(
    Debug,
    structsy::derive::Persistent,
    Serialize,
    Deserialize,
    PartialEq,
    bon::Builder,
)]
pub struct RingEvent {
    #[builder(into)]
    #[index(mode = "cluster")]
    pub mac: String,
    #[builder(into)]
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

impl EventData {
    pub fn activity(steps: u8, calories: f64, distance: u8) -> Self {
        EventData::Activity(Activity {
            steps,
            calories,
            distance,
        })
    }
    pub fn oxygen(value: u16) -> Self {
        EventData::Oxygen(value)
    }
    pub fn sleep(value: u16) -> Self {
        EventData::Sleep(value)
    }
    pub fn stress(value: u16) -> Self {
        EventData::Stress(value)
    }
    pub fn heart_rate(value: u16) -> Self {
        EventData::HeartRate(value)
    }
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
    fn with_when(self, when: Filter<DateTime>) -> Self;
    fn between_time<R: RangeBounds<DateTime>>(self, when: R) -> Self;
}

#[cfg(test)]
mod tests {
    use std::{sync::atomic::AtomicUsize, time::Duration};

    use time::{Date, Month, Time};

    use super::*;

    static MAC: &str = "00:00:00:00:00:00";
    static MAC2: &str = "00:00:00:00:00:02";

    #[test]
    fn add_rings() {
        let db = Database::test().unwrap();
        let ring1 = Ring {
            mac: MAC.to_string(),
            nickname: None,
            name: "ring1".to_string(),
        };
        let ring2 = Ring {
            mac: MAC2.to_string(),
            nickname: None,
            name: "ring2".to_string(),
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
            mac: MAC.to_string(),
            nickname: None,
            name: "name".to_string(),
        };
        db.add_ring(&ring).unwrap();
        let from_db = db.get_ring(&ring.mac).unwrap();
        assert_eq!(from_db, ring);
    }

    #[test]
    fn serde_events() {
        let events = [
            RingEvent::builder()
                .mac(MAC)
                .when(DateTime::builder().year(2001).month(1).day(31).build())
                .value(EventData::activity(11, 222.0, 88))
                .build(),
            RingEvent::builder()
                .mac(MAC)
                .when(DateTime::builder().year(2001).month(1).day(31).build())
                .value(EventData::heart_rate(90))
                .build(),
            RingEvent::builder()
                .mac(MAC)
                .when(DateTime::builder().year(2001).month(1).day(31).build())
                .value(EventData::oxygen(11))
                .build(),
            RingEvent::builder()
                .mac(MAC)
                .when(DateTime::builder().year(2001).month(1).day(31).build())
                .value(EventData::Sleep(0))
                .build(),
            RingEvent::builder()
                .mac(MAC)
                .when(DateTime::builder().year(2001).month(1).day(31).build())
                .value(EventData::Stress(0))
                .build(),
        ];
        let json = serde_json::to_string_pretty(&events).unwrap();
        let back: Vec<RingEvent> = serde_json::from_str(&json).unwrap();
        assert_eq!(events.as_slice(), back.as_slice());
        insta::assert_snapshot!(json);
    }

    #[test]
    fn no_data_loss() {
        let db = Database::test().unwrap();
        let mut events = Vec::new();
        let mut time = OffsetDateTime::new_utc(
            Date::from_calendar_date(2001, time::Month::January, 31).unwrap(),
            Time::from_hms(0, 0, 0).unwrap(),
        );
        for i in 0..48 {
            events.push(RingEvent {
                mac: MAC.to_string(),
                when: time.try_into().unwrap(),
                value: super::EventData::Stress(i),
            });
            time += Duration::from_secs(60 * 60);
        }

        db.add_events(&events).unwrap();
        let from_db: Vec<_> =
            db.0.query::<RingEvent>()
                .fetch()
                .into_iter()
                .map(|(_, e)| e)
                .collect();
        assert_eq!(from_db, events)
    }

    #[test]
    fn time_search_works() {
        // const MAC: &str = "00:00:00:00:00:00";
        // let db = Database::test().unwrap();
        // let mut jan_events = Vec::new();
        // let mut feb_events = Vec::new();
        // let mut time = OffsetDateTime::new_utc(
        //     Date::from_calendar_date(2001, time::Month::January, 31).unwrap(),
        //     Time::from_hms(0, 0, 0).unwrap(),
        // );
        // let start = time;
        // while time.month() == Month::January {
        //     jan_events.push(RingEvent {
        //         mac: MAC.to_string(),
        //         when: DateTime(time),
        //         value: super::EventData::Stress(jan_events.len() as _),
        //     });
        //     time += Duration::from_secs(60 * 60);
        // }
        // for i in 0..24 {
        //     feb_events.push(RingEvent {
        //         mac: MAC.to_string(),
        //         when: DateTime(time),
        //         value: super::EventData::Stress(i),
        //     });
        //     time += Duration::from_secs(60 * 60);
        // }

        // db.add_events(&jan_events).unwrap();
        // db.add_events(&feb_events).unwrap();
        // let from_db = db.get_events_for_ring(MAC, start).unwrap();
        // assert_eq!(from_db, jan_events)
    }
}
