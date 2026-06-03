use chrono::{
    DateTime,
    Utc,
};
use crate::dates::{
    str2dt,
};
use eyre::Error;

#[derive(Debug)]
pub struct TimeRange {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
}

impl TimeRange {
    pub fn new(start: DateTime<Utc>, end: DateTime<Utc>) -> Result<TimeRange, Error> {
        if start > end {
            return Err(Error::msg("start time after end"));
        }

        Ok(Self {start, end})
    }

    pub fn new_from_strs(start: &str, end: &str) -> Result<TimeRange, Error> {
        let start_dt = str2dt(start)?;
        let end_dt = str2dt(end)?;

        Self::new(start_dt, end_dt)
    }
}
