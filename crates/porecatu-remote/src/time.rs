// SPDX-License-Identifier: GPL-3.0-or-later

//! Clock and RFC 3339 UTC dates.
//!
//! The protocol calls take a monotonic millisecond counter ([`Clock`]); the
//! dates written to `remote_devices.json` and to quarantine file names are
//! civil UTC dates computed here, without a date crate.

use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// Monotonic millisecond clock for the protocol state machines. Zero is the
/// moment of creation; the value never goes back.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Clock {
    origin: Instant,
}

impl Clock {
    pub(crate) fn new() -> Self {
        Self {
            origin: Instant::now(),
        }
    }

    /// Milliseconds since [`Clock::new`], saturating.
    pub(crate) fn now_ms(&self) -> u64 {
        u64::try_from(self.origin.elapsed().as_millis()).unwrap_or(u64::MAX)
    }
}

/// Broken-down UTC date and time, whole seconds.
struct Civil {
    year: i64,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
}

/// Seconds since the Unix epoch; instants before it clamp to zero.
fn unix_seconds(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Civil date from days since 1970-01-01 (Howard Hinnant's `civil_from_days`).
fn civil(t: SystemTime) -> Civil {
    let secs = unix_seconds(t);
    let days = i64::try_from(secs / 86_400).unwrap_or(i64::MAX / 2);
    let rem = u32::try_from(secs % 86_400).unwrap_or(0);

    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = u32::try_from(doy - (153 * mp + 2) / 5 + 1).unwrap_or(1);
    let month = u32::try_from(if mp < 10 { mp + 3 } else { mp - 9 }).unwrap_or(1);
    let year = yoe + era * 400 + i64::from(month <= 2);

    Civil {
        year,
        month,
        day,
        hour: rem / 3_600,
        minute: rem % 3_600 / 60,
        second: rem % 60,
    }
}

/// `"2026-11-15T14:03:00Z"`: UTC, whole seconds.
pub(crate) fn rfc3339_utc(t: SystemTime) -> String {
    let c = civil(t);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        c.year, c.month, c.day, c.hour, c.minute, c.second
    )
}

/// `"20261115-140300"`: the suffix of a quarantine file name.
pub(crate) fn file_stamp(t: SystemTime) -> String {
    let c = civil(t);
    format!(
        "{:04}{:02}{:02}-{:02}{:02}{:02}",
        c.year, c.month, c.day, c.hour, c.minute, c.second
    )
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn at(secs: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(secs)
    }

    #[test]
    fn the_epoch() {
        assert_eq!(rfc3339_utc(at(0)), "1970-01-01T00:00:00Z");
        assert_eq!(file_stamp(at(0)), "19700101-000000");
    }

    #[test]
    fn a_leap_day() {
        // 2024-02-29T23:59:59Z
        assert_eq!(rfc3339_utc(at(1_709_251_199)), "2024-02-29T23:59:59Z");
        // One second later is March 1st.
        assert_eq!(rfc3339_utc(at(1_709_251_200)), "2024-03-01T00:00:00Z");
    }

    #[test]
    fn century_rules() {
        // 2000-02-29 exists (divisible by 400); 2100-02-29 does not.
        assert_eq!(rfc3339_utc(at(951_782_400)), "2000-02-29T00:00:00Z");
        assert_eq!(rfc3339_utc(at(4_107_542_400)), "2100-03-01T00:00:00Z");
    }

    #[test]
    fn a_known_instant() {
        // 2026-11-15T14:03:00Z
        assert_eq!(rfc3339_utc(at(1_794_751_380)), "2026-11-15T14:03:00Z");
        assert_eq!(file_stamp(at(1_794_751_380)), "20261115-140300");
    }

    #[test]
    fn an_instant_before_the_epoch_clamps_to_it() {
        assert_eq!(
            rfc3339_utc(UNIX_EPOCH - Duration::from_secs(5)),
            "1970-01-01T00:00:00Z"
        );
    }

    #[test]
    fn the_clock_never_goes_back() {
        let clock = Clock::new();
        let first = clock.now_ms();
        std::thread::sleep(Duration::from_millis(5));
        assert!(clock.now_ms() >= first + 5);
    }
}
