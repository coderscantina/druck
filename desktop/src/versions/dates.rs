//! Dates in the author's language and time zone, the way the lists and the timeline word them.

use jiff::Timestamp;
use jiff::civil::Weekday;
use jiff::tz::TimeZone;

use crate::shell::strings::{Language, language};

const WEEKDAYS: [(&str, &str); 7] = [
    ("Monday", "Montag"),
    ("Tuesday", "Dienstag"),
    ("Wednesday", "Mittwoch"),
    ("Thursday", "Donnerstag"),
    ("Friday", "Freitag"),
    ("Saturday", "Samstag"),
    ("Sunday", "Sonntag"),
];
const MONTHS: [(&str, &str); 12] = [
    ("Jan", "Jan."),
    ("Feb", "Feb."),
    ("Mar", "März"),
    ("Apr", "Apr."),
    ("May", "Mai"),
    ("Jun", "Juni"),
    ("Jul", "Juli"),
    ("Aug", "Aug."),
    ("Sep", "Sep."),
    ("Oct", "Okt."),
    ("Nov", "Nov."),
    ("Dec", "Dez."),
];

fn word(pair: (&'static str, &'static str)) -> &'static str {
    match language() {
        Language::English => pair.0,
        Language::German => pair.1,
    }
}

fn weekday(day: Weekday) -> &'static str {
    word(WEEKDAYS[day.to_monday_zero_offset() as usize])
}

fn month_and_day(date: jiff::civil::Date) -> String {
    let month = word(MONTHS[date.month() as usize - 1]);
    match language() {
        Language::English => format!("{month} {}", date.day()),
        Language::German => format!("{}. {month}", date.day()),
    }
}

/// What a list row shows: the time today, the weekday this week, then the date.
pub fn short(seconds: i64, now_seconds: i64) -> String {
    let zone = TimeZone::system();
    let at = |seconds: i64| {
        Timestamp::from_second(seconds)
            .unwrap_or_default()
            .to_zoned(zone.clone())
    };
    let (then, now) = (at(seconds), at(now_seconds));
    if then.date() == now.date() {
        return format!("{:02}:{:02}", then.hour(), then.minute());
    }
    let days = (now.date() - then.date()).get_days();
    if (0..7).contains(&days) {
        return weekday(then.weekday()).chars().take(3).collect();
    }
    if then.year() == now.year() {
        return month_and_day(then.date());
    }
    format!("{} {}", word(MONTHS[then.month() as usize - 1]), then.year())
}

/// What stands above a version: its weekday and time this week, then its whole date and time.
pub fn long(seconds: i64, now_seconds: i64) -> String {
    let zone = TimeZone::system();
    let at = |seconds: i64| {
        Timestamp::from_second(seconds)
            .unwrap_or_default()
            .to_zoned(zone.clone())
    };
    let (then, now) = (at(seconds), at(now_seconds));
    let clock = format!("{:02}:{:02}", then.hour(), then.minute());
    let days = (now.date() - then.date()).get_days();
    if (0..7).contains(&days) {
        return format!("{} {clock}", weekday(then.weekday()));
    }
    format!("{} {}, {clock}", month_and_day(then.date()), then.year())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell::strings::{LANGUAGE_TEST, set_language};

    #[test]
    fn dates_read_in_the_language_set() {
        let _guard = LANGUAGE_TEST.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let now = 1_788_000_000;
        let long_ago = now - 400 * 24 * 60 * 60;
        set_language(Language::English);
        assert_eq!(short(now, now).len(), 5);
        assert!(short(now - 2 * 24 * 60 * 60, now).len() == 3);
        let english = long(long_ago, now);
        set_language(Language::German);
        let german = long(long_ago, now);
        assert_ne!(english, german);
        assert!(german.contains(". "));
        set_language(Language::English);
    }
}
