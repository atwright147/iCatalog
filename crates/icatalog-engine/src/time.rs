use std::time::{SystemTime, UNIX_EPOCH};

fn now() -> std::time::Duration {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
}

/// Current time as an ISO-8601 UTC string (`1970-01-01T00:00:00Z`).
pub fn utc_now_iso8601() -> String {
    epoch_to_iso8601(i64::try_from(now().as_secs()).unwrap_or(0))
}

pub(crate) fn epoch_to_iso8601(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // Howard Hinnant's civil_from_days.
    let z = days.saturating_add(719_468);
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era.saturating_mul(400) + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

pub(crate) fn generate_catalogue_id() -> String {
    let nanos = u64::try_from(now().as_nanos()).unwrap_or(0);
    format!("cat-{:016x}-{:08x}", nanos, std::process::id())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_epoch() {
        assert_eq!(epoch_to_iso8601(0), "1970-01-01T00:00:00Z");
        assert_eq!(epoch_to_iso8601(86_400), "1970-01-02T00:00:00Z");
        assert_eq!(epoch_to_iso8601(1_600_000_000), "2020-09-13T12:26:40Z");
    }
}
