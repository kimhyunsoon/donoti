// 감시 알림 스케줄 모델 - 모든 시각은 KST 24시간제 'HH:MM' (frontend/src/schedule.ts와 동기 유지)
//
// 실행 규칙 하나 - 여러 규칙을 배열로 조합해 iOS 미리알림처럼 자유롭게 설정
//   { days: number[] (0=일~6=토, 최소 1개), mode: 'times'|'interval',
//     times?: string[] (mode=times), start?/end?: 'HH:MM', every?: 분 (mode=interval, start 시각부터 기산) }
use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;

use crate::core::js;
use crate::jsx;

// JS \d 는 ASCII 숫자만
static TIME_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^([01][0-9]|2[0-3]):[0-5][0-9]$").unwrap());

// 'HH:MM' → 자정 기준 분
fn to_minutes(hhmm: &str) -> f64 {
    js::string_to_number(&js::utf16_slice(hhmm, 0, 2)) * 60.0
        + js::string_to_number(&js::utf16_slice(hhmm, 3, 5))
}

/// toMinutes(v) - 문자열이 아니면 TS 처럼 TypeError
fn to_minutes_of(v: Option<&Value>) -> Result<f64, String> {
    match v {
        Some(Value::String(s)) => Ok(to_minutes(s)),
        None => Err("Cannot read properties of undefined (reading 'slice')".into()),
        Some(Value::Null) => Err("Cannot read properties of null (reading 'slice')".into()),
        Some(_) => Err("hhmm.slice is not a function".into()),
    }
}

pub struct KstNow {
    pub day: u32,
    pub hhmm: String,
    /// ends_at('YYYY-MM-DD HH:MM')과 문자열 비교 가능한 형식
    pub stamp: String,
}

/// 현재 KST 시각. KST는 UTC+9 고정(서머타임 없음)
pub fn kst_now() -> KstNow {
    kst_at(chrono::Utc::now().timestamp_millis())
}

pub fn kst_at(epoch_ms: i64) -> KstNow {
    use chrono::{Datelike, Timelike};
    let t = chrono::DateTime::from_timestamp_millis(epoch_ms + 9 * 3_600_000)
        .unwrap()
        .naive_utc();
    let hhmm = format!("{:02}:{:02}", t.hour(), t.minute());
    KstNow {
        day: t.weekday().num_days_from_sunday(),
        stamp: format!("{}-{:02}-{:02} {hhmm}", t.year(), t.month(), t.day()),
        hhmm,
    }
}

fn is_integer_in(v: &Value, lo: f64, hi: f64) -> bool {
    js::is_integer(Some(v)) && {
        let f = v.as_f64().unwrap_or(f64::NAN);
        f >= lo && f <= hi
    }
}

/// 스케줄 JSON 검증. 문제 없으면 None, 있으면 오류 메시지
pub fn validate_schedule(value: Option<&Value>) -> Option<&'static str> {
    let Some(Value::Array(rules)) = value else {
        return Some("실행 조건이 최소 1개 필요합니다");
    };
    if rules.is_empty() {
        return Some("실행 조건이 최소 1개 필요합니다");
    }
    for rule in rules {
        // typeof rule !== 'object' || rule === null (배열은 object 라 통과)
        let empty = serde_json::Map::new();
        let r = match rule {
            Value::Object(m) => m,
            Value::Array(_) => &empty,
            _ => return Some("잘못된 조건 형식입니다"),
        };
        match r.get("days") {
            Some(Value::Array(days))
                if !days.is_empty() && days.iter().all(|d| is_integer_in(d, 0.0, 6.0)) => {}
            _ => return Some("요일을 1개 이상 선택해야 합니다"),
        }
        match r.get("mode").and_then(|m| m.as_str()) {
            Some("times") => match r.get("times") {
                Some(Value::Array(times))
                    if !times.is_empty()
                        && times
                            .iter()
                            .all(|t| matches!(t, Value::String(s) if TIME_RE.is_match(s))) => {}
                _ => return Some("실행 시각을 1개 이상 지정해야 합니다"),
            },
            Some("interval") => {
                let (Some(Value::String(start)), Some(Value::String(end))) =
                    (r.get("start"), r.get("end"))
                else {
                    return Some("시작·종료 시각이 올바르지 않습니다");
                };
                if !TIME_RE.is_match(start) || !TIME_RE.is_match(end) {
                    return Some("시작·종료 시각이 올바르지 않습니다");
                }
                if to_minutes(start) > to_minutes(end) {
                    return Some("시작 시각이 종료 시각보다 늦습니다");
                }
                // 간격은 5분~120분, 5분 단위
                let every = r.get("every");
                let ok = js::is_integer(every) && {
                    let e = every.and_then(|v| v.as_f64()).unwrap_or(f64::NAN);
                    (5.0..=120.0).contains(&e) && e % 5.0 == 0.0
                };
                if !ok {
                    return Some("실행 간격은 5분~120분 사이 5분 단위여야 합니다");
                }
            }
            _ => return Some("잘못된 실행 방식입니다"),
        }
    }
    None
}

/// SameValueZero 로 배열에 있는지 (Array.prototype.includes)
fn includes(arr: &[Value], x: &Value) -> bool {
    arr.iter().any(|v| match (v, x) {
        (Value::Number(a), Value::Number(b)) => js::as_f64(a) == js::as_f64(b),
        _ => v == x,
    })
}

/// 현재 KST 시각(요일·'HH:MM')이 규칙 중 하나와 일치하는지 - 일치 시점이 크롤링 시작 타이밍.
/// 규칙 모양이 깨져 있으면 TS 처럼 TypeError (스케줄러 틱 전체가 중단된다)
pub fn matches_schedule(rules: &Value, day: u32, hhmm: &str) -> Result<bool, String> {
    let now = to_minutes(hhmm);
    let Value::Array(rules) = rules else {
        return Err("rules.some is not a function".into());
    };
    let day = Value::from(day);
    let hhmm_v = Value::String(hhmm.to_string());
    for rule in rules {
        let days = jsx::prop(Some(rule), "days")?;
        let Some(Value::Array(days)) = days else {
            return Err(match days {
                None => "Cannot read properties of undefined (reading 'includes')".into(),
                Some(Value::Null) => "Cannot read properties of null (reading 'includes')".into(),
                _ => "rule.days.includes is not a function".into(),
            });
        };
        if !includes(days, &day) {
            continue;
        }
        if jsx::prop(Some(rule), "mode")? == Some(&Value::String("times".into())) {
            let times = jsx::prop(Some(rule), "times")?;
            match times {
                Some(Value::Array(t)) => {
                    if includes(t, &hhmm_v) {
                        return Ok(true);
                    }
                    continue;
                }
                Some(Value::String(s)) => {
                    // String.prototype.includes
                    if s.contains(hhmm) {
                        return Ok(true);
                    }
                    continue;
                }
                None => {
                    return Err("Cannot read properties of undefined (reading 'includes')".into());
                }
                Some(Value::Null) => {
                    return Err("Cannot read properties of null (reading 'includes')".into());
                }
                _ => return Err("rule.times.includes is not a function".into()),
            }
        }
        let start = to_minutes_of(jsx::prop(Some(rule), "start")?)?;
        if now < start || now > to_minutes_of(jsx::prop(Some(rule), "end")?)? {
            continue;
        }
        let every = js::to_number(jsx::prop(Some(rule), "every")?);
        if (now - start) % every == 0.0 {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn kst() {
        // 2026-11-16 00:30 UTC = 09:30 KST 월요일
        let k = kst_at(1_794_789_000_000);
        assert_eq!(k.stamp, "2026-11-16 09:30");
        assert_eq!(k.day, 1);
    }

    #[test]
    fn schedule_rules() {
        let rules = json!([{"days":[1],"mode":"interval","start":"09:00","end":"21:00","every":5}]);
        assert!(matches_schedule(&rules, 1, "09:05").unwrap());
        assert!(!matches_schedule(&rules, 1, "09:06").unwrap());
        assert!(!matches_schedule(&rules, 2, "09:05").unwrap());
        assert!(!matches_schedule(&rules, 1, "21:05").unwrap());
        let t = json!([{"days":[0,1],"mode":"times","times":["13:39"]}]);
        assert!(matches_schedule(&t, 0, "13:39").unwrap());
        assert!(matches_schedule(&json!({}), 0, "13:39").is_err());
        assert_eq!(
            validate_schedule(Some(&json!([]))),
            Some("실행 조건이 최소 1개 필요합니다")
        );
        assert_eq!(
            validate_schedule(Some(
                &json!([{"days":[1],"mode":"interval","start":"09:00","end":"21:00","every":7}])
            )),
            Some("실행 간격은 5분~120분 사이 5분 단위여야 합니다")
        );
        assert_eq!(
            validate_schedule(Some(
                &json!([{"days":[1.0],"mode":"times","times":["23:59"]}])
            )),
            None
        );
    }
}
