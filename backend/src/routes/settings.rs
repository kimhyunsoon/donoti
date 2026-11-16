use serde_json::{Value, json};

use crate::core::app::{HResult, Pre, Reply, Req, Routes, require_auth};
use crate::core::db::{self, P};
use crate::params;

// 범용 key-value 설정 (크롤러 주기 등은 다음 단계에서 이 위에 얹는다)
async fn list(_req: Req) -> HResult {
    let c = db::conn();
    let rows = db::all(&c, "SELECT * FROM settings ORDER BY key", params![])?;
    Ok(Reply::ok(Value::Array(
        rows.into_iter().map(Value::Object).collect(),
    )))
}

async fn put(req: Req) -> HResult {
    let value = req.field("value")?;
    let c = db::conn();
    db::run(
        &c,
        "INSERT INTO settings (key, value, updated_at) VALUES (?, ?, datetime('now'))
       ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = datetime('now')",
        &[vec![P::from(req.param("key"))], db::js_arg(value.as_ref())].concat(),
    )?;
    Ok(Reply::ok(json!({ "ok": true })))
}

pub fn register(r: &mut Routes) {
    let (p, a): (&str, Option<Pre>) = ("/api/settings", Some(require_auth));
    r.add("GET", p, "/", a, list);
    r.add("PUT", p, "/:key", a, put);
}
