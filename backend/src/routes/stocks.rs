use serde_json::Value;

use crate::core::app::{AppError, HResult, Pre, Reply, Req, Routes, require_auth};
use crate::core::db::{self, P};
use crate::core::js;

// 주식 종목 마스터 검색 (알림 등록 폼에서 코드·이름으로 검색)
async fn search(req: Req) -> HResult {
    // (req.query.q ?? '').trim()
    let q = match req.query("q") {
        None => String::new(),
        Some(Value::String(s)) => js::trim(s).to_string(),
        Some(_) => {
            return Err(AppError::type_error(
                "(req.query.q ?? \"\").trim is not a function",
            ));
        }
    };
    if q.is_empty() {
        return Ok(Reply::ok(Value::Array(vec![])));
    }
    let like = format!("%{q}%");
    let prefix = format!("{q}%");
    // 이름 접두 일치 → 코드 접두 일치 → 나머지 순으로 정렬
    let c = db::conn();
    let rows = db::all(
        &c,
        "SELECT code, name, kind, market, keywords FROM stock_symbols
         WHERE code LIKE ? OR name LIKE ? OR keywords LIKE ?
         ORDER BY CASE WHEN name LIKE ? THEN 0 WHEN code LIKE ? THEN 1 ELSE 2 END, name
         LIMIT 20",
        &[
            P::from(&like),
            P::from(&like),
            P::from(&like),
            P::from(&prefix),
            P::from(&prefix),
        ],
    )?;
    Ok(Reply::ok(Value::Array(
        rows.into_iter().map(Value::Object).collect(),
    )))
}

pub fn register(r: &mut Routes) {
    let (p, a): (&str, Option<Pre>) = ("/api/stocks", Some(require_auth));
    r.add("GET", p, "/", a, search);
}
