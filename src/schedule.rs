//! Minute-resolution calendar schedules with explicit IANA time zones.
use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use croner::parser::{CronParser, Seconds, Year};
use croner::Cron;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CronSchedule {
    pub expression: String,
    pub timezone: String,
}

impl CronSchedule {
    pub fn new(expression: &str, timezone: &str) -> Result<Self> {
        if expression.len() > 256 {
            bail!("cron expression must be at most 256 bytes");
        }
        let schedule = Self {
            expression: expression.split_whitespace().collect::<Vec<_>>().join(" "),
            timezone: timezone.into(),
        };
        schedule.parse()?;
        Ok(schedule)
    }

    fn parse(&self) -> Result<(Cron, Tz)> {
        // Keep a bounded, conventional five-field interface. Do not silently accept
        // seconds, years, nicknames or library-specific calendar extensions.
        if self.expression.len() > 256 {
            bail!("cron expression must be at most 256 bytes");
        }
        let fields: Vec<_> = self.expression.split_whitespace().collect();
        if fields.len() != 5 {
            bail!("cron requires five fields: minute hour day-of-month month day-of-week");
        }
        const MONTHS: [&str; 12] = [
            "JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP", "OCT", "NOV", "DEC",
        ];
        const DAYS: [&str; 7] = ["SUN", "MON", "TUE", "WED", "THU", "FRI", "SAT"];
        for (index, field) in fields.iter().enumerate() {
            if !field.bytes().all(|byte| {
                byte.is_ascii_digit() || byte.is_ascii_alphabetic() || b"*,-/".contains(&byte)
            }) {
                bail!("unsupported cron syntax in field {}", index + 1);
            }
            for name in field.split(|character: char| !character.is_ascii_alphabetic()) {
                if name.is_empty() {
                    continue;
                }
                let name = name.to_ascii_uppercase();
                let allowed = match index {
                    3 => MONTHS.contains(&name.as_str()),
                    4 => DAYS.contains(&name.as_str()),
                    _ => false,
                };
                if !allowed {
                    bail!("unsupported cron name {name} in field {}", index + 1);
                }
            }
        }
        let cron = CronParser::builder()
            .seconds(Seconds::Disallowed)
            .year(Year::Disallowed)
            .build()
            .parse(&self.expression)
            .context("invalid cron expression")?;
        if self.timezone.len() > 64 {
            bail!("timezone must be an IANA name, e.g. Asia/Tokyo or UTC");
        }
        let zone = self
            .timezone
            .parse::<Tz>()
            .context("invalid IANA timezone; use e.g. Asia/Tokyo or UTC")?;
        Ok((cron, zone))
    }

    /// Return the first occurrence strictly after an absolute Unix millisecond time.
    /// Croner's Vixie rules move fixed times in a DST gap to its end, and run
    /// fixed times once during a repeated hour. Wildcards follow each real minute.
    pub fn next_after(&self, after: u64) -> Result<u64> {
        let (cron, zone) = self.parse()?;
        let millis = i64::try_from(after).context("cron timestamp is out of range")?;
        let start = DateTime::<Utc>::from_timestamp_millis(millis)
            .context("cron timestamp is out of range")?
            .with_timezone(&zone);
        let next = cron
            .find_next_occurrence(&start, false)
            .context("cron has no future occurrence in the supported calendar")?;
        let next = u64::try_from(next.timestamp_millis())?;
        if next <= after {
            bail!("cron did not produce a future occurrence");
        }
        Ok(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn time(value: &str) -> u64 {
        DateTime::parse_from_rfc3339(value)
            .unwrap()
            .timestamp_millis() as u64
    }
    fn next(expression: &str, zone: &str, after: &str) -> u64 {
        CronSchedule::new(expression, zone)
            .unwrap()
            .next_after(time(after))
            .unwrap()
    }

    #[test]
    fn tokyo_weekday_morning_and_strict_future_boundary() {
        assert_eq!(
            next("0 9 * * MON-FRI", "Asia/Tokyo", "2026-10-09T00:00:00Z"),
            time("2026-10-12T00:00:00Z")
        );
        assert_eq!(
            next("0 9 * * *", "Asia/Tokyo", "2026-10-08T00:00:00.001Z"),
            time("2026-10-09T00:00:00Z")
        );
        assert_eq!(
            next("0 9 * * *", "UTC", "2026-10-08T00:00:00Z"),
            time("2026-10-08T09:00:00Z")
        );
        assert_eq!(
            next("* * * * *", "UTC", "2026-10-08T00:00:59.999Z"),
            time("2026-10-08T00:01:00Z")
        );
    }

    #[test]
    fn lists_steps_leap_day_and_day_fields_use_standard_or() {
        assert_eq!(
            next("*/15 9-17 * * *", "UTC", "2026-10-08T09:15:00Z"),
            time("2026-10-08T09:30:00Z")
        );
        assert_eq!(
            next("0 9 29 FEB *", "UTC", "2026-10-08T00:00:00Z"),
            time("2028-02-29T09:00:00Z")
        );
        assert_eq!(
            next("0 9 1 * MON", "UTC", "2026-10-08T00:00:00Z"),
            time("2026-10-12T09:00:00Z")
        );
        assert_eq!(
            next("0,30 9 * * 0,7", "UTC", "2026-10-10T00:00:00Z"),
            time("2026-10-11T09:00:00Z")
        );
    }

    #[test]
    fn dst_fixed_times_move_to_gap_end_and_do_not_repeat_in_overlap() {
        assert_eq!(
            next("30 2 * * *", "America/New_York", "2026-03-08T06:00:00Z"),
            time("2026-03-08T07:00:00Z")
        );
        assert_eq!(
            next("30 1 * * *", "America/New_York", "2026-11-01T04:00:00Z"),
            time("2026-11-01T05:30:00Z")
        );
        assert_eq!(
            next("30 1 * * *", "America/New_York", "2026-11-01T05:30:00Z"),
            time("2026-11-02T06:30:00Z")
        );
        assert_eq!(
            next("* * * * *", "America/New_York", "2026-11-01T05:59:00Z"),
            time("2026-11-01T06:00:00Z")
        );
    }

    #[test]
    fn invalid_fields_extensions_timezones_and_impossible_dates_fail() {
        for expression in [
            "",
            "* * * *",
            "0 * * * * *",
            "@daily",
            "60 9 * * *",
            "0 24 * * *",
            "0 9 * 13 *",
            "0 9 * * 8",
            "*/0 * * * *",
            "0 9 L * *",
            "0 9 * * MON#2",
            "0 9 ? * *",
            "0 9 * * +MON",
        ] {
            assert!(
                CronSchedule::new(expression, "UTC").is_err(),
                "{expression}"
            );
        }
        assert!(CronSchedule::new("* * * * *", "JST").is_err());
        assert!(CronSchedule::new("* * * * *", "Asia/Typo").is_err());
        assert!(CronSchedule::new("0 9 31 FEB *", "UTC")
            .unwrap()
            .next_after(time("2026-10-08T00:00:00Z"))
            .is_err());
        assert!(CronSchedule::new("* * * * *", "UTC")
            .unwrap()
            .next_after(u64::MAX)
            .is_err());
    }
}
