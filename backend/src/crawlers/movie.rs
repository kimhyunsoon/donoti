// CGV·롯데·메가박스 크롤러 공통 조각 (세 TS 파일에 같은 코드가 각자 들어 있다)
use std::collections::HashMap;
use std::sync::Mutex;

use futures_util::FutureExt;
use futures_util::future::{BoxFuture, Shared};
use serde_json::Value;

use crate::core::dates;
use crate::jsx;

pub const DAY_LABELS: [&str; 7] = ["일", "월", "화", "수", "목", "금", "토"];

/// new Date(`${date}T00:00`) 의 (월, 일, 요일) - 로컬 시각이라 달력 날짜 그대로. Invalid 면 None
pub fn local_date(date: &str) -> Option<(u32, u32, usize)> {
    use chrono::Datelike;
    let norm = dates::add_days(date, 0.0)?;
    let d = chrono::NaiveDate::parse_from_str(&norm, "%Y-%m-%d").ok()?;
    Some((
        d.month(),
        d.day(),
        d.weekday().num_days_from_sunday() as usize,
    ))
}

/// '2026-08-21' → '8/21 (금)' (롯데·메가박스 dateHeader)
pub fn date_header_dash(date: &str) -> String {
    match local_date(date) {
        Some((m, d, w)) => format!("{m}/{d} ({})", DAY_LABELS[w]),
        None => "NaN/NaN (undefined)".into(),
    }
}

/// 오늘(KST) 이후 날짜만: dates.filter((d) => d >= today)
pub fn valid_dates(dates: &[Value], today: &str) -> Vec<Value> {
    let t = Value::String(today.to_string());
    dates
        .iter()
        .filter(|d| jsx::js_ge(Some(d), Some(&t)))
        .cloned()
        .collect()
}

/// `s.replaceAll(a, b)` (문자열 아니면 TypeError)
pub fn replace_all(v: &Value, a: &str, b: &str, expr: &str) -> Result<String, String> {
    match v {
        Value::String(s) => Ok(s.replace(a, b)),
        Value::Null => Err("Cannot read properties of null (reading 'replaceAll')".into()),
        _ => Err(format!("{expr}.replaceAll is not a function")),
    }
}

pub struct Hit {
    pub key: String,
    pub day: String,
    pub text: String,
}

/// 시간순 정렬 + 날짜 헤더로 묶어 8회까지 (넘치면 '외 N회')
pub fn hits_body(mut hits: Vec<Hit>, header: impl Fn(&str) -> String) -> String {
    // localeCompare: 숫자·구분자만 있는 같은 꼴의 문자열이라 코드 단위 비교와 같다
    hits.sort_by(|a, b| jsx::js_str_cmp(&a.key, &b.key));
    let mut parts: Vec<String> = vec![];
    let mut last = String::new();
    for hit in hits.iter().take(8) {
        if hit.day != last {
            last = hit.day.clone();
            parts.push(header(&hit.day));
        }
        parts.push(hit.text.clone());
    }
    if hits.len() > 8 {
        parts.push(format!("외 {}회", hits.len() - 8));
    }
    parts.join("\n")
}

pub type Rows = Result<Vec<Value>, String>;

/// 같은 회차의 상영표 조회 캐시 (Map<string, Promise>)
#[derive(Default)]
pub struct RowCache(Mutex<HashMap<String, Shared<BoxFuture<'static, Rows>>>>);

impl RowCache {
    pub async fn get(&self, key: String, make: impl FnOnce() -> BoxFuture<'static, Rows>) -> Rows {
        let fut = {
            let mut m = self.0.lock().unwrap();
            m.entry(key).or_insert_with(|| make().shared()).clone()
        };
        fut.await
    }
}

/// config 의 배열 필드 (`!x?.length` 검사와 같은 판단: 비어 있지 않은 배열/문자열)
pub fn nonempty(v: Option<&Value>) -> bool {
    match v {
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::String(s)) => !s.is_empty(),
        _ => false,
    }
}

/// for (const x of v) - 배열이 아니면 TypeError (문자열은 글자 단위)
pub fn iter_of(v: Option<&Value>, expr: &str) -> Result<Vec<Value>, String> {
    match v {
        Some(Value::Array(a)) => Ok(a.clone()),
        Some(Value::String(s)) => Ok(s.chars().map(|c| Value::String(c.to_string())).collect()),
        _ => Err(format!("{expr} is not iterable")),
    }
}

/// Promise.allSettled 결과 처리: 실패 로그 + 전부 실패면 '<name>_all_failed'
pub fn settle(
    results: Vec<Result<(), String>>,
    label: &str,
    all_failed: &str,
) -> Result<(), String> {
    let failed: Vec<&String> = results.iter().filter_map(|r| r.as_ref().err()).collect();
    for f in &failed {
        super::log(&format!("{label} 크롤링 실패: {}: {f}", error_name(f)));
    }
    if !failed.is_empty() && failed.len() == results.len() {
        return Err(all_failed.into());
    }
    Ok(())
}

/// `${err}` 의 오류 이름 - 문자열 오류만 들고 있어서 문구로 TypeError·SyntaxError 를 가린다
fn error_name(m: &str) -> &'static str {
    if m.starts_with("Cannot ")
        || m.contains(" is not a function")
        || m.contains(" is not iterable")
    {
        "TypeError"
    } else if m.contains("JSON") {
        "SyntaxError"
    } else {
        "Error"
    }
}
