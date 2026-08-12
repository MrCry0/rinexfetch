//! `latest` / explicit-datetime resolution to a GPS day.
//!
//! Only day-level resolution is implemented here (Phase 1 of the project
//! plan). CDDIS product-tier fallback (final / rapid for `--time latest`)
//! and any sub-day session refinement happen later, during path discovery
//! (Phase 3), since they depend on what CDDIS actually has published
//! rather than on the input time alone.

use hifitime::{Epoch, HifitimeError, NANOSECONDS_PER_DAY, TimeScale};
use std::str::FromStr;

#[derive(Debug, thiserror::Error)]
pub enum TimeError {
    #[error("could not read system clock: {0}")]
    SystemClock(#[source] HifitimeError),
    #[error("invalid --time value {value:?}: {source}")]
    InvalidTimestamp {
        value: String,
        #[source]
        source: HifitimeError,
    },
}

/// A single UTC calendar day, resolved to its GPS week / day-of-week /
/// day-of-year, as needed to locate CDDIS daily products.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GpsDay {
    /// This calendar day at UTC midnight.
    pub date_utc: Epoch,
    pub year: i32,
    pub day_of_year: u16,
    /// Rolling GPS week number (not mod-1024).
    pub gps_week: u32,
    /// Day within the GPS week: 0 = Sunday .. 6 = Saturday. GPS weeks start
    /// at Sunday midnight, which is a different convention from
    /// [`hifitime::Weekday`]'s ISO numbering (Monday = 0).
    pub gps_day_of_week: u8,
}

impl GpsDay {
    /// Resolves `"latest"` or an explicit ISO 8601 timestamp to the UTC
    /// calendar day it falls on. `"latest"` anchors to today; discovery
    /// (Phase 3) walks backward from there to the most recent day that
    /// actually has a published product.
    pub fn resolve(time_arg: &str) -> Result<Self, TimeError> {
        let epoch = match time_arg {
            "latest" => Epoch::now().map_err(TimeError::SystemClock)?,
            explicit => {
                Epoch::from_str(explicit).map_err(|source| TimeError::InvalidTimestamp {
                    value: explicit.to_string(),
                    source,
                })?
            }
        };
        Ok(Self::from_epoch(epoch))
    }

    fn from_epoch(epoch: Epoch) -> Self {
        let (year, month, day, ..) = epoch.to_gregorian_utc();
        let date_utc = Epoch::from_gregorian_utc_at_midnight(year, month, day);

        let day_of_year = date_utc.day_of_year().round() as u16;

        let (gps_week, ns_into_week) = date_utc.to_time_scale(TimeScale::GPST).to_time_of_week();
        let gps_day_of_week = (ns_into_week / NANOSECONDS_PER_DAY) as u8;

        Self {
            date_utc,
            year,
            day_of_year,
            gps_week,
            gps_day_of_week,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_explicit_date_to_gps_day() {
        // 2024-01-06 was a Saturday, the last day of GPS week 2295.
        let day = GpsDay::resolve("2024-01-06T00:00:00Z").unwrap();
        assert_eq!(day.year, 2024);
        assert_eq!(day.day_of_year, 6);
        assert_eq!(day.gps_week, 2295);
        assert_eq!(day.gps_day_of_week, 6);
    }

    #[test]
    fn resolves_explicit_date_at_week_boundary() {
        // 2024-01-07 was a Sunday: the first day of the next GPS week.
        let day = GpsDay::resolve("2024-01-07").unwrap();
        assert_eq!(day.gps_week, 2296);
        assert_eq!(day.gps_day_of_week, 0);
    }

    #[test]
    fn explicit_timestamp_truncates_to_utc_midnight() {
        let day = GpsDay::resolve("2024-06-15T13:45:00Z").unwrap();
        let (_, _, _, hour, minute, second, _) = day.date_utc.to_gregorian_utc();
        assert_eq!((hour, minute, second), (0, 0, 0));
        assert_eq!(day.day_of_year, 167);
    }

    #[test]
    fn latest_resolves_to_todays_utc_midnight() {
        let latest = GpsDay::resolve("latest").unwrap();
        let (year, month, day, ..) = Epoch::now().unwrap().to_gregorian_utc();
        assert_eq!(
            latest.date_utc,
            Epoch::from_gregorian_utc_at_midnight(year, month, day)
        );
    }

    #[test]
    fn rejects_garbage_input() {
        assert!(GpsDay::resolve("not-a-date").is_err());
    }
}
