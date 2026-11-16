// 4개 앱 공통 사본 - 고칠 때 네 곳을 같이 고친다.
//
// YYYY-MM-DD 문자열 날짜 (TS dates.ts). Node(V8) Date 와 같게:
// - `new Date('YYYY-MM-DDT00:00:00Z')`: 월 1~12, 일 1~31 이면 받고 넘치는 날은 다음 달로(2026-02-30 → 03-02), 형식이 다르면 Invalid
// - setUTCDate/setUTCMonth 는 인자를 정수로 자른다(trunc), 말일을 넘치게 둔다(1/31 + 1개월 = 3/3)
// - Invalid 면 toISOString 이 RangeError('Invalid time value') → 여기서는 None
pub const TZ: chrono_tz::Tz = chrono_tz::Asia::Seoul;

/// new Date().toLocaleDateString('en-CA', { timeZone: 'Asia/Seoul' })
pub fn today_str() -> String {
    chrono::Utc::now()
        .with_timezone(&TZ)
        .format("%Y-%m-%d")
        .to_string()
}

/// 'YYYY-MM-DD' → (년, 0부터 월, 일) - V8 ISO 날짜 파서 규칙
fn parse(date: &str) -> Option<(i64, i64, i64)> {
    let b = date.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return None;
    }
    let num = |s: &str| {
        if s.bytes().all(|c| c.is_ascii_digit()) {
            s.parse::<i64>().ok()
        } else {
            None
        }
    };
    let (y, m, d) = (num(&date[0..4])?, num(&date[5..7])?, num(&date[8..10])?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    Some((y, m - 1, d))
}

/// 1970-01-01 기준 일수 (proleptic Gregorian, 월 넘침 허용)
fn make_day(y: i64, m0: i64, d: i64) -> i64 {
    let y = y + m0.div_euclid(12);
    let m = m0.rem_euclid(12) + 1;
    // days_from_civil (H. Hinnant)
    let yy = if m <= 2 { y - 1 } else { y };
    let era = yy.div_euclid(400);
    let yoe = yy - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468 + (d - 1)
}

fn format_day(days: i64) -> String {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

fn trunc(n: f64) -> Option<i64> {
    if n.is_finite() {
        Some(n.trunc() as i64)
    } else {
        None
    }
}

/// 유효한 날짜인지 (new Date(`${d}T00:00:00Z`) 가 Invalid 가 아님)
pub fn is_parsable(date: &str) -> bool {
    parse(date).is_some()
}

pub fn add_days(date: &str, days: f64) -> Option<String> {
    let (y, m, d) = parse(date)?;
    Some(format_day(make_day(y, m, trunc(d as f64 + days)?)))
}

pub fn add_months(date: &str, months: f64) -> Option<String> {
    let (y, m, d) = parse(date)?;
    Some(format_day(make_day(y, trunc(m as f64 + months)?, d)))
}

/// a - b (일, Math.round). 하나라도 Invalid 면 NaN → None
pub fn diff_days(a: &str, b: &str) -> Option<i64> {
    let (ya, ma, da) = parse(a)?;
    let (yb, mb, db) = parse(b)?;
    Some(make_day(ya, ma, da) - make_day(yb, mb, db))
}

pub fn month_of(date: &str) -> f64 {
    // Number(date.slice(5, 7))
    let s: String = date.chars().skip(5).take(2).collect();
    if s.is_empty() {
        0.0
    } else {
        s.trim().parse::<f64>().unwrap_or(f64::NAN)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn like_v8() {
        assert_eq!(add_days("2026-02-30", 0.0).unwrap(), "2026-03-02");
        assert!(add_days("2026-02-32", 0.0).is_none());
        assert!(add_days("2026-1-02", 0.0).is_none());
        assert_eq!(add_months("2026-01-31", 1.0).unwrap(), "2026-03-03");
        assert_eq!(add_months("2026-01-31", 1.7).unwrap(), "2026-03-03");
        assert_eq!(add_months("2026-01-31", -1.2).unwrap(), "2025-12-31");
        assert_eq!(add_months("2026-11-15", 3.0).unwrap(), "2027-02-15");
        assert_eq!(add_days("2026-01-31", 7.9).unwrap(), "2026-02-07");
        assert_eq!(add_days("2026-01-31", -0.5).unwrap(), "2026-01-30");
        assert_eq!(add_days("2026-12-31", 1.0).unwrap(), "2027-01-01");
        assert_eq!(add_days("2024-02-28", 1.0).unwrap(), "2024-02-29");
        assert_eq!(diff_days("2026-11-20", "2026-11-16").unwrap(), 4);
        assert_eq!(diff_days("2026-03-01", "2026-02-30").unwrap(), -1);
    }
}
