//! Parse human-readable time periods into date ranges.

use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveTime, TimeZone, Utc};

/// A resolved time range.
#[derive(Debug, Clone)]
pub struct TimePeriod {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub label: String,
}

/// Parse a period string into a UTC time range.
///
/// Supported formats:
/// - "today" — midnight to midnight (UTC)
/// - "yesterday"
/// - "this-week" — Monday to now
/// - "last-week" — previous Monday to Sunday
/// - "this-month"
/// - "last-month"
/// - "YYYY-MM-DD" — a single day
/// - "YYYY-MM-DD..YYYY-MM-DD" — explicit range
pub fn parse_period(period: &str) -> anyhow::Result<TimePeriod> {
    let now = Utc::now();
    let today = now.date_naive();

    match period.to_lowercase().as_str() {
        "today" => {
            let start = day_start(today);
            Ok(TimePeriod {
                start,
                end: now,
                label: "today".to_string(),
            })
        }
        "yesterday" => {
            let yesterday = today - Duration::days(1);
            Ok(TimePeriod {
                start: day_start(yesterday),
                end: day_start(today),
                label: "yesterday".to_string(),
            })
        }
        "this-week" | "this_week" => {
            let monday = today - Duration::days(today.weekday().num_days_from_monday() as i64);
            Ok(TimePeriod {
                start: day_start(monday),
                end: now,
                label: "this week".to_string(),
            })
        }
        "last-week" | "last_week" => {
            let this_monday = today - Duration::days(today.weekday().num_days_from_monday() as i64);
            let last_monday = this_monday - Duration::days(7);
            Ok(TimePeriod {
                start: day_start(last_monday),
                end: day_start(this_monday),
                label: "last week".to_string(),
            })
        }
        "this-month" | "this_month" => {
            let first = NaiveDate::from_ymd_opt(today.year(), today.month(), 1)
                .ok_or_else(|| anyhow::anyhow!("invalid date"))?;
            Ok(TimePeriod {
                start: day_start(first),
                end: now,
                label: "this month".to_string(),
            })
        }
        "last-month" | "last_month" => {
            let first_this = NaiveDate::from_ymd_opt(today.year(), today.month(), 1)
                .ok_or_else(|| anyhow::anyhow!("invalid date"))?;
            let first_last = if today.month() == 1 {
                NaiveDate::from_ymd_opt(today.year() - 1, 12, 1)
            } else {
                NaiveDate::from_ymd_opt(today.year(), today.month() - 1, 1)
            }
            .ok_or_else(|| anyhow::anyhow!("invalid date"))?;
            Ok(TimePeriod {
                start: day_start(first_last),
                end: day_start(first_this),
                label: "last month".to_string(),
            })
        }
        other => {
            // Try explicit range: YYYY-MM-DD..YYYY-MM-DD
            if let Some((start_str, end_str)) = other.split_once("..") {
                let start_date = NaiveDate::parse_from_str(start_str.trim(), "%Y-%m-%d")?;
                let end_date = NaiveDate::parse_from_str(end_str.trim(), "%Y-%m-%d")?;
                return Ok(TimePeriod {
                    start: day_start(start_date),
                    end: day_start(end_date + Duration::days(1)),
                    label: format!("{start_str} to {end_str}"),
                });
            }

            // Try single date: YYYY-MM-DD
            let date = NaiveDate::parse_from_str(other.trim(), "%Y-%m-%d")
                .map_err(|e| anyhow::anyhow!("unrecognized period '{other}': {e}"))?;
            Ok(TimePeriod {
                start: day_start(date),
                end: day_start(date + Duration::days(1)),
                label: other.to_string(),
            })
        }
    }
}

fn day_start(date: NaiveDate) -> DateTime<Utc> {
    Utc.from_utc_datetime(&date.and_time(NaiveTime::MIN))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_today() {
        let tp = parse_period("today").unwrap();
        assert_eq!(tp.label, "today");
        assert!(tp.start < tp.end);
    }

    #[test]
    fn test_parse_single_date() {
        let tp = parse_period("2026-03-04").unwrap();
        assert_eq!(tp.label, "2026-03-04");
        let diff = tp.end - tp.start;
        assert_eq!(diff.num_hours(), 24);
    }

    #[test]
    fn test_parse_range() {
        let tp = parse_period("2026-03-01..2026-03-04").unwrap();
        let diff = tp.end - tp.start;
        assert_eq!(diff.num_days(), 4);
    }

    #[test]
    fn test_parse_invalid() {
        assert!(parse_period("not-a-date").is_err());
    }
}
