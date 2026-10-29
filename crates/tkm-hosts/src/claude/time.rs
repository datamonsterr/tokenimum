//! Small checked RFC3339 parser, with no clock or date library dependency.

use tkm_core::model::Timestamp;

fn number(text: &str, range: std::ops::Range<usize>) -> Option<i64> {
    let digits = text.get(range)?;
    if !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

pub(super) fn parse(text: &str) -> Option<Timestamp> {
    let bytes = text.as_bytes();
    if bytes.get(4) != Some(&b'-')
        || bytes.get(7) != Some(&b'-')
        || !matches!(bytes.get(10), Some(b'T' | b't'))
        || bytes.get(13) != Some(&b':')
        || bytes.get(16) != Some(&b':')
    {
        return None;
    }
    let year = number(text, 0..4)?;
    let month = number(text, 5..7)?;
    let day = number(text, 8..10)?;
    let hour = number(text, 11..13)?;
    let minute = number(text, 14..16)?;
    let second = number(text, 17..19)?;
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let max_day = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return None,
    };
    if !(1..=max_day).contains(&day)
        || !(0..=23).contains(&hour)
        || !(0..=59).contains(&minute)
        || !(0..=59).contains(&second)
    {
        return None;
    }
    let mut cursor = 19;
    let mut milliseconds = 0;
    if bytes.get(cursor) == Some(&b'.') {
        cursor += 1;
        let start = cursor;
        while bytes.get(cursor).is_some_and(u8::is_ascii_digit) {
            cursor += 1;
        }
        if cursor == start {
            return None;
        }
        for offset in 0..3 {
            milliseconds *= 10;
            if let Some(digit) = bytes
                .get(start + offset)
                .filter(|_| start + offset < cursor)
            {
                milliseconds += i64::from(*digit - b'0');
            }
        }
    }
    let offset = match bytes.get(cursor) {
        Some(b'Z' | b'z') if cursor + 1 == bytes.len() => 0,
        Some(sign @ (b'+' | b'-'))
            if cursor + 6 == bytes.len() && bytes.get(cursor + 3) == Some(&b':') =>
        {
            let hours = number(text, cursor + 1..cursor + 3)?;
            let minutes = number(text, cursor + 4..cursor + 6)?;
            if hours > 23 || minutes > 59 {
                return None;
            }
            (hours * 3600 + minutes * 60) * if *sign == b'+' { 1 } else { -1 }
        }
        _ => return None,
    };
    // Gregorian civil date to days since Unix epoch (400-year eras).
    let y = year - i64::from(month <= 2);
    let era = y.div_euclid(400);
    let year_of_era = y - era * 400;
    let m = month + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * m + 2) / 5 + day - 1;
    let days = era * 146_097 + year_of_era * 365 + year_of_era / 4 - year_of_era / 100
        + day_of_year
        - 719_468;
    Some(Timestamp(
        ((days * 24 + hour) * 60 * 60 + minute * 60 + second - offset) * 1000 + milliseconds,
    ))
}
