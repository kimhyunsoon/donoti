// 4개 앱 공통 사본 - 고칠 때 네 곳을 같이 고친다.
//
// @fastify/session 11.1.2 + @fastify/cookie 11.1.2 (cookie 2.0.1) 이식. 기존 쿠키·sessions 행과 호환:
// - 쿠키 `sessionId` = `<sid>.<HMAC-SHA256(sid, secret) base64, '=' 제거>` (값은 인코딩 없이: cookie-octet 문자만이라)
// - sid = 랜덤 24바이트 base64url (32자)
// - sessions.sess = JSON.stringify(session) = {"cookie":{expires,originalMaxAge,originalExpires?,sameSite,secure,path,httpOnly,domain},<데이터>}
// - saveUninitialized=false, rolling=true: 쿠키가 지금 세션과 같으면 요청마다 재저장 + Set-Cookie(만료 +90일)
// - 만료 판정: 행의 expires < now 이면 없음, 복원된 쿠키의 originalExpires(직전 만료) || expires <= now 이면 파기 후 새 세션
use base64::Engine;
use base64::engine::general_purpose::{STANDARD_NO_PAD, URL_SAFE_NO_PAD};
use hmac::{Hmac, KeyInit, Mac};
use serde_json::{Map, Value, json};
use sha2::Sha256;
use subtle::ConstantTimeEq;

use super::db::{self, P};
use super::js;

pub const COOKIE_NAME: &str = "sessionId";
pub const MAX_AGE_MS: f64 = 1000.0 * 60.0 * 60.0 * 24.0 * 90.0; // 로그인 유지 90일
const FALLBACK_TTL_MS: f64 = MAX_AGE_MS;

pub struct Config {
    pub secret: String,
    /// TS: secure: process.env.NODE_ENV === 'production'
    pub secure: bool,
}

/// 세션의 cookie 부분 (lib/cookie.js 의 Cookie)
#[derive(Clone, Debug)]
struct Cookie {
    expires: Option<f64>, // ms
    original_max_age: Option<f64>,
    original_expires: Option<f64>,
    same_site: Value, // 'lax' | null
    secure: Value,    // bool | null
    path: String,
    http_only: bool,
    domain: Value,
    partitioned: Option<Value>,
}

impl Cookie {
    /// new Cookie(opts) - opts 는 cookieOpts 와 (복원이면) 저장된 cookie 를 합친 것
    fn new(opts: &Map<String, Value>, now: f64) -> Cookie {
        let num = |k: &str| opts.get(k).and_then(|v| v.as_f64()).filter(|f| *f != 0.0);
        let original_max_age = num("originalMaxAge").or_else(|| num("maxAge"));
        let expires_opt = opts.get("expires").and_then(parse_date_value);
        let mut c = Cookie {
            expires: None,
            original_max_age: None,
            original_expires: None,
            same_site: match opts.get("sameSite") {
                Some(v) if js::truthy(Some(v)) => v.clone(),
                _ => Value::Null,
            },
            secure: match opts.get("secure") {
                None | Some(Value::Null) => Value::Null,
                Some(v) => v.clone(),
            },
            path: match opts.get("path") {
                Some(Value::String(s)) if !s.is_empty() => s.clone(),
                _ => "/".into(),
            },
            http_only: match opts.get("httpOnly") {
                None => true,
                Some(v) => js::truthy(Some(v)),
            },
            domain: match opts.get("domain") {
                Some(v) if js::truthy(Some(v)) => v.clone(),
                _ => Value::Null,
            },
            partitioned: opts.get("partitioned").cloned(),
        };
        if let Some(e) = expires_opt {
            c.original_expires = Some(e);
        }
        if let Some(ms) = original_max_age {
            c.expires = Some(now + ms);
            c.original_max_age = Some(ms);
        } else if let Some(e) = expires_opt {
            c.expires = Some(e);
            c.original_max_age = None;
        }
        c
    }

    fn to_json(&self) -> Value {
        let mut m = Map::new();
        m.insert(
            "expires".into(),
            self.expires
                .map(|e| Value::String(iso(e)))
                .unwrap_or(Value::Null),
        );
        m.insert(
            "originalMaxAge".into(),
            self.original_max_age.map(js::num).unwrap_or(Value::Null),
        );
        if let Some(e) = self.original_expires {
            m.insert("originalExpires".into(), Value::String(iso(e)));
        }
        m.insert("sameSite".into(), self.same_site.clone());
        m.insert("secure".into(), self.secure.clone());
        m.insert("path".into(), Value::String(self.path.clone()));
        m.insert("httpOnly".into(), Value::Bool(self.http_only));
        m.insert("domain".into(), self.domain.clone());
        if let Some(p) = &self.partitioned {
            m.insert("partitioned".into(), p.clone());
        }
        Value::Object(m)
    }

    /// reply.setCookie(name, value, cookie.toJSON()) 의 Set-Cookie 문자열 (cookie 2.0.1 stringifySetCookie 순서)
    fn set_cookie(&self, value: &str) -> String {
        let mut s = format!("{COOKIE_NAME}={value}");
        if let Value::String(d) = &self.domain {
            s.push_str(&format!("; Domain={d}"));
        }
        s.push_str(&format!("; Path={}", self.path));
        if let Some(e) = self.expires {
            s.push_str(&format!("; Expires={}", utc_string(e)));
        }
        if self.http_only {
            s.push_str("; HttpOnly");
        }
        if self.secure == Value::Bool(true) {
            s.push_str("; Secure");
        }
        if js::truthy(self.partitioned.as_ref()) {
            s.push_str("; Partitioned");
        }
        // setCookie 는 { sameSite: 'lax', ...opts } 라 저장값이 null 이면 생략
        if let Value::String(ss) = &self.same_site {
            match ss.to_ascii_lowercase().as_str() {
                "strict" => s.push_str("; SameSite=Strict"),
                "lax" => s.push_str("; SameSite=Lax"),
                "none" => s.push_str("; SameSite=None"),
                _ => {}
            }
        }
        s
    }
}

/// 쿠키 지우기 (reply.clearCookie(name, { path: '/' }))
pub fn clear_cookie() -> String {
    format!(
        "{COOKIE_NAME}=; Max-Age=0; Path=/; Expires=Thu, 01 Jan 1970 00:00:00 GMT; SameSite=Lax"
    )
}

pub struct Session {
    sid: String,
    encrypted: String,
    cookie: Cookie,
    /// cookie 를 뺀 나머지 키 (userId 등), 삽입 순
    data: Map<String, Value>,
    persisted_hash: String,
    saved: bool,
}

impl Session {
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.data.get(key)
    }

    pub fn set(&mut self, key: &str, v: Value) {
        self.data.insert(key.to_string(), v);
    }

    /// req.session.userId (truthy 일 때만)
    pub fn user_id(&self) -> Option<i64> {
        let v = self.data.get("userId");
        if js::truthy(v) {
            v.and_then(|v| v.as_i64().or_else(|| v.as_f64().map(|f| f as i64)))
        } else {
            None
        }
    }

    fn hash(&self) -> String {
        // safe-stable-stringify(키 정렬)로 cookie 를 뺀 내용을 비교한다
        let mut keys: Vec<&String> = self.data.keys().collect();
        keys.sort();
        let sorted: Map<String, Value> = keys
            .into_iter()
            .map(|k| (k.clone(), sort_keys(&self.data[k])))
            .collect();
        js::stringify(&Value::Object(sorted))
    }

    fn is_modified(&self) -> bool {
        self.persisted_hash != self.hash()
    }

    fn to_json(&self) -> String {
        let mut m = Map::new();
        m.insert("cookie".into(), self.cookie.to_json());
        for (k, v) in &self.data {
            m.insert(k.clone(), v.clone());
        }
        js::stringify(&Value::Object(m))
    }

    fn store_set(&self) -> db::Result<()> {
        let expires = self
            .cookie
            .expires
            .unwrap_or_else(|| now_ms() + FALLBACK_TTL_MS);
        let c = db::conn();
        db::run(
            &c,
            "INSERT OR REPLACE INTO sessions (sid, sess, expires) VALUES (?, ?, ?)",
            crate::params![self.sid.clone(), self.to_json(), expires],
        )?;
        Ok(())
    }

    fn save(&mut self) -> db::Result<()> {
        self.store_set()?;
        self.saved = true;
        self.persisted_hash = self.hash();
        Ok(())
    }
}

fn sort_keys(v: &Value) -> Value {
    match v {
        Value::Object(m) => {
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort();
            Value::Object(
                keys.into_iter()
                    .map(|k| (k.clone(), sort_keys(&m[k])))
                    .collect(),
            )
        }
        Value::Array(a) => Value::Array(a.iter().map(sort_keys).collect()),
        _ => v.clone(),
    }
}

pub fn now_ms() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as f64
}

fn iso(ms: f64) -> String {
    chrono::DateTime::from_timestamp_millis(ms as i64)
        .map(|d| d.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string())
        .unwrap_or_default()
}

fn utc_string(ms: f64) -> String {
    chrono::DateTime::from_timestamp_millis(ms as i64)
        .map(|d| d.format("%a, %d %b %Y %H:%M:%S GMT").to_string())
        .unwrap_or_default()
}

fn parse_date_value(v: &Value) -> Option<f64> {
    match v {
        Value::String(s) => chrono::DateTime::parse_from_rfc3339(s)
            .ok()
            .map(|d| d.timestamp_millis() as f64),
        Value::Number(n) => n.as_f64(),
        _ => None,
    }
}

type HmacSha256 = Hmac<Sha256>;

fn signature(value: &str, secret: &str) -> String {
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).expect("hmac key");
    mac.update(value.as_bytes());
    STANDARD_NO_PAD.encode(mac.finalize().into_bytes())
}

pub fn sign(value: &str, secret: &str) -> String {
    format!("{value}.{}", signature(value, secret))
}

/// signer.unsign - lastIndexOf('.') 기준 (점이 없으면 JS slice(0,-1) 동작)
pub fn unsign(signed: &str, secret: &str) -> Option<String> {
    let (value, actual) = match signed.rfind('.') {
        Some(i) => (&signed[..i], &signed[i + 1..]),
        None => {
            let u: Vec<u16> = signed.encode_utf16().collect();
            let v = String::from_utf16_lossy(&u[..u.len().saturating_sub(1)]);
            return if signature(&v, secret)
                .as_bytes()
                .ct_eq(signed.as_bytes())
                .into()
            {
                Some(v)
            } else {
                None
            };
        }
    };
    let expected = signature(value, secret);
    if expected.len() == actual.len() && bool::from(expected.as_bytes().ct_eq(actual.as_bytes())) {
        Some(value.to_string())
    } else {
        None
    }
}

fn generate_id() -> String {
    let mut b = [0u8; 24];
    rand::fill(&mut b);
    URL_SAFE_NO_PAD.encode(b)
}

fn cookie_opts(cfg: &Config) -> Map<String, Value> {
    let v =
        json!({ "secure": cfg.secure, "httpOnly": true, "sameSite": "lax", "maxAge": MAX_AGE_MS });
    v.as_object().unwrap().clone()
}

fn new_session(cfg: &Config, sid: Option<String>) -> Session {
    let sid = sid.unwrap_or_else(generate_id);
    let encrypted = sign(&sid, &cfg.secret);
    let mut s = Session {
        sid,
        encrypted,
        cookie: Cookie::new(&cookie_opts(cfg), now_ms()),
        data: Map::new(),
        persisted_hash: String::new(),
        saved: false,
    };
    s.persisted_hash = s.hash();
    s
}

/// Cookie 헤더에서 sessionId (cookie 2.0.1 parseCookie: 첫 값 우선, '%' 가 있으면 decodeURIComponent)
pub fn cookie_value(header: Option<&str>) -> Option<String> {
    let h = header?;
    for part in h.split(';') {
        let Some(eq) = part.find('=') else { continue };
        let key = part[..eq].trim_matches([' ', '\t']);
        if key == COOKIE_NAME {
            let raw = part[eq + 1..].trim_matches([' ', '\t']);
            if !raw.contains('%') {
                return Some(raw.to_string());
            }
            return Some(
                percent_encoding::percent_decode_str(raw)
                    .decode_utf8()
                    .map(|s| s.into_owned())
                    .unwrap_or_else(|_| raw.to_string()),
            );
        }
    }
    None
}

/// onRequest: 쿠키로 세션 복원 (없거나 틀리면 새 세션, 저장은 안 함)
pub fn load(cfg: &Config, cookie: Option<&str>) -> db::Result<Session> {
    let Some(cookie) = cookie.filter(|c| !c.is_empty()) else {
        return Ok(new_session(cfg, None));
    };
    let Some(sid) = unsign(cookie, &cfg.secret) else {
        return Ok(new_session(cfg, None));
    };
    let row = {
        let c = db::conn();
        db::get(
            &c,
            "SELECT sess, expires FROM sessions WHERE sid = ?",
            crate::params![sid.clone()],
        )?
    };
    let Some(row) = row else {
        return Ok(new_session(cfg, None));
    };
    let expires = row.get("expires").and_then(|v| v.as_f64()).unwrap_or(0.0);
    if expires < now_ms() {
        return Ok(new_session(cfg, None));
    }
    let parsed: Value = serde_json::from_str(db::text(&row, "sess").unwrap_or("{}"))
        .map_err(|e| db::DbError::msg(e.to_string()))?;
    let parsed = parsed.as_object().cloned().unwrap_or_default();
    // restoredCookieOpts = { ...cookieOpts, ...previousCookie }
    let mut opts = cookie_opts(cfg);
    if let Some(Value::Object(prev)) = parsed.get("cookie") {
        for (k, v) in prev {
            opts.insert(k.clone(), v.clone());
        }
    }
    let encrypted = sign(&sid, &cfg.secret);
    let mut s = Session {
        sid,
        encrypted,
        cookie: Cookie::new(&opts, now_ms()),
        data: parsed
            .into_iter()
            .filter(|(k, _)| k != "cookie" && k != "sessionId" && k != "encryptedSessionId")
            .collect(),
        persisted_hash: String::new(),
        saved: false,
    };
    s.persisted_hash = s.hash();
    let expiration = s.cookie.original_expires.or(s.cookie.expires);
    if expiration.is_some_and(|e| e <= now_ms()) {
        // destroy → regenerate (새 세션을 바로 저장한다 - TS 와 같다)
        let c = db::conn();
        db::run(
            &c,
            "DELETE FROM sessions WHERE sid = ?",
            crate::params![s.sid.clone()],
        )?;
        drop(c);
        let fresh = new_session(cfg, None);
        fresh.store_set()?;
        return Ok(fresh);
    }
    Ok(s)
}

/// 로그아웃: 저장소에서 지우고 request.session = null
pub fn destroy(s: &Session) -> db::Result<()> {
    let c = db::conn();
    db::run(
        &c,
        "DELETE FROM sessions WHERE sid = ?",
        crate::params![s.sid.clone()],
    )?;
    Ok(())
}

/// onSend: 저장 여부 판단 + Set-Cookie 헤더 (없으면 빈 Vec)
pub fn finish(
    session: Option<&mut Session>,
    cookie: Option<&str>,
    https: bool,
) -> db::Result<Vec<String>> {
    let Some(s) = session else { return Ok(vec![]) };
    let mut out = vec![];
    let cookie = cookie.filter(|c| !c.is_empty());
    let should_save = if cookie != Some(s.encrypted.as_str()) {
        s.is_modified()
    } else {
        true
    };
    let insecure = s.cookie.secure == Value::Bool(true) && !https;
    if !should_save || insecure {
        if cookie.is_some_and(|c| c != s.encrypted) {
            out.push(clear_cookie());
        }
        if s.saved {
            out.push(s.cookie.set_cookie(&s.encrypted));
        }
        return Ok(out);
    }
    s.save()?;
    out.push(s.cookie.set_cookie(&s.encrypted));
    Ok(out)
}

/// SqliteSessionStore 생성자: 표 만들기 + 만료 세션 정리
pub fn init_store() -> db::Result<()> {
    let c = db::conn();
    db::exec(
        &c,
        "CREATE TABLE IF NOT EXISTS sessions (
        sid     TEXT PRIMARY KEY,
        sess    TEXT NOT NULL,
        expires INTEGER NOT NULL
      )",
    )?;
    db::run(
        &c,
        "DELETE FROM sessions WHERE expires < ?",
        &[P::Num(now_ms())],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_matches_fastify_cookie() {
        // fastify/cookie signer 와 같은 HMAC-SHA256 base64(패딩 제거)
        let s = sign(
            "dwMnltcfzTM1SQ99b1MlJM_IshnPI1m9",
            "secret-secret-secret-secret-secret-1234",
        );
        let v = unsign(&s, "secret-secret-secret-secret-secret-1234");
        assert_eq!(v.as_deref(), Some("dwMnltcfzTM1SQ99b1MlJM_IshnPI1m9"));
        assert!(unsign(&s, "other-secret-other-secret-other-secret").is_none());
        assert!(!s.ends_with('='));
    }

    #[test]
    fn cookie_string_like_ts() {
        let c = Cookie::new(
            &cookie_opts(&Config {
                secret: String::new(),
                secure: false,
            }),
            1802584391273.0 - MAX_AGE_MS,
        );
        assert_eq!(
            c.set_cookie("a.b"),
            "sessionId=a.b; Path=/; Expires=Sun, 14 Feb 2027 05:53:11 GMT; HttpOnly; SameSite=Lax"
        );
        assert_eq!(
            js::stringify(&c.to_json()),
            r#"{"expires":"2027-02-14T05:53:11.273Z","originalMaxAge":7776000000,"sameSite":"lax","secure":false,"path":"/","httpOnly":true,"domain":null}"#
        );
    }

    #[test]
    fn parse_cookie_header() {
        assert_eq!(
            cookie_value(Some("a=1; sessionId=x.y+z; sessionId=2")).as_deref(),
            Some("x.y+z")
        );
        assert_eq!(
            cookie_value(Some("sessionId=x%2By")).as_deref(),
            Some("x+y")
        );
        assert_eq!(cookie_value(Some("a=1")), None);
    }
}
