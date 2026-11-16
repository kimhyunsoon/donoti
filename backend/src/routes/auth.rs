use serde_json::{Value, json};

use crate::core::app::{AppError, HResult, Reply, Req, Routes, require_auth};
use crate::core::{db, password};
use crate::params;

/// `const { username, password } = req.body` - body 가 undefined/null 이면 TypeError
pub fn destructure(req: &Req, first: &str) -> Result<(Option<Value>, Option<Value>), AppError> {
    match req.body() {
        None => Err(AppError::type_error(format!(
            "Cannot destructure property '{first}' of 'req.body' as it is undefined."
        ))),
        Some(Value::Null) => Err(AppError::type_error(format!(
            "Cannot destructure property '{first}' of 'req.body' as it is null."
        ))),
        Some(Value::Object(m)) => Ok((m.get("username").cloned(), m.get("password").cloned())),
        Some(_) => Ok((None, None)),
    }
}

async fn login(req: Req) -> HResult {
    let (username, password) = destructure(&req, "username")?;
    let user = {
        let c = db::conn();
        db::get(
            &c,
            "SELECT id, username, password_hash FROM users WHERE username = ?",
            &db::js_arg(username.as_ref()),
        )?
    };
    let ok = match &user {
        None => false,
        Some(u) => {
            let Some(Value::String(pw)) = password else {
                return Err(AppError::type_error("pchstr must be a non-empty string"));
            };
            password::verify(db::text(u, "password_hash").unwrap_or("").to_string(), pw).await
        }
    };
    let Some(user) = user.filter(|_| ok) else {
        return Ok(Reply::error(401, "invalid_credentials"));
    };
    req.with_session(|s| {
        if let Some(s) = s {
            s.set("userId", user["id"].clone());
        }
    });
    Ok(Reply::ok(
        json!({ "id": user["id"], "username": user["username"] }),
    ))
}

async fn logout(req: Req) -> HResult {
    let s = req.with_session(|s| s.take());
    if let Some(s) = s {
        crate::core::session::destroy(&s)?;
    }
    Ok(Reply::ok(json!({ "ok": true })))
}

async fn me(req: Req) -> HResult {
    let c = db::conn();
    match db::get(
        &c,
        "SELECT id, username FROM users WHERE id = ?",
        params![req.user_id()],
    )? {
        Some(u) => Ok(Reply::ok(Value::Object(u))),
        None => Ok(Reply::empty(200)),
    }
}

pub fn register(r: &mut Routes) {
    let p = "/api/auth";
    r.add("POST", p, "/login", None, login);
    r.add("POST", p, "/logout", Some(require_auth), logout);
    r.add("GET", p, "/me", Some(require_auth), me);
}
