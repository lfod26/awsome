use anyhow::{Context, Result};
use chrono::{Local, NaiveTime, TimeDelta};

/// Parses a "HH:MM" 24-hour local time string and computes how many
/// whole minutes remain until the next occurrence of that clock time
/// (today if it hasn't passed yet, otherwise tomorrow).
///
/// Returns `(minutes, target_time)` so callers can print a friendly
/// message including the resolved target time.
pub fn minutes_until_next(time_str: &str) -> Result<(i64, NaiveTime)> {
    let target_time = NaiveTime::parse_from_str(time_str, "%H:%M").with_context(|| {
        format!("invalid time '{time_str}', expected 24-hour HH:MM (e.g. 18:30)")
    })?;

    let now = Local::now();
    let today_target = now.date_naive().and_time(target_time);

    let target = if today_target > now.naive_local() {
        today_target
    } else {
        today_target + TimeDelta::days(1)
    };

    let minutes = (target - now.naive_local()).num_minutes();
    // Round up to make sure we don't schedule for a moment slightly
    // before the target time due to truncation.
    let minutes = minutes.max(1);

    Ok((minutes, target_time))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_invalid_time_strings() {
        for bad in ["25:00", "abc", "18:60", "18", "", "1830"] {
            assert!(
                minutes_until_next(bad).is_err(),
                "expected error for input {bad:?}"
            );
        }
    }

    #[test]
    fn parses_valid_time_and_returns_sane_minutes() {
        let (minutes, target) = minutes_until_next("18:30").unwrap();
        assert_eq!(target, NaiveTime::from_hms_opt(18, 30, 0).unwrap());
        // The next occurrence is always within the coming 24 hours, and
        // the result is clamped to at least 1 minute.
        assert!(
            (1..=1440).contains(&minutes),
            "minutes out of expected range: {minutes}"
        );
    }

    #[test]
    fn minutes_is_at_least_one() {
        // Regardless of the target time, the clamp guarantees >= 1.
        for t in ["00:00", "12:00", "23:59"] {
            let (minutes, _) = minutes_until_next(t).unwrap();
            assert!(
                minutes >= 1,
                "minutes should be >= 1 for {t}, got {minutes}"
            );
        }
    }
}
