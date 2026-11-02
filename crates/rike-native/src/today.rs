//! The local date, without a time-zone crate.

use rike_core::journal::{JournalDate, today_from_unix};

/// Today's date in the local time zone. The offset comes from `date +%z`
/// (std has no local time zone); a failure falls back to UTC.
pub fn local_today() -> JournalDate {
    let unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    today_from_unix(unix, local_utc_offset_secs())
        .or_else(|| JournalDate::new(1970, 1, 1))
        .expect("1970-01-01 is a valid date")
}

fn local_utc_offset_secs() -> i32 {
    let Ok(out) = std::process::Command::new("date").arg("+%z").output() else {
        return 0;
    };
    parse_offset(String::from_utf8_lossy(&out.stdout).trim()).unwrap_or(0)
}

fn parse_offset(s: &str) -> Option<i32> {
    let sign = match s.as_bytes().first()? {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let digits = s.get(1..5)?;
    let hours: i32 = digits.get(..2)?.parse().ok()?;
    let minutes: i32 = digits.get(2..4)?.parse().ok()?;
    Some(sign * (hours * 3600 + minutes * 60))
}

#[cfg(test)]
mod tests {
    use super::parse_offset;

    #[test]
    fn parses_offsets() {
        assert_eq!(parse_offset("+0800"), Some(28_800));
        assert_eq!(parse_offset("-0330"), Some(-12_600));
        assert_eq!(parse_offset("x"), None);
    }
}
