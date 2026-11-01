//! Regional date display. Capture times are wall-clock values, not UTC instants.
use chrono::{DateTime, Datelike, NaiveDateTime, Utc};
use std::cell::RefCell;
use std::collections::HashMap;
use std::time::{Duration, Instant};

struct Cache {
    values: HashMap<(i64, bool), String>,
    refreshed: Instant,
}

pub struct DateFormatter {
    cache: RefCell<Cache>,
    #[cfg(test)]
    fixed: Option<(chrono::Locale, i32)>,
}

impl DateFormatter {
    pub fn system() -> Self {
        Self {
            cache: RefCell::new(Cache {
                values: HashMap::new(),
                refreshed: Instant::now(),
            }),
            #[cfg(test)]
            fixed: None,
        }
    }

    pub fn modified(&self, seconds: Option<i64>) -> String {
        let Some(date) = seconds.and_then(|s| DateTime::from_timestamp(s, 0)) else {
            return "—".into();
        };
        self.format(date, false)
    }

    pub fn captured(&self, value: Option<&str>) -> String {
        let Some(value) = value.filter(|s| s.len() <= 64) else {
            return "—".into();
        };
        let parsed = NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S%.f")
            .or_else(|_| NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S%.f"));
        let Ok(date) = parsed else {
            return "—".into();
        };
        // UTC is only a container here. The native formatter also uses UTC for
        // this value, preserving the camera clock even across DST gaps.
        self.format(date.and_utc(), true)
    }

    fn format(&self, date: DateTime<Utc>, floating: bool) -> String {
        if !(1..=9999).contains(&date.year()) {
            return "—".into();
        }
        let key = (date.timestamp(), floating);
        if let Ok(mut cache) = self.cache.try_borrow_mut() {
            // Re-read system preferences periodically without retaining an
            // unbounded cache while browsing large catalogues.
            if cache.refreshed.elapsed() >= Duration::from_secs(30) {
                cache.values.clear();
                cache.refreshed = Instant::now();
            }
            if let Some(value) = cache.values.get(&key) {
                return value.clone();
            }
        }
        #[cfg(test)]
        if let Some((locale, offset)) = self.fixed {
            return fixed_format(date, floating, locale, offset).unwrap_or_else(|| "—".into());
        }
        let value = system_format(date, floating)
            .filter(|s| !s.trim().is_empty() && s.len() <= 1024)
            .unwrap_or_else(|| {
                if floating {
                    date.format("%Y-%m-%d at %H:%M:%S").to_string()
                } else {
                    date.format("%Y-%m-%d at %H:%M:%S UTC").to_string()
                }
            });
        if let Ok(mut cache) = self.cache.try_borrow_mut() {
            if cache.values.len() >= 512 {
                cache.values.clear();
            }
            cache.values.insert(key, value.clone());
        }
        value
    }

    /// Explicit deterministic settings for tests; never change the process locale.
    #[cfg(test)]
    pub fn fixed(locale: chrono::Locale, offset_seconds: i32) -> Self {
        let mut formatter = Self::system();
        formatter.fixed = Some((locale, offset_seconds));
        formatter
    }
}

#[cfg(target_os = "macos")]
fn system_format(date: DateTime<Utc>, floating: bool) -> Option<String> {
    use objc2_foundation::{NSLocale, NSTimeZone};
    Some(macos_format(
        date,
        floating,
        &NSLocale::currentLocale(),
        &NSTimeZone::localTimeZone(),
    ))
}

#[cfg(target_os = "macos")]
fn macos_format(
    date: DateTime<Utc>,
    floating: bool,
    locale: &objc2_foundation::NSLocale,
    zone: &objc2_foundation::NSTimeZone,
) -> String {
    use objc2_foundation::{NSDate, NSDateFormatter, NSDateFormatterStyle, NSString, NSTimeZone};
    let formatter = NSDateFormatter::new();
    formatter.setLocale(Some(locale));
    // Ask Foundation for the region's numeric order/separators, explicitly
    // retaining two-digit day/month and a full year for catalogue dates.
    formatter.setLocalizedDateFormatFromTemplate(&NSString::from_str("ddMMyyyy"));
    let time = NSDateFormatter::new();
    time.setLocale(Some(locale));
    time.setDateStyle(NSDateFormatterStyle::NoStyle);
    time.setTimeStyle(NSDateFormatterStyle::MediumStyle);
    if floating {
        formatter.setTimeZone(Some(&NSTimeZone::timeZoneForSecondsFromGMT(0)));
        time.setTimeZone(Some(&NSTimeZone::timeZoneForSecondsFromGMT(0)));
    } else {
        formatter.setTimeZone(Some(zone));
        time.setTimeZone(Some(zone));
    }
    let date = NSDate::dateWithTimeIntervalSince1970(date.timestamp() as f64);
    format!(
        "{} at {}",
        formatter.stringFromDate(&date),
        time.stringFromDate(&date)
    )
}

#[cfg(target_os = "windows")]
fn system_format(date: DateTime<Utc>, floating: bool) -> Option<String> {
    use windows::core::HSTRING;
    use windows::Foundation::DateTime as WindowsDateTime;
    use windows::Globalization::DateTimeFormatting::DateTimeFormatter;
    let ticks = date
        .timestamp()
        .checked_add(11_644_473_600)?
        .checked_mul(10_000_000)?;
    let date = WindowsDateTime {
        UniversalTime: ticks,
    };
    let short_date = DateTimeFormatter::ShortDate().ok()?;
    let pattern = short_date.Patterns().ok()?.GetAt(0).ok()?.to_string();
    if pattern.len() > 1024 {
        return None;
    }
    let short_date = DateTimeFormatter::CreateDateTimeFormatter(&HSTRING::from(
        numeric_windows_pattern(&pattern),
    ))
    .ok()?;
    let time = DateTimeFormatter::LongTime().ok()?;
    let format = |formatter: &DateTimeFormatter| {
        if floating {
            formatter.FormatUsingTimeZone(date, &HSTRING::from("UTC"))
        } else {
            formatter.Format(date)
        }
    };
    Some(format!(
        "{} at {}",
        format(&short_date).ok()?,
        format(&time).ok()?
    ))
}

#[cfg(target_os = "linux")]
fn system_format(date: DateTime<Utc>, floating: bool) -> Option<String> {
    let locale =
        regional_locale(["LC_ALL", "LC_TIME", "LANG"].map(|name| std::env::var(name).ok()));
    if floating {
        localized(date, locale)
    } else {
        localized(date.with_timezone(&chrono::Local), locale)
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
fn system_format(_date: DateTime<Utc>, _floating: bool) -> Option<String> {
    None
}

#[cfg(any(target_os = "linux", test))]
fn regional_locale(names: [Option<String>; 3]) -> chrono::Locale {
    let name = names.iter().flatten().find(|s| !s.is_empty());
    let name = name.map(String::as_str).unwrap_or("C");
    if name.len() > 128 {
        return chrono::Locale::POSIX;
    }
    let base = name.split(['.', '@']).next().unwrap_or("C");
    if base == "C" || base == "POSIX" {
        chrono::Locale::POSIX
    } else {
        base.replace('-', "_")
            .parse()
            .unwrap_or(chrono::Locale::POSIX)
    }
}

#[cfg(test)]
fn fixed_format(
    date: DateTime<Utc>,
    floating: bool,
    locale: chrono::Locale,
    offset: i32,
) -> Option<String> {
    let offset = chrono::FixedOffset::east_opt(if floating { 0 } else { offset })?;
    localized(date.with_timezone(&offset), locale)
}

#[cfg(any(target_os = "linux", test))]
fn localized<T: chrono::TimeZone>(date: DateTime<T>, locale: chrono::Locale) -> Option<String>
where
    T::Offset: std::fmt::Display,
{
    use chrono::format::{Fixed, Item, Numeric, Pad, StrftimeItems};
    use std::fmt::Write;
    let numeric_date = StrftimeItems::new_with_locale("%x", locale).map(|item| match item {
        Item::Numeric(Numeric::YearMod100, _) => Item::Numeric(Numeric::Year, Pad::Zero),
        Item::Numeric(field @ (Numeric::Day | Numeric::Month | Numeric::Year), _) => {
            Item::Numeric(field, Pad::Zero)
        }
        Item::Fixed(Fixed::ShortMonthName | Fixed::LongMonthName) => {
            Item::Numeric(Numeric::Month, Pad::Zero)
        }
        other => other,
    });
    let mut text = String::new();
    write!(
        &mut text,
        "{} at {}",
        date.format_localized_with_items(numeric_date, locale),
        date.format_localized("%X", locale)
    )
    .ok()?;
    Some(text)
}

#[cfg(any(target_os = "windows", test))]
fn numeric_windows_pattern(pattern: &str) -> String {
    let mut pattern = pattern.to_owned();
    for (from, to) in [
        ("{day.integer}", "{day.integer(2)}"),
        ("{day.integer(1)}", "{day.integer(2)}"),
        ("{month.integer}", "{month.integer(2)}"),
        ("{month.integer(1)}", "{month.integer(2)}"),
        ("{month.abbreviated}", "{month.integer(2)}"),
        ("{month.full}", "{month.integer(2)}"),
        ("{year.abbreviated}", "{year.full}"),
        ("{year.abbreviated(2)}", "{year.full}"),
    ] {
        pattern = pattern.replace(from, to);
    }
    pattern
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn region_controls_date_order_and_twelve_or_twenty_four_hour_time() {
        let gb = DateFormatter::fixed(chrono::Locale::en_GB, 0);
        let us = DateFormatter::fixed(chrono::Locale::en_US, 0);
        let de = DateFormatter::fixed(chrono::Locale::de_DE, 0);
        let captured = Some("2000-02-28T14:30:00");
        assert_eq!(gb.captured(captured), "28/02/2000 at 14:30:00");
        assert!(us.captured(captured).starts_with("02/28/2000"));
        assert!(us.captured(captured).contains("02:30:00 PM"));
        assert_eq!(de.captured(captured), "28.02.2000 at 14:30:00");
    }

    #[test]
    fn modified_uses_local_zone_and_capture_preserves_camera_clock() {
        for offset in [-18_000, 3600, 19_800] {
            let formatter = DateFormatter::fixed(chrono::Locale::en_GB, offset);
            assert_eq!(
                formatter.captured(Some("2000-02-28T14:30:00")),
                "28/02/2000 at 14:30:00"
            );
            assert_ne!(
                formatter.modified(Some(951_782_400)),
                "29/02/2000 at 00:00:00"
            );
            // A valid wall-clock capture in a spring-forward gap stays valid.
            assert_eq!(
                formatter.captured(Some("2026-03-29T01:30:00")),
                "29/03/2026 at 01:30:00"
            );
        }
        let formatter = DateFormatter::fixed(chrono::Locale::en_GB, -18_000);
        assert_eq!(formatter.modified(Some(0)), "31/12/1969 at 19:00:00");
    }

    #[test]
    fn invalid_and_missing_dates_are_explicit_and_never_panic() {
        let formatter = DateFormatter::system();
        for seconds in [None, Some(i64::MIN), Some(i64::MAX)] {
            assert_eq!(formatter.modified(seconds), "—");
        }
        for value in [
            None,
            Some(""),
            Some("bad"),
            Some("2001-02-29T12:00:00"),
            Some("2000-01-01T25:00:00"),
            Some("0000-01-01T00:00:00"),
            Some("2000-01-01T12:00:00Z"),
            Some("é:::::::::::::::::"),
        ] {
            assert_eq!(formatter.captured(value), "—", "{value:?}");
        }
        assert_eq!(formatter.captured(Some(&"a".repeat(100_000))), "—");
        assert_ne!(formatter.modified(Some(-1)), "—");
        assert_ne!(formatter.captured(Some("2000-02-29T12:34:56.789")), "—");
    }

    #[test]
    fn time_locale_honors_posix_precedence_and_encoding_suffixes() {
        let names = |a: &str, b: &str, c: &str| [a, b, c].map(|s| Some(s.into()));
        assert_eq!(
            regional_locale(names("de_DE.UTF-8", "en_US.UTF-8", "en_GB.UTF-8")),
            chrono::Locale::de_DE
        );
        assert_eq!(
            regional_locale(names("", "en_GB.UTF-8", "en_US.UTF-8")),
            chrono::Locale::en_GB
        );
        assert_eq!(
            regional_locale(names("", "", "en_US.UTF-8")),
            chrono::Locale::en_US
        );
        assert_eq!(
            regional_locale(names("C.UTF-8", "de_DE", "en_GB")),
            chrono::Locale::POSIX
        );
        assert_eq!(
            regional_locale(names("invalid", "de_DE", "en_GB")),
            chrono::Locale::POSIX
        );
    }

    #[test]
    fn cache_is_bounded_and_expired_preferences_are_reloaded() {
        let mut formatter = DateFormatter::system();
        let cache = formatter.cache.get_mut();
        for second in 1..=512 {
            cache.values.insert((second, false), "old settings".into());
        }
        assert_ne!(formatter.modified(Some(0)), "—");
        assert_eq!(formatter.cache.try_borrow().unwrap().values.len(), 1);
        let cache = formatter.cache.get_mut();
        cache.values.insert((0, false), "stale value".into());
        cache.refreshed = Instant::now().checked_sub(Duration::from_secs(31)).unwrap();
        assert_ne!(formatter.modified(Some(0)), "stale value");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_native_regions_and_daylight_saving_preserve_capture_wall_time() {
        use objc2_foundation::{NSLocale, NSString, NSTimeZone};
        let gb = NSLocale::localeWithLocaleIdentifier(&NSString::from_str("en_GB"));
        let us = NSLocale::localeWithLocaleIdentifier(&NSString::from_str("en_US"));
        let utc = NSTimeZone::timeZoneForSecondsFromGMT(0);
        let london = NSTimeZone::timeZoneWithName(&NSString::from_str("Europe/London")).unwrap();
        let date = NaiveDateTime::parse_from_str("2026-03-29T01:30:00", "%Y-%m-%dT%H:%M:%S")
            .unwrap()
            .and_utc();
        assert_ne!(
            macos_format(date, false, &gb, &utc),
            macos_format(date, false, &us, &utc)
        );
        assert_ne!(
            macos_format(date, false, &gb, &london),
            macos_format(date, true, &gb, &london)
        );
        assert_eq!(
            macos_format(date, true, &gb, &london),
            macos_format(date, true, &gb, &utc)
        );
        // Britain has advanced to UTC+1 for this instant, while the stored
        // capture wall-clock time stays at 01:30.
        let later = date
            .checked_add_signed(chrono::TimeDelta::hours(1))
            .unwrap();
        assert_eq!(
            macos_format(date, false, &gb, &london),
            macos_format(later, false, &gb, &utc)
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_uses_current_system_date_time_and_timezone_preferences() {
        use objc2_foundation::{NSDate, NSDateFormatter, NSDateFormatterStyle, NSLocale, NSString};
        let date = NSDate::dateWithTimeIntervalSince1970(951_782_400.0);
        let numeric = NSDateFormatter::new();
        numeric.setLocale(Some(&NSLocale::currentLocale()));
        numeric.setLocalizedDateFormatFromTemplate(&NSString::from_str("ddMMyyyy"));
        let time = NSDateFormatter::localizedStringFromDate_dateStyle_timeStyle(
            &date,
            NSDateFormatterStyle::NoStyle,
            NSDateFormatterStyle::MediumStyle,
        )
        .to_string();
        let expected = format!("{} at {time}", numeric.stringFromDate(&date));
        assert_eq!(
            DateFormatter::system().modified(Some(951_782_400)),
            expected
        );
    }

    #[test]
    fn numeric_examples_keep_full_years_regional_order_and_at_separator() {
        let captured = Some("2026-10-06T22:30:05");
        assert_eq!(
            DateFormatter::fixed(chrono::Locale::en_GB, 0).captured(captured),
            "06/10/2026 at 22:30:05"
        );
        assert_eq!(
            DateFormatter::fixed(chrono::Locale::en_US, 0).captured(captured),
            "10/06/2026 at 10:30:05 PM"
        );
        assert_eq!(
            numeric_windows_pattern("{month.integer}/{day.integer}/{year.abbreviated(2)}"),
            "{month.integer(2)}/{day.integer(2)}/{year.full}"
        );
        assert_eq!(
            numeric_windows_pattern("{day.integer(2)}.{month.integer(2)}.{year.full}"),
            "{day.integer(2)}.{month.integer(2)}.{year.full}"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn native_numeric_dates_use_padded_day_month_and_full_year_in_both_regions() {
        use objc2_foundation::{NSLocale, NSString, NSTimeZone};
        let utc = NSTimeZone::timeZoneForSecondsFromGMT(0);
        let date = NaiveDateTime::parse_from_str("2026-10-06T22:30:05", "%Y-%m-%dT%H:%M:%S")
            .unwrap()
            .and_utc();
        for (locale, prefix) in [("en_GB", "06/10/2026 at "), ("en_US", "10/06/2026 at ")] {
            let locale = NSLocale::localeWithLocaleIdentifier(&NSString::from_str(locale));
            let text = macos_format(date, true, &locale, &utc);
            assert!(text.starts_with(prefix), "{text}");
        }
    }
}
