use serde_json::{Value, json};

use crate::config::CONFIG;
use crate::core::app::{AppError, HResult, Pre, Reply, Req, Routes, require_auth};
use crate::core::db;

async fn vapid_public_key(_req: Req) -> HResult {
    Ok(Reply::ok(json!({ "key": CONFIG.vapid.public_key })))
}

async fn subscribe(req: Req) -> HResult {
    // const { endpoint, keys } = req.body
    let b = match req.body() {
        None => {
            return Err(AppError::type_error(
                "Cannot destructure property 'endpoint' of 'req.body' as it is undefined.",
            ));
        }
        Some(Value::Null) => {
            return Err(AppError::type_error(
                "Cannot destructure property 'endpoint' of 'req.body' as it is null.",
            ));
        }
        Some(Value::Object(m)) => m,
        Some(_) => Default::default(),
    };
    let keys = match b.get("keys") {
        None => {
            return Err(AppError::type_error(
                "Cannot read properties of undefined (reading 'p256dh')",
            ));
        }
        Some(Value::Null) => {
            return Err(AppError::type_error(
                "Cannot read properties of null (reading 'p256dh')",
            ));
        }
        Some(Value::Object(k)) => k.clone(),
        Some(_) => Default::default(),
    };
    let c = db::conn();
    db::run(
        &c,
        "INSERT INTO push_subscriptions (endpoint, keys_p256dh, keys_auth) VALUES (?, ?, ?)
       ON CONFLICT(endpoint) DO UPDATE SET keys_p256dh = excluded.keys_p256dh, keys_auth = excluded.keys_auth",
        &[db::js_arg(b.get("endpoint")), db::js_arg(keys.get("p256dh")), db::js_arg(keys.get("auth"))].concat(),
    )?;
    Ok(Reply::json(201, json!({ "ok": true })))
}

// 이 기기의 알림 끄기 (프론트가 구독 해지 후 호출)
async fn unsubscribe(req: Req) -> HResult {
    let endpoint = req.field("endpoint")?;
    let c = db::conn();
    db::run(
        &c,
        "DELETE FROM push_subscriptions WHERE endpoint = ?",
        &db::js_arg(endpoint.as_ref()),
    )?;
    Ok(Reply::ok(json!({ "ok": true })))
}

pub fn register(r: &mut Routes) {
    let (p, a): (&str, Option<Pre>) = ("/api/push", Some(require_auth));
    r.add("GET", p, "/vapid-public-key", a, vapid_public_key);
    r.add("POST", p, "/subscriptions", a, subscribe);
    r.add("DELETE", p, "/subscriptions", a, unsubscribe);
}
