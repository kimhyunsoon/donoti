pub mod auth;
pub mod links;
pub mod movies;
pub mod notifications;
pub mod push;
pub mod settings;
pub mod stocks;
pub mod watches;

use serde_json::Value;

use crate::core::app::{AppError, Req};
use crate::core::js;

/// `req.body` (undefined/null 이면 `b.x` 에서 TypeError)
pub fn body_obj(req: &Req) -> Result<serde_json::Map<String, Value>, AppError> {
    match req.body() {
        None => Err(AppError::type_error(
            "Cannot read properties of undefined (reading '{k}')",
        )),
        Some(Value::Null) => Err(AppError::type_error(
            "Cannot read properties of null (reading '{k}')",
        )),
        Some(Value::Object(m)) => Ok(m),
        Some(_) => Ok(Default::default()),
    }
}

/// 첫 속성 이름을 넣은 TypeError 문구로 body 를 꺼낸다
pub fn body_for(req: &Req, first: &str) -> Result<serde_json::Map<String, Value>, AppError> {
    body_obj(req).map_err(|e| AppError::type_error(e.message.replace("{k}", first)))
}

/// `expr.trim()` - 문자열이 아니면 TS 와 같은 TypeError
pub fn trim_of(v: Option<&Value>, expr: &str) -> Result<String, AppError> {
    match v {
        Some(Value::String(s)) => Ok(js::trim(s).to_string()),
        None => Err(AppError::type_error(
            "Cannot read properties of undefined (reading 'trim')",
        )),
        Some(Value::Null) => Err(AppError::type_error(
            "Cannot read properties of null (reading 'trim')",
        )),
        Some(_) => Err(AppError::type_error(format!(
            "{expr}.trim is not a function"
        ))),
    }
}

/// `v ?? null`
pub fn or_null(v: Option<&Value>) -> Option<&Value> {
    match v {
        None | Some(Value::Null) => None,
        x => x,
    }
}
