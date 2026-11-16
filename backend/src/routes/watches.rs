use std::sync::LazyLock;

use regex::Regex;
use serde_json::{Map, Value, json};

use crate::catalog::is_valid_target;
use crate::core::app::{AppError, HResult, Pre, Reply, Req, Routes, require_auth};
use crate::core::db::{self, P};
use crate::core::js;
use crate::crawlers::get_crawler;
use crate::jsx;
use crate::params;
use crate::watch_schedule::validate_schedule;

// 종료 시점: KST 'YYYY-MM-DD HH:MM' (프론트는 시 단위까지만 받아 분은 00)
static ENDS_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[0-9]{4}-[0-9]{2}-[0-9]{2} ([01][0-9]|2[0-3]):[0-5][0-9]$").unwrap()
});

/// endsAt != null && !ENDS_RE.test(endsAt) (test 는 인자를 문자열로 바꾼다)
fn bad_ends_at(v: Option<&Value>) -> bool {
    jsx::nn(v).is_some() && !ENDS_RE.is_match(&js::to_js_string(v))
}

// 400 응답 헬퍼
fn bad(message: &str) -> Reply {
    Reply::json(400, json!({ "error": "invalid_input", "message": message }))
}

/// `const { first, ... } = req.body` - undefined/null 이면 TypeError, 원시값이면 빈 객체처럼
fn destructure(req: &Req, first: &str) -> Result<Map<String, Value>, AppError> {
    match req.body() {
        None => Err(AppError::type_error(format!(
            "Cannot destructure property '{first}' of 'req.body' as it is undefined."
        ))),
        Some(Value::Null) => Err(AppError::type_error(format!(
            "Cannot destructure property '{first}' of 'req.body' as it is null."
        ))),
        Some(Value::Object(m)) => Ok(m),
        Some(_) => Ok(Map::new()),
    }
}

/// name 검사: typeof name !== 'string' || name.trim() === ''
fn name_ok(v: Option<&Value>) -> bool {
    matches!(v, Some(Value::String(s)) if !js::trim(s).is_empty())
}

/// JSON.stringify(v) (undefined 는 호출하지 않는 곳에서만 쓴다)
fn stringify(v: Option<&Value>) -> String {
    js::stringify(v.unwrap_or(&Value::Null))
}

fn rows(v: Vec<db::Row>) -> Value {
    Value::Array(v.into_iter().map(Value::Object).collect())
}

fn id_param(req: &Req) -> f64 {
    js::string_to_number(req.param("id"))
}

// 활성 알림 목록 (메인페이지) - watch별 최근 알림 내용·시각 포함
async fn list(_req: Req) -> HResult {
    let c = db::conn();
    Ok(Reply::ok(rows(db::all(
        &c,
        "SELECT w.*, ln.body AS last_body, ln.created_at AS last_at
         FROM watches w
         LEFT JOIN notifications ln ON ln.id = (
           SELECT id FROM notifications WHERE watch_id = w.id ORDER BY id DESC LIMIT 1
         )
         WHERE w.deleted_at IS NULL ORDER BY w.id DESC",
        params![],
    )?)))
}

// 종료·삭제된 알림 - 등록(생성) 기준 최근 100개
async fn deleted(_req: Req) -> HResult {
    let c = db::conn();
    Ok(Reply::ok(rows(db::all(
        &c,
        "SELECT * FROM watches WHERE deleted_at IS NOT NULL ORDER BY id DESC LIMIT 100",
        params![],
    )?)))
}

async fn get_one(req: Req) -> HResult {
    let c = db::conn();
    match db::get(
        &c,
        "SELECT * FROM watches WHERE id = ?",
        &[P::Num(id_param(&req))],
    )? {
        Some(r) => Ok(Reply::ok(Value::Object(r))),
        None => Ok(Reply::error(404, "not_found")),
    }
}

async fn create(req: Req) -> HResult {
    let b = destructure(&req, "category")?;
    let (category, provider, name, schedule, config, ends_at) = (
        b.get("category"),
        b.get("provider"),
        b.get("name"),
        b.get("schedule"),
        b.get("config"),
        b.get("endsAt"),
    );
    // CATALOG[category] - 속성 키는 문자열로 바뀐다
    let key = js::to_js_string(category);
    let valid = is_valid_target(&key, provider.and_then(|p| p.as_str()))
        .map_err(|_| AppError::type_error("CATALOG[category]?.includes is not a function"))?;
    if !valid {
        return Ok(bad("알 수 없는 알림 종류입니다"));
    }
    if !name_ok(name) {
        return Ok(bad("이름을 입력해 주세요"));
    }
    if let Some(e) = validate_schedule(schedule) {
        return Ok(bad(e));
    }
    if bad_ends_at(ends_at) {
        return Ok(bad("종료 시점이 올바르지 않습니다"));
    }
    let crawler = get_crawler(provider.and_then(|p| p.as_str()).unwrap_or(""));
    let empty = json!({});
    let cfg = jsx::nn(config).unwrap_or(&empty);
    if let Some(cr) = crawler
        && let Some(e) = cr
            .validate_config(Some(cfg), None)
            .map_err(AppError::type_error)?
        && !e.is_empty()
    {
        return Ok(bad(&e));
    }
    // 생성 시점 기준 상태 (예: 디시는 지금까지의 최신 글 번호 - 이후 글부터 알림)
    let state = match crawler {
        Some(cr) => cr
            .initial_state(Some(cfg), None)
            .map_err(AppError::type_error)?,
        None => None,
    };

    let name = js::trim(name.and_then(|n| n.as_str()).unwrap_or("")).to_string();
    let c = db::conn();
    let r = db::run(
        &c,
        "INSERT INTO watches (category, provider, name, schedule, config, ends_at, state)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
        &[
            db::js_arg(category),
            db::js_arg(provider),
            vec![
                P::from(name),
                P::from(stringify(schedule)),
                P::from(js::stringify(cfg)),
            ],
            db::js_arg(jsx::nn(ends_at)),
            vec![P::from(state.map(|s| js::stringify(&s)))],
        ]
        .concat(),
    )?;
    Ok(Reply::json(201, json!({ "id": r.last_insert_rowid })))
}

async fn update(req: Req) -> HResult {
    let id = id_param(&req);
    let row = {
        let c = db::conn();
        db::get(
            &c,
            "SELECT * FROM watches WHERE id = ? AND deleted_at IS NULL",
            &[P::Num(id)],
        )?
    };
    let Some(row) = row else {
        return Ok(Reply::error(404, "not_found"));
    };

    let b = destructure(&req, "name")?;
    let (name, schedule, config, ends_at) = (
        b.get("name"),
        b.get("schedule"),
        b.get("config"),
        b.get("endsAt"),
    );
    if name.is_some() && !name_ok(name) {
        return Ok(bad("이름을 입력해 주세요"));
    }
    if schedule.is_some()
        && let Some(e) = validate_schedule(schedule)
    {
        return Ok(bad(e));
    }
    if bad_ends_at(ends_at) {
        return Ok(bad("종료 시점이 올바르지 않습니다"));
    }
    // 감시 대상이 바뀌면 크롤러가 state를 새로 잡는다 (null이면 유지)
    let mut state = row.get("state").cloned().unwrap_or(Value::Null);
    if config.is_some() {
        let crawler = get_crawler(db::text(&row, "provider").unwrap_or(""));
        if let Some(cr) = crawler {
            if let Some(e) = cr
                .validate_config(config, Some(id))
                .map_err(AppError::type_error)?
                && !e.is_empty()
            {
                return Ok(bad(&e));
            }
            if cr == crate::crawlers::Crawler::Dcinside {
                let prev = jsx::json_parse(&js::to_js_string(row.get("config")))
                    .map_err(AppError::internal)?;
                if let Some(next) = cr
                    .initial_state(config, Some(&prev))
                    .map_err(AppError::type_error)?
                    && js::truthy(Some(&next))
                {
                    state = Value::String(js::stringify(&next));
                }
            }
        }
    }

    let name_p = match name {
        Some(Value::String(s)) => P::from(js::trim(s)),
        _ => db::from_json(row.get("name"))?,
    };
    let c = db::conn();
    db::run(
        &c,
        "UPDATE watches
       SET name = ?, schedule = ?, config = ?, ends_at = ?, state = ?, updated_at = datetime('now')
       WHERE id = ?",
        &[
            name_p,
            match schedule {
                Some(s) => P::from(js::stringify(s)),
                None => db::from_json(row.get("schedule"))?,
            },
            match config {
                Some(s) => P::from(js::stringify(s)),
                None => db::from_json(row.get("config"))?,
            },
            db::from_json(row.get("ends_at"))?,
            db::from_json(Some(&state))?,
            P::Num(id),
        ]
        .into_iter()
        .enumerate()
        .flat_map(|(i, p)| match (i, ends_at) {
            // endsAt: undefined = 유지, null = 상시로 해제
            (3, Some(v)) => db::js_arg(Some(v)),
            _ => vec![p],
        })
        .collect::<Vec<_>>(),
    )?;
    Ok(Reply::ok(json!({ "ok": true })))
}

// 임시 중지·재개 - 중지 동안은 크롤링과 알림 모두 쉰다
async fn toggle(req: Req) -> HResult {
    let enabled = req.field("enabled")?;
    let c = db::conn();
    let r = db::run(
        &c,
        "UPDATE watches SET enabled = ?, updated_at = datetime('now')
         WHERE id = ? AND deleted_at IS NULL",
        &[
            P::Num(if js::truthy(enabled.as_ref()) {
                1.0
            } else {
                0.0
            }),
            P::Num(id_param(&req)),
        ],
    )?;
    if r.changes == 0 {
        return Ok(Reply::error(404, "not_found"));
    }
    Ok(Reply::ok(json!({ "ok": true })))
}

// 목록에서 제거 (soft delete - 종료된 알림 페이지에서 복구 가능)
async fn remove(req: Req) -> HResult {
    let c = db::conn();
    let r = db::run(
        &c,
        "UPDATE watches SET deleted_at = datetime('now'), updated_at = datetime('now')
         WHERE id = ? AND deleted_at IS NULL",
        &[P::Num(id_param(&req))],
    )?;
    if r.changes == 0 {
        return Ok(Reply::error(404, "not_found"));
    }
    Ok(Reply::ok(json!({ "ok": true })))
}

// 복구 - 종료 시점은 해제되어 상시로 되살아난다
async fn restore(req: Req) -> HResult {
    let c = db::conn();
    let r = db::run(
        &c,
        "UPDATE watches SET deleted_at = NULL, ends_at = NULL, updated_at = datetime('now')
         WHERE id = ? AND deleted_at IS NOT NULL",
        &[P::Num(id_param(&req))],
    )?;
    if r.changes == 0 {
        return Ok(Reply::error(404, "not_found"));
    }
    Ok(Reply::ok(json!({ "ok": true })))
}

pub fn register(r: &mut Routes) {
    let (p, a): (&str, Option<Pre>) = ("/api/watches", Some(require_auth));
    r.add("GET", p, "/", a, list);
    r.add("GET", p, "/deleted", a, deleted);
    r.add("GET", p, "/:id", a, get_one);
    r.add("POST", p, "/", a, create);
    r.add("PUT", p, "/:id", a, update);
    r.add("POST", p, "/:id/toggle", a, toggle);
    r.add("DELETE", p, "/:id", a, remove);
    r.add("POST", p, "/:id/restore", a, restore);
}
