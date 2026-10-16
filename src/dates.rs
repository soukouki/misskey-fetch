use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;
use thiserror::Error;

/// 解析不能・不正日付を LLM に返す文言（spec 6.2.3）。
#[derive(Debug, Error)]
#[error("具体的な日付（YYYY-MM-DD形式）で再指定してください: {input}")]
pub struct DateError {
    pub input: String,
}

impl DateError {
    fn new(input: &str) -> Self {
        Self {
            input: input.to_string(),
        }
    }
}

/// 厳格な ISO 8601 のみを検証する（曖昧解析はしない。正規化は LLM 側の仕事）。
///
/// - 日付のみ（`YYYY-MM-DD`）: `TZ` の 0 時境界として扱う
/// - オフセット付き（`Z` / `+09:00`）: そのまま尊重する
pub fn parse_iso8601(input: &str, tz: Tz) -> Result<DateTime<Utc>, DateError> {
    let invalid = || DateError::new(input);

    if input.len() < 10 {
        return Err(invalid());
    }
    let bytes = input.as_bytes();
    if bytes[4] != b'-' || bytes[7] != b'-' {
        return Err(invalid());
    }
    let year = number(input, 0, 4).ok_or_else(invalid)?;
    let month = number(input, 5, 2).ok_or_else(invalid)?;
    let day = number(input, 8, 2).ok_or_else(invalid)?;
    let date = NaiveDate::from_ymd_opt(year as i32, month, day).ok_or_else(invalid)?;

    let rest = &input[10..];
    if rest.is_empty() {
        let naive = date.and_time(NaiveTime::MIN);
        let local = tz.from_local_datetime(&naive).latest().ok_or_else(invalid)?;
        return Ok(local.with_timezone(&Utc));
    }

    if bytes[10] != b'T' || rest.len() < 9 {
        return Err(invalid());
    }
    let hour = number(rest, 1, 2).ok_or_else(invalid)?;
    if rest.as_bytes()[3] != b':' || rest.as_bytes()[6] != b':' {
        return Err(invalid());
    }
    let minute = number(rest, 4, 2).ok_or_else(invalid)?;
    let second = number(rest, 7, 2).ok_or_else(invalid)?;
    if hour > 23 || minute > 59 || second > 59 {
        return Err(invalid());
    }

    let mut idx = 9;
    let mut nano = 0u32;
    if rest.as_bytes().get(9) == Some(&b'.') {
        let start = 10;
        let mut end = start;
        while end < rest.len() && rest.as_bytes()[end].is_ascii_digit() {
            end += 1;
        }
        let digits = end - start;
        if digits == 0 || digits > 3 {
            return Err(invalid());
        }
        nano = rest[start..end].parse::<u32>().map_err(|_| invalid())?;
        for _ in 0..(9 - digits) {
            nano *= 10;
        }
        idx = end;
    }

    let offset = match rest.as_bytes().get(idx) {
        Some(&b'Z') => {
            if idx + 1 != rest.len() {
                return Err(invalid());
            }
            FixedOffset::east_opt(0).ok_or_else(invalid)?
        }
        Some(&b'+') | Some(&b'-') => {
            if idx + 6 != rest.len() {
                return Err(invalid());
            }
            let sign = if rest.as_bytes()[idx] == b'+' { 1 } else { -1 };
            let off_hour = number(rest, idx + 1, 2).ok_or_else(invalid)?;
            if rest.as_bytes()[idx + 3] != b':' {
                return Err(invalid());
            }
            let off_min = number(rest, idx + 4, 2).ok_or_else(invalid)?;
            if off_hour > 23 || off_min > 59 {
                return Err(invalid());
            }
            FixedOffset::east_opt(sign * (off_hour as i32 * 60 + off_min as i32) * 60)
                .ok_or_else(invalid)?
        }
        _ => return Err(invalid()),
    };

    let time = NaiveTime::from_hms_nano_opt(hour, minute, second, nano).ok_or_else(invalid)?;
    let naive = NaiveDateTime::new(date, time);
    let dt = naive
        .and_local_timezone(offset)
        .single()
        .ok_or_else(invalid)?;
    Ok(dt.with_timezone(&Utc))
}

/// Misskey API が返す `createdAt`（常に UTC の ISO 8601）を再検証用に解析する。
pub fn parse_created_at(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc))
        .ok()
}

fn number(s: &str, start: usize, len: usize) -> Option<u32> {
    let chunk = s.get(start..start + len)?;
    if !chunk.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    chunk.parse::<u32>().ok()
}
