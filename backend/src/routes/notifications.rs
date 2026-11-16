use serde_json::{Value, json};

use crate::core::app::{HResult, Pre, Reply, Req, Routes, require_auth};
use crate::core::db::{self, P};
use crate::core::js;
use crate::notify::{enqueue_args, now_ms};
use crate::params;

fn rows(v: Vec<db::Row>) -> Value {
    Value::Array(v.into_iter().map(Value::Object).collect())
}

// 알림센터는 항상 최근 100개까지만 보여준다
async fn list(req: Req) -> HResult {
    // Math.min(req.query.limit ?? 100, 100)
    let limit = match req.query("limit") {
        None => 100.0,
        v => {
            let n = js::to_number(v);
            if n.is_nan() { f64::NAN } else { n.min(100.0) }
        }
    };
    let c = db::conn();
    let status = req.query("status");
    if js::truthy(status) {
        return Ok(Reply::ok(rows(db::all(
            &c,
            "SELECT * FROM notifications WHERE status = ? ORDER BY id DESC LIMIT ?",
            &[db::js_arg(status), vec![P::Num(limit)]].concat(),
        )?)));
    }
    Ok(Reply::ok(rows(db::all(
        &c,
        "SELECT * FROM notifications ORDER BY id DESC LIMIT ?",
        &[P::Num(limit)],
    )?)))
}

/// `x ?? d` 를 파라미터로
fn or_p(v: Option<Value>, d: P) -> Vec<P> {
    match v {
        None | Some(Value::Null) => vec![d],
        v => db::js_arg(v.as_ref()),
    }
}

// 수동 등록 (파이프라인 테스트용 - 워커가 몇 초 내 이 기기로 푸시를 보낸다)
async fn create(req: Req) -> HResult {
    let title = req.field("title")?;
    let body = req.field("body")?;
    let url = req.field("url")?;
    let id = enqueue_args(
        [
            vec![P::from("manual")],
            db::js_arg(title.as_ref()),
            or_p(body, P::from("")),
            or_p(url, P::Null),
            vec![P::Null],
        ]
        .concat(),
    )?;
    Ok(Reply::json(201, json!({ "id": id })))
}

fn unread(c: &rusqlite::Connection) -> db::Result<Value> {
    Ok(db::get(
        c,
        "SELECT COUNT(*) AS c FROM notifications WHERE status = 'sent' AND read_at IS NULL",
        params![],
    )?
    .and_then(|r| r.get("c").cloned())
    .unwrap_or(Value::Null))
}

// 안읽은 알림 수 (알림센터 배지용)
async fn unread_count(_req: Req) -> HResult {
    let c = db::conn();
    Ok(Reply::ok(json!({ "count": unread(&c)? })))
}

// 전부 읽음 처리 (알림센터 '모두 읽음' 버튼)
async fn read_all(_req: Req) -> HResult {
    let c = db::conn();
    db::run(
        &c,
        "UPDATE notifications SET read_at = datetime('now') WHERE status = 'sent' AND read_at IS NULL",
        params![],
    )?;
    Ok(Reply::ok(json!({ "ok": true })))
}

// 개별 읽음 처리 (알림센터 스와이프·링크 이동·OS 알림 클릭) - 남은 안읽음 수를 함께 반환해 앱 배지 갱신에 쓴다
async fn read_one(req: Req) -> HResult {
    let c = db::conn();
    db::run(
        &c,
        "UPDATE notifications SET read_at = datetime('now') WHERE id = ? AND read_at IS NULL",
        &[P::Num(js::string_to_number(req.param("id")))],
    )?;
    Ok(Reply::ok(json!({ "ok": true, "unread": unread(&c)? })))
}

// 최종 실패한 알림을 재시도 큐로 되돌린다
async fn retry(req: Req) -> HResult {
    let c = db::conn();
    let r = db::run(
        &c,
        "UPDATE notifications
         SET status = 'pending', attempts = 0, next_attempt_at = ?, last_error = NULL
         WHERE id = ? AND status = 'failed'",
        &[
            P::Num(now_ms() as f64),
            P::Num(js::string_to_number(req.param("id"))),
        ],
    )?;
    if r.changes == 0 {
        return Ok(Reply::error(404, "not_found"));
    }
    Ok(Reply::ok(json!({ "ok": true })))
}

pub fn register(r: &mut Routes) {
    let (p, a): (&str, Option<Pre>) = ("/api/notifications", Some(require_auth));
    r.add("GET", p, "/", a, list);
    r.add("POST", p, "/", a, create);
    r.add("GET", p, "/unread-count", a, unread_count);
    r.add("POST", p, "/read-all", a, read_all);
    r.add("POST", p, "/:id/read", a, read_one);
    r.add("POST", p, "/:id/retry", a, retry);
}
