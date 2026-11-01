//! Shared presentation of catalogue values across desktop views.

/// Keep exposure seconds concise without changing the stored precision.
pub fn exposure_time(seconds: f64) -> String {
    if !seconds.is_finite() || seconds <= 0.0 {
        return "—".into();
    }
    let value = seconds.to_string();
    let Some((whole, fraction)) = value.split_once('.') else {
        return format!("{value}s");
    };
    // Trim rather than round up, matching 0.1666… → 0.16. For exposures
    // below 0.01 seconds retain two significant digits instead of showing 0s.
    let digits = if seconds < 0.01 {
        fraction
            .bytes()
            .take_while(|b| *b == b'0')
            .count()
            .saturating_add(2)
    } else {
        2
    };
    let fraction: String = fraction.chars().take(digits).collect();
    let fraction = fraction.trim_end_matches('0');
    if fraction.is_empty() {
        format!("{whole}s")
    } else {
        format!("{whole}.{fraction}s")
    }
}

/// Format file sizes using binary units, keeping unavailable values explicit.
pub fn file_size(size: Option<i64>) -> String {
    let Some(size) = size.filter(|s| *s >= 0) else {
        return "—".into();
    };
    if size < 1024 {
        return format!("{size} B");
    }
    let mut value = size as f64;
    let mut unit = "B";
    for next in ["KiB", "MiB", "GiB", "TiB", "PiB", "EiB"] {
        if value < 1024.0 {
            break;
        }
        value /= 1024.0;
        unit = next;
    }
    format!("{value:.1} {unit}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposure_times_trim_long_fractions_without_zeroing_short_exposures() {
        for (seconds, expected) in [
            (1.0 / 6.0, "0.16s"),
            (0.16666666666666666, "0.16s"),
            (0.03333333333333333, "0.03s"),
            (0.1, "0.1s"),
            (0.01, "0.01s"),
            (0.004, "0.004s"),
            (1.0 / 8000.0, "0.00012s"),
            (0.00000001, "0.00000001s"),
            (1.0, "1s"),
            (1.234567, "1.23s"),
            (30.0, "30s"),
            (0.0, "—"),
            (-1.0, "—"),
            (f64::NAN, "—"),
            (f64::INFINITY, "—"),
            (f64::NEG_INFINITY, "—"),
        ] {
            assert_eq!(exposure_time(seconds), expected, "{seconds}");
        }
        for seconds in [f64::MIN_POSITIVE, f64::from_bits(1), f64::MAX] {
            let displayed = exposure_time(seconds);
            assert!(displayed.ends_with('s'));
            assert_ne!(displayed, "0s");
            assert!(displayed.len() < 400);
        }
    }

    #[test]
    fn file_sizes_cover_unit_boundaries_and_missing_or_hostile_values() {
        for (size, expected) in [
            (None, "—"),
            (Some(-1), "—"),
            (Some(i64::MIN), "—"),
            (Some(0), "0 B"),
            (Some(1), "1 B"),
            (Some(1023), "1023 B"),
            (Some(1024), "1.0 KiB"),
            (Some(1536), "1.5 KiB"),
            (Some(1_048_576), "1.0 MiB"),
            (Some(1_572_864), "1.5 MiB"),
            (Some(1_073_741_824), "1.0 GiB"),
            (Some(1_099_511_627_776), "1.0 TiB"),
            (Some(1_125_899_906_842_624), "1.0 PiB"),
            (Some(1_152_921_504_606_846_976), "1.0 EiB"),
            (Some(i64::MAX), "8.0 EiB"),
        ] {
            assert_eq!(file_size(size), expected, "{size:?}");
        }
    }
}
