use std::future::Future;
use std::sync::LazyLock;
use std::time::Duration;

use regex::Regex;
use serde_json::Value;

use super::WatchRow;
use crate::core::db;
use crate::core::js;
use crate::crawl_log::crawl_log;
use crate::jsx;
use crate::notify;
use crate::params;

// 수집 재시도 간 대기 (1차 실패 후 1초, 2차 실패 후 2초)
const RETRY_DELAYS: [u64; 2] = [1_000, 2_000];

static HTTP_4XX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"_http_4[0-9][0-9]").unwrap());

/// 수집 호출을 최대 3회 시도한다. 중간 실패는 일자별 로그에 남긴다.
/// label: 로그 표기용 이름 (예: 'CGV 상영표 0013 20260821')
/// f: 시도 함수 - attempt(0부터)를 받아 시도별 전략(쿠키 갱신 등)을 쓸 수 있다
pub async fn with_retry<T, F, Fut>(label: &str, mut f: F) -> Result<T, String>
where
    F: FnMut(u32) -> Fut,
    Fut: Future<Output = Result<T, String>>,
{
    let mut last_error = String::new();
    for attempt in 0..3u32 {
        match f(attempt).await {
            Ok(v) => return Ok(v),
            Err(message) => {
                // 4xx는 재시도해도 같은 결과 (미취급 상품 등 정상 흐름) - 즉시 중단
                if HTTP_4XX.is_match(&message) {
                    return Err(message);
                }
                crawl_log(&format!("시도 {}/3 실패: {label} - {message}", attempt + 1));
                last_error = message;
                if attempt < 2 {
                    tokio::time::sleep(Duration::from_millis(RETRY_DELAYS[attempt as usize])).await;
                }
            }
        }
    }
    Err(last_error)
}

/// JSON.parse(watch.state ?? '{}') - 객체가 아니면 속성 대입에서 TS 처럼 TypeError
fn parse_state(watch: &WatchRow) -> Result<Value, String> {
    jsx::json_parse(watch.state.as_deref().unwrap_or("{}"))
}

fn failing(state: &Value) -> Result<bool, String> {
    Ok(js::truthy(jsx::prop(Some(state), "failing")?))
}

/// state.k = v (원시값이면 strict mode TypeError)
pub fn set_state_key(state: &mut Value, k: &str, v: Value) -> Result<(), String> {
    match state {
        Value::Object(m) => {
            m.insert(k.to_string(), v);
            Ok(())
        }
        Value::Array(_) => Ok(()),
        other => Err(format!(
            "Cannot create property '{k}' on {} '{}'",
            match other {
                Value::String(_) => "string",
                Value::Bool(_) => "boolean",
                _ => "number",
            },
            js::to_js_string(Some(other))
        )),
    }
}

pub fn save_state(watch: &mut WatchRow, state: &Value) -> Result<(), String> {
    let s = js::stringify(state);
    watch.state = Some(s.clone());
    let c = db::conn();
    db::run(
        &c,
        "UPDATE watches SET state = ? WHERE id = ?",
        params![s, watch.id],
    )
    .map_err(|e| e.0)?;
    Ok(())
}

/// 최종 수집 실패 처리: 일자별 로그 기록 + 실패 알림 발송.
/// 연속 실패는 최초 1회만 알림 (watch.state.failing으로 억제, 복구되면 리셋)
pub fn report_crawl_failure(watch: &mut WatchRow, message: &str) -> Result<(), String> {
    crawl_log(&format!(
        "수집 실패: watch {} \"{}\" ({}) - {message}",
        watch.id, watch.name, watch.provider
    ));
    let mut state = parse_state(watch)?;
    if failing(&state)? {
        return Ok(());
    }
    set_state_key(&mut state, "failing", Value::Bool(true))?;
    save_state(watch, &state)?;
    notify::enqueue(
        &watch.provider,
        &format!("⚠️ 수집 실패 · {}", watch.name),
        &format!("{message}\n복구되면 알림이 재개돼요"),
        None,
        watch.id,
    )
    .map_err(|e| e.0)?;
    Ok(())
}

/// 수집 성공 시 실패 상태 해제 (다음 실패 때 다시 알림이 가도록)
pub fn clear_crawl_failure(watch: &mut WatchRow) -> Result<(), String> {
    let mut state = parse_state(watch)?;
    if !failing(&state)? {
        return Ok(());
    }
    if let Value::Object(m) = &mut state {
        m.shift_remove("failing");
    }
    save_state(watch, &state)
}
