//! Validation for the date shapes accepted by documented VUT GET parameters.

pub(crate) fn date(value: &str) -> Result<String, String> {
    let bytes = value.as_bytes();
    if bytes.len() != 10
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || !bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| index == 4 || index == 7 || byte.is_ascii_digit())
    {
        return Err("expected a calendar date in YYYY-MM-DD format".into());
    }
    let year: u32 = value[0..4].parse().expect("digits validated");
    let month: u32 = value[5..7].parse().expect("digits validated");
    let day: u32 = value[8..10].parse().expect("digits validated");
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let max_day = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => 0,
    };
    if year == 0 || day == 0 || day > max_day {
        return Err("expected a real calendar date".into());
    }
    Ok(value.into())
}

pub(crate) fn local_datetime(value: &str) -> Result<String, String> {
    let bytes = value.as_bytes();
    if bytes.len() != 16
        || bytes[10] != b'T'
        || bytes[13] != b':'
        || !bytes[11..13].iter().all(u8::is_ascii_digit)
        || !bytes[14..16].iter().all(u8::is_ascii_digit)
    {
        return Err("expected local date and time in YYYY-MM-DDTHH:MM format".into());
    }
    date(value.get(..10).ok_or("expected an ASCII calendar date")?)?;
    let hour: u32 = value[11..13].parse().expect("digits validated");
    let minute: u32 = value[14..16].parse().expect("digits validated");
    if hour > 23 || minute > 59 {
        return Err("expected a real local time".into());
    }
    Ok(value.into())
}

pub(crate) fn ordered(from: &str, to: &str) -> Result<(), &'static str> {
    if from > to {
        Err("--from must not be after --to")
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_accepts_real_calendar_days() {
        assert_eq!(date("2028-02-29"), Ok("2028-02-29".into()));
        for value in [
            "2026-02-29",
            "2026-04-31",
            "2026-13-01",
            "0000-01-01",
            "2026-9-01",
        ] {
            assert!(date(value).is_err(), "accepted {value}");
        }
    }

    #[test]
    fn local_datetime_requires_minute_precision_and_valid_time() {
        assert_eq!(
            local_datetime("2028-02-29T23:59"),
            Ok("2028-02-29T23:59".into())
        );
        for value in [
            "2026-02-30T12:00",
            "2026-09-29T24:00",
            "2026-09-29T12:60",
            "2026-09-29",
            "2026-09-29T12:00Z",
            "é026-09-29T12:00",
        ] {
            assert!(local_datetime(value).is_err(), "accepted {value}");
        }
    }

    #[test]
    fn range_includes_equal_boundaries_and_rejects_reversal() {
        assert!(ordered("2026-09-29", "2026-09-29").is_ok());
        assert!(ordered("2026-10-01", "2026-09-29").is_err());
        assert!(ordered("2026-09-29T18:00", "2026-09-29T08:00").is_err());
    }
}
