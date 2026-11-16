// 4개 앱 공통 사본 - 고칠 때 네 곳을 같이 고친다.
//
// Fastify 5 처리 순서를 따른다:
//   onRequest(쿠키 → 세션 복원) → 라우트 찾기(없으면 404) → 본문 해석(JSON/텍스트/multipart, 1MiB)
//   → preHandler(requireAuth 등) → 핸들러 → onSend(세션 저장·Set-Cookie)
// 오류 본문은 Fastify 기본 모양 그대로({statusCode, code?, error, message}).
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use axum::body::Body as AxBody;
use axum::http::{HeaderMap, HeaderValue, Method, Request, Response, StatusCode, header};
use bytes::Bytes;
use serde_json::{Map, Value, json};

use super::db::DbError;
use super::js;
use super::session::{self, Session};

pub const BODY_LIMIT: usize = 1024 * 1024; // Fastify 기본 bodyLimit

// ───────── 요청 ─────────

pub enum Body {
    /// 본문 없음 (TS 의 req.body === undefined)
    None,
    Json(Value),
    Text(String),
    /// multipart/form-data 원문 (핸들러가 core::multipart 로 해석)
    Multipart {
        content_type: String,
        bytes: Bytes,
    },
}

pub struct Req {
    pub method: Method,
    /// 쿼리 포함 원래 URL (404 문구용)
    pub url: String,
    pub path: String,
    pub params: HashMap<String, String>,
    pub query: Map<String, Value>,
    pub headers: HeaderMap,
    pub body: Body,
    pub https: bool,
    /// request.session (로그아웃하면 None)
    pub session: Arc<Mutex<Option<Session>>>,
}

impl Req {
    pub fn param(&self, k: &str) -> &str {
        self.params.get(k).map(|s| s.as_str()).unwrap_or("")
    }

    /// req.query.k (같은 키가 여러 번이면 배열)
    pub fn query(&self, k: &str) -> Option<&Value> {
        self.query.get(k)
    }

    /// req.query.k 가 문자열일 때만
    pub fn query_str(&self, k: &str) -> Option<&str> {
        self.query.get(k).and_then(|v| v.as_str())
    }

    pub fn header(&self, k: &str) -> Option<&str> {
        self.headers.get(k).and_then(|v| v.to_str().ok())
    }

    /// req.body (JSON 일 때만 값, 텍스트면 문자열)
    pub fn body(&self) -> Option<Value> {
        match &self.body {
            Body::Json(v) => Some(v.clone()),
            Body::Text(s) => Some(Value::String(s.clone())),
            _ => None,
        }
    }

    /// `req.body.k` - body 가 undefined/null 이면 TS 처럼 TypeError(500)
    pub fn field(&self, k: &str) -> Result<Option<Value>, AppError> {
        match self.body() {
            None => Err(AppError::type_error(format!(
                "Cannot read properties of undefined (reading '{k}')"
            ))),
            Some(Value::Null) => Err(AppError::type_error(format!(
                "Cannot read properties of null (reading '{k}')"
            ))),
            Some(Value::Object(m)) => Ok(m.get(k).cloned()),
            Some(_) => Ok(None),
        }
    }

    /// `req.body?.k` (body 가 없어도 오류 없이 undefined)
    pub fn field_opt(&self, k: &str) -> Option<Value> {
        match self.body() {
            Some(Value::Object(m)) => m.get(k).cloned(),
            _ => None,
        }
    }

    /// reply.callNotFound()
    pub fn not_found(&self) -> Reply {
        not_found(&self.method, &self.url)
    }

    pub fn user_id(&self) -> Option<i64> {
        self.session
            .lock()
            .unwrap()
            .as_ref()
            .and_then(|s| s.user_id())
    }

    pub fn with_session<T>(&self, f: impl FnOnce(&mut Option<Session>) -> T) -> T {
        f(&mut self.session.lock().unwrap())
    }
}

// ───────── 응답 ─────────

pub struct Reply {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub content_type: Option<String>,
    pub body: Bytes,
}

impl Reply {
    /// reply.send(object) - JSON.stringify, application/json; charset=utf-8
    pub fn json(status: u16, v: Value) -> Reply {
        Reply {
            status,
            headers: vec![],
            content_type: Some("application/json; charset=utf-8".into()),
            body: Bytes::from(js::stringify(&v)),
        }
    }

    /// 핸들러가 객체를 return (200)
    pub fn ok(v: Value) -> Reply {
        Reply::json(200, v)
    }

    /// reply.code(n).send() - 본문 없음
    pub fn empty(status: u16) -> Reply {
        Reply {
            status,
            headers: vec![],
            content_type: None,
            body: Bytes::new(),
        }
    }

    /// reply.type(t).send(buffer)
    pub fn bytes(status: u16, content_type: &str, body: impl Into<Bytes>) -> Reply {
        Reply {
            status,
            headers: vec![],
            content_type: Some(content_type.into()),
            body: body.into(),
        }
    }

    pub fn header(mut self, k: &str, v: &str) -> Reply {
        self.headers.push((k.to_string(), v.to_string()));
        self
    }

    /// { error: '...' } 모양
    pub fn error(status: u16, code: &str) -> Reply {
        Reply::json(status, json!({ "error": code }))
    }
}

/// 던져진 오류 → Fastify 기본 오류 처리기 (500 은 message 포함)
#[derive(Debug)]
pub struct AppError {
    pub status: u16,
    pub code: Option<String>,
    pub message: String,
}

impl AppError {
    pub fn internal(message: impl Into<String>) -> AppError {
        AppError {
            status: 500,
            code: None,
            message: message.into(),
        }
    }
    pub fn type_error(message: impl Into<String>) -> AppError {
        AppError::internal(message)
    }
    fn fst(status: u16, code: &str, message: &str) -> AppError {
        AppError {
            status,
            code: Some(code.into()),
            message: message.into(),
        }
    }
    pub fn reply(&self) -> Reply {
        let error = StatusCode::from_u16(self.status)
            .ok()
            .and_then(|s| s.canonical_reason())
            .unwrap_or("Error");
        let mut m = Map::new();
        m.insert("statusCode".into(), json!(self.status));
        if let Some(c) = &self.code {
            m.insert("code".into(), json!(c));
        }
        m.insert("error".into(), json!(error));
        m.insert("message".into(), json!(self.message));
        Reply::json(self.status, Value::Object(m))
    }
}

impl From<DbError> for AppError {
    fn from(e: DbError) -> Self {
        AppError {
            status: 500,
            code: e.1.map(String::from),
            message: e.0,
        }
    }
}

pub type HResult = Result<Reply, AppError>;

// ───────── 라우터 ─────────

type Fut = Pin<Box<dyn Future<Output = HResult> + Send>>;
type HandlerFn = Arc<dyn Fn(Req) -> Fut + Send + Sync>;
/// preHandler: Some(reply) 를 돌려주면 거기서 끝 (requireAuth 의 401)
pub type Pre = fn(&Req) -> Option<Reply>;

#[derive(Clone)]
struct Route {
    handler: HandlerFn,
    pre: Option<Pre>,
}

pub struct Routes {
    by_method: HashMap<Method, matchit::Router<Route>>,
    multipart: bool,
}

/// Fastify 경로(`/x/:id`, `/x/*`) → matchit (`/x/{id}`, `/x/{*wildcard}`)
fn to_matchit(path: &str) -> String {
    path.split('/')
        .map(|seg| {
            if let Some(name) = seg.strip_prefix(':') {
                format!("{{{name}}}")
            } else if seg == "*" {
                "{*wildcard}".to_string()
            } else {
                seg.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}

impl Default for Routes {
    fn default() -> Self {
        Self::new()
    }
}

impl Routes {
    pub fn new() -> Routes {
        Routes {
            by_method: HashMap::new(),
            multipart: false,
        }
    }

    /// @fastify/multipart 등록 (multipart/form-data 본문을 받는다)
    pub fn enable_multipart(&mut self) {
        self.multipart = true;
    }

    /// 플러그인 prefix + 경로. 경로가 '/' 이면 Fastify 처럼 prefix 와 prefix + '/' 둘 다
    pub fn add<F, R>(&mut self, method: &str, prefix: &str, path: &str, pre: Option<Pre>, f: F)
    where
        F: Fn(Req) -> R + Send + Sync + 'static,
        R: Future<Output = HResult> + Send + 'static,
    {
        let handler: HandlerFn = Arc::new(move |req| Box::pin(f(req)));
        let route = Route { handler, pre };
        let full: Vec<String> = if path == "/" && !prefix.is_empty() {
            vec![prefix.to_string(), format!("{prefix}/")]
        } else {
            vec![format!("{prefix}{path}")]
        };
        let m = Method::from_bytes(method.as_bytes()).unwrap();
        for p in full {
            let mp = to_matchit(&p);
            self.by_method
                .entry(m.clone())
                .or_default()
                .insert(mp.clone(), route.clone())
                .expect("route conflict");
            if m == Method::GET {
                // exposeHeadRoutes
                self.by_method
                    .entry(Method::HEAD)
                    .or_default()
                    .insert(mp, route.clone())
                    .expect("route conflict");
            }
        }
    }
}

// ───────── 처리 ─────────

pub struct App {
    pub routes: Routes,
    pub session: session::Config,
}

fn parse_query(q: &str) -> Map<String, Value> {
    // fast-querystring: '&' 로 나누고 첫 '=' 기준, '+' → 공백, 같은 키는 배열
    let mut m = Map::new();
    for part in q.split('&') {
        if part.is_empty() {
            continue;
        }
        let (k, v) = match part.find('=') {
            Some(i) => (&part[..i], &part[i + 1..]),
            None => (part, ""),
        };
        let dec = |s: &str| {
            let s = s.replace('+', " ");
            percent_encoding::percent_decode_str(&s)
                .decode_utf8()
                .map(|c| c.into_owned())
                .unwrap_or(s)
        };
        let (k, v) = (dec(k), Value::String(dec(v)));
        match m.get_mut(&k) {
            Some(Value::Array(a)) => a.push(v),
            Some(prev) => {
                let p = prev.take();
                *prev = Value::Array(vec![p, v]);
            }
            None => {
                m.insert(k, v);
            }
        }
    }
    m
}

/// secure-json-parse: __proto__, constructor.prototype 금지
fn has_forbidden_key(v: &Value) -> bool {
    match v {
        Value::Object(m) => {
            m.contains_key("__proto__")
                || matches!(m.get("constructor"), Some(Value::Object(c)) if c.contains_key("prototype"))
                || m.values().any(has_forbidden_key)
        }
        Value::Array(a) => a.iter().any(has_forbidden_key),
        _ => false,
    }
}

fn media_type(ct: &str) -> String {
    ct.split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase()
}

async fn read_body(
    app: &App,
    method: &Method,
    headers: &HeaderMap,
    body: AxBody,
) -> Result<Body, AppError> {
    if *method == Method::GET || *method == Method::HEAD {
        return Ok(Body::None);
    }
    let ct = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let has_body = headers.contains_key(header::TRANSFER_ENCODING)
        || headers
            .get(header::CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|l| l.trim() != "0");
    let Some(ct) = ct.filter(|c| !c.is_empty()) else {
        if has_body {
            return Err(AppError::fst(
                415,
                "FST_ERR_CTP_INVALID_MEDIA_TYPE",
                "Unsupported Media Type",
            ));
        }
        return Ok(Body::None);
    };
    let mt = media_type(&ct);
    let too_large = || {
        AppError::fst(
            413,
            "FST_ERR_CTP_BODY_TOO_LARGE",
            "Request body is too large",
        )
    };
    match mt.as_str() {
        "application/json" | "text/plain" => {
            if let Some(len) = headers
                .get(header::CONTENT_LENGTH)
                .and_then(|v| v.to_str().ok())
                .and_then(|l| l.parse::<usize>().ok())
                && len > BODY_LIMIT
            {
                return Err(too_large());
            }
            let bytes = axum::body::to_bytes(body, BODY_LIMIT)
                .await
                .map_err(|_| too_large())?;
            let text = String::from_utf8_lossy(&bytes).into_owned();
            if mt == "text/plain" {
                return Ok(Body::Text(text));
            }
            if text.is_empty() {
                return Err(AppError::fst(
                    400,
                    "FST_ERR_CTP_EMPTY_JSON_BODY",
                    "Body cannot be empty when content-type is set to 'application/json'",
                ));
            }
            let invalid = || {
                AppError::fst(
                    400,
                    "FST_ERR_CTP_INVALID_JSON_BODY",
                    "Body is not valid JSON but content-type is set to 'application/json'",
                )
            };
            let v: Value =
                serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(|_| invalid())?;
            if has_forbidden_key(&v) {
                return Err(invalid());
            }
            Ok(Body::Json(v))
        }
        "multipart/form-data" if app.routes.multipart => {
            let bytes = axum::body::to_bytes(body, 64 * 1024 * 1024)
                .await
                .map_err(|_| too_large())?;
            Ok(Body::Multipart {
                content_type: ct,
                bytes,
            })
        }
        _ => {
            if has_body {
                Err(AppError::fst(
                    415,
                    "FST_ERR_CTP_INVALID_MEDIA_TYPE",
                    "Unsupported Media Type",
                ))
            } else {
                Ok(Body::None)
            }
        }
    }
}

/// Fastify 404 (라우트 없음)
pub fn not_found(method: &Method, url: &str) -> Reply {
    Reply::json(
        404,
        json!({ "message": format!("Route {method}:{url} not found"), "error": "Not Found", "statusCode": 404 }),
    )
}

pub async fn handle(app: Arc<App>, req: Request<AxBody>) -> Response<AxBody> {
    let started = Instant::now();
    let (parts, body) = req.into_parts();
    let method = parts.method.clone();
    let url = parts
        .uri
        .path_and_query()
        .map(|p| p.as_str().to_string())
        .unwrap_or_else(|| "/".into());
    let path = parts.uri.path().to_string();
    let query = parse_query(parts.uri.query().unwrap_or(""));
    // trustProxy: X-Forwarded-Proto 첫 값
    let https = parts
        .headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .map(|p| p.split(',').next().unwrap_or("").trim() == "https")
        .unwrap_or(false);
    let cookie_header: Vec<&str> = parts
        .headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .collect();
    let joined = cookie_header.join("; ");
    let cookie = session::cookie_value(if cookie_header.is_empty() {
        None
    } else {
        Some(&joined)
    });

    let session = match session::load(&app.session, cookie.as_deref()) {
        Ok(s) => Some(s),
        Err(e) => {
            return finish_response(&method, AppError::from(e).reply(), vec![], started, &url);
        }
    };
    let session = Arc::new(Mutex::new(session));

    let reply = dispatch(
        &app,
        method.clone(),
        url.clone(),
        path,
        query,
        parts.headers,
        body,
        https,
        session.clone(),
    )
    .await;

    // onSend: 세션 저장
    let mut guard = session.lock().unwrap();
    let cookies = match session::finish(guard.as_mut(), cookie.as_deref(), https) {
        Ok(c) => c,
        Err(e) => {
            drop(guard);
            return finish_response(&method, AppError::from(e).reply(), vec![], started, &url);
        }
    };
    drop(guard);
    finish_response(&method, reply, cookies, started, &url)
}

#[allow(clippy::too_many_arguments)]
async fn dispatch(
    app: &App,
    method: Method,
    url: String,
    path: String,
    query: Map<String, Value>,
    headers: HeaderMap,
    body: AxBody,
    https: bool,
    session: Arc<Mutex<Option<Session>>>,
) -> Reply {
    let found = app.routes.by_method.get(&method).and_then(|r| {
        r.at(&path).ok().map(|m| {
            let params = m
                .params
                .iter()
                .map(|(k, v)| {
                    let d = percent_encoding::percent_decode_str(v)
                        .decode_utf8()
                        .map(|c| c.into_owned())
                        .unwrap_or_else(|_| v.to_string());
                    (k.to_string(), d)
                })
                .collect::<HashMap<_, _>>();
            (m.value.clone(), params)
        })
    });
    let Some((route, params)) = found else {
        return not_found(&method, &url);
    };
    let body = match read_body(app, &method, &headers, body).await {
        Ok(b) => b,
        Err(e) => return e.reply(),
    };
    let req = Req {
        method,
        url,
        path,
        params,
        query,
        headers,
        body,
        https,
        session,
    };
    if let Some(pre) = route.pre
        && let Some(r) = pre(&req)
    {
        return r;
    }
    // 핸들러 panic 도 500 으로 (Fastify 는 프로세스가 죽지 않는다)
    let fut = (route.handler)(req);
    match tokio::spawn(fut).await {
        Ok(Ok(r)) => r,
        Ok(Err(e)) => {
            if e.status >= 500 {
                tracing::error!("{}", e.message);
            }
            e.reply()
        }
        Err(join) => {
            tracing::error!("handler panic: {join}");
            AppError::internal("Internal Server Error").reply()
        }
    }
}

fn finish_response(
    method: &Method,
    reply: Reply,
    cookies: Vec<String>,
    started: Instant,
    url: &str,
) -> Response<AxBody> {
    let mut b = Response::builder().status(reply.status);
    let h = b.headers_mut().unwrap();
    if let Some(ct) = &reply.content_type {
        h.insert(header::CONTENT_TYPE, HeaderValue::from_str(ct).unwrap());
    }
    for (k, v) in &reply.headers {
        if let (Ok(k), Ok(v)) = (
            header::HeaderName::from_bytes(k.as_bytes()),
            HeaderValue::from_str(v),
        ) {
            h.append(k, v);
        }
    }
    for c in cookies {
        h.append(header::SET_COOKIE, HeaderValue::from_str(&c).unwrap());
    }
    h.insert(header::CONTENT_LENGTH, HeaderValue::from(reply.body.len()));
    tracing::info!(
        "{} {} {} {}ms",
        method,
        url,
        reply.status,
        started.elapsed().as_millis()
    );
    let body = if *method == Method::HEAD {
        AxBody::empty()
    } else {
        AxBody::from(reply.body)
    };
    b.body(body).unwrap()
}

/// 서버 시작 (SIGTERM/SIGINT 정상 종료)
pub async fn serve(app: App, host: &str, port: u16) -> std::io::Result<()> {
    let app = Arc::new(app);
    let router = axum::Router::new().fallback(move |req: Request<AxBody>| {
        let app = app.clone();
        async move { handle(app, req).await }
    });
    let listener = tokio::net::TcpListener::bind((host, port)).await?;
    tracing::info!("Server listening at http://{host}:{port}");
    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown_signal())
        .await
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    let term = async {
        if let Ok(mut s) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            s.recv().await;
        }
    };
    tokio::select! { _ = ctrl_c => {}, _ = term => {} }
}

/// requireAuth: 세션에 userId 가 없으면 401 { error: 'unauthorized' }
pub fn require_auth(req: &Req) -> Option<Reply> {
    if req.user_id().is_none() {
        Some(Reply::error(401, "unauthorized"))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_like_fastify() {
        let q = parse_query("a=1&b=x+y&a=2&c=%E2%9C%93&d");
        assert_eq!(
            js::stringify(&Value::Object(q)),
            r#"{"a":["1","2"],"b":"x y","c":"✓","d":""}"#
        );
    }

    #[test]
    fn matchit_paths() {
        assert_eq!(
            to_matchit("/api/plants/:id/archive"),
            "/api/plants/{id}/archive"
        );
        assert_eq!(to_matchit("/api/files/*"), "/api/files/{*wildcard}");
    }
}
