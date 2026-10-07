//! The day of the run, for `{build-date}` and `{year}`.
//!
//! It is the UTC day of the system clock, or of `SOURCE_DATE_EPOCH` when set, so builds that must give
//! the same bytes can fix it.

use std::time::{SystemTime, UNIX_EPOCH};

use crate::config::theme::Lang;

const MONTHS_EN: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
const MONTHS_DE: [&str; 12] = [
    "Januar",
    "Februar",
    "März",
    "April",
    "Mai",
    "Juni",
    "Juli",
    "August",
    "September",
    "Oktober",
    "November",
    "Dezember",
];

/// A day of the proleptic Gregorian calendar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Date {
    pub year: i64,
    /// 1 to 12.
    pub month: u8,
    /// 1 to 31.
    pub day: u8,
}

impl Date {
    /// The UTC day of `SOURCE_DATE_EPOCH`, if set, else of the system clock.
    pub fn today() -> Result<Self, String> {
        let seconds = match std::env::var("SOURCE_DATE_EPOCH") {
            Ok(value) => value.trim().parse::<i64>().map_err(|_| {
                format!("SOURCE_DATE_EPOCH must be a whole number of seconds since 1970, not \"{value}\"")
            })?,
            Err(_) => {
                let elapsed = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_err(|_| "the system clock is set before 1970".to_owned())?;
                i64::try_from(elapsed.as_secs()).map_err(|_| "the system clock is out of range".to_owned())?
            }
        };
        Ok(Self::from_unix(seconds))
    }

    /// The UTC day of a Unix time, by Howard Hinnant's `civil_from_days`.
    pub fn from_unix(seconds: i64) -> Self {
        let days = seconds.div_euclid(86_400) + 719_468;
        let era = days.div_euclid(146_097);
        let day_of_era = days - era * 146_097;
        let year_of_era = (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
        let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
        let shifted_month = (5 * day_of_year + 2) / 153;
        let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
        let month = if shifted_month < 10 {
            shifted_month + 3
        } else {
            shifted_month - 9
        };
        let year = year_of_era + era * 400 + i64::from(month <= 2);
        Self {
            year,
            month: month as u8,
            day: day as u8,
        }
    }

    /// The date written out in `lang`: "7 October 2026" or "7. Oktober 2026".
    pub fn long(&self, lang: Lang) -> String {
        let index = usize::from(self.month - 1);
        match lang {
            Lang::En => format!("{} {} {}", self.day, MONTHS_EN[index], self.year),
            Lang::De => format!("{}.\u{a0}{} {}", self.day, MONTHS_DE[index], self.year),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_unix_times_to_utc_days() {
        let date = |year, month, day| Date { year, month, day };
        assert_eq!(Date::from_unix(0), date(1970, 1, 1));
        assert_eq!(Date::from_unix(-1), date(1969, 12, 31));
        assert_eq!(Date::from_unix(951_782_400), date(2000, 2, 29));
        assert_eq!(Date::from_unix(1_791_417_599), date(2026, 10, 7));
        assert_eq!(Date::from_unix(1_791_417_600), date(2026, 10, 8));
    }

    #[test]
    fn writes_the_date_out_by_language() {
        let date = Date::from_unix(1_791_331_200);
        assert_eq!(date.long(Lang::En), "7 October 2026");
        assert_eq!(date.long(Lang::De), "7.\u{a0}Oktober 2026");
    }
}
