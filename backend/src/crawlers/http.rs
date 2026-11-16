// 크롤러용 fetch - Node 22 fetch(undici) 와 같은 옵션을 크롤러별로 그대로 넘긴다.
// - redirect: 기본 follow(최대 20회), emart 단축링크만 manual
// - timeout: AbortSignal.timeout(ms) 처럼 응답 본문까지 포함한 전체 시간
// - 오류 문구: 네트워크 오류 'fetch failed', 시간 초과 'The operation was aborted due to timeout'
// - res.text(): 항상 UTF-8 (BOM 제거), res.json() = JSON.parse(text) (V8 오류 문구)
// 시험(fixture 비교)에서는 set_hook 으로 실제 요청 대신 저장된 응답을 돌려준다.
use std::sync::OnceLock;
use std::time::Duration;

use serde_json::Value;

use crate::jsx;

#[derive(Clone, Debug)]
pub enum ReqBody {
    Text(String),
    /// FormData (텍스트 필드만)
    Form(Vec<(String, String)>),
}

#[derive(Clone, Debug)]
pub struct FetchReq {
    pub method: &'static str,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<ReqBody>,
    pub timeout_ms: u64,
    pub redirect_manual: bool,
}

impl FetchReq {
    pub fn get(url: impl Into<String>, headers: &[(&str, &str)], timeout_ms: u64) -> FetchReq {
        FetchReq {
            method: "GET",
            url: url.into(),
            headers: headers
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            body: None,
            timeout_ms,
            redirect_manual: false,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct FetchRes {
    pub status: u16,
    /// 소문자 이름, 받은 순서 (set-cookie 는 여러 개)
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl FetchRes {
    pub fn ok(&self) -> bool {
        (200..=299).contains(&self.status)
    }
    /// Headers.get - 같은 이름이 여러 개면 ', ' 로 합친다
    pub fn header(&self, name: &str) -> Option<String> {
        let v: Vec<&str> = self
            .headers
            .iter()
            .filter(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
            .collect();
        if v.is_empty() {
            None
        } else {
            Some(v.join(", "))
        }
    }
    /// Headers.getSetCookie()
    pub fn set_cookies(&self) -> Vec<String> {
        self.headers
            .iter()
            .filter(|(k, _)| k.eq_ignore_ascii_case("set-cookie"))
            .map(|(_, v)| v.clone())
            .collect()
    }
    pub fn text(&self) -> String {
        let b = self
            .body
            .strip_prefix(b"\xEF\xBB\xBF")
            .unwrap_or(&self.body);
        String::from_utf8_lossy(b).into_owned()
    }
    pub fn json(&self) -> Result<Value, String> {
        jsx::json_parse(&self.text())
    }
}

type Hook = Box<dyn Fn(&FetchReq) -> Result<FetchRes, String> + Send + Sync>;
static HOOK: OnceLock<Hook> = OnceLock::new();

/// 시험용: 모든 fetch 를 이 함수로 돌린다 (프로세스당 한 번)
pub fn set_hook(f: impl Fn(&FetchReq) -> Result<FetchRes, String> + Send + Sync + 'static) {
    let _ = HOOK.set(Box::new(f));
}

fn client(manual: bool) -> &'static reqwest::Client {
    static FOLLOW: OnceLock<reqwest::Client> = OnceLock::new();
    static MANUAL: OnceLock<reqwest::Client> = OnceLock::new();
    let (cell, policy) = if manual {
        (&MANUAL, reqwest::redirect::Policy::none())
    } else {
        (&FOLLOW, reqwest::redirect::Policy::limited(20))
    };
    cell.get_or_init(|| {
        reqwest::Client::builder()
            .redirect(policy)
            .build()
            .expect("http client")
    })
}

pub const TIMEOUT_MESSAGE: &str = "The operation was aborted due to timeout";

/// fetch(url, init) + 본문 읽기
pub async fn fetch(req: FetchReq) -> Result<FetchRes, String> {
    if let Some(h) = HOOK.get() {
        return h(&req);
    }
    let timeout = Duration::from_millis(req.timeout_ms);
    let work = async {
        let method = reqwest::Method::from_bytes(req.method.as_bytes()).unwrap();
        let mut b = client(req.redirect_manual).request(method, &req.url);
        for (k, v) in &req.headers {
            b = b.header(k, v);
        }
        match &req.body {
            None => {}
            Some(ReqBody::Text(t)) => {
                // undici 는 문자열 본문에 content-type 이 없으면 text/plain;charset=UTF-8 을 붙인다
                if !req
                    .headers
                    .iter()
                    .any(|(k, _)| k.eq_ignore_ascii_case("content-type"))
                {
                    b = b.header("content-type", "text/plain;charset=UTF-8");
                }
                b = b.body(t.clone());
            }
            Some(ReqBody::Form(fields)) => {
                let mut form = reqwest::multipart::Form::new();
                for (k, v) in fields {
                    form = form.text(k.clone(), v.clone());
                }
                b = b.multipart(form);
            }
        }
        let res = b.send().await.map_err(|_| "fetch failed".to_string())?;
        let status = res.status().as_u16();
        let headers = res
            .headers()
            .iter()
            .map(|(k, v)| {
                (
                    k.as_str().to_string(),
                    String::from_utf8_lossy(v.as_bytes()).into_owned(),
                )
            })
            .collect();
        // !res.ok 면 TS 는 본문을 읽지 않고 throw 하지만, 읽어도 결과는 같다
        let body = res
            .bytes()
            .await
            .map_err(|_| "terminated".to_string())?
            .to_vec();
        Ok(FetchRes {
            status,
            headers,
            body,
        })
    };
    match tokio::time::timeout(timeout, work).await {
        Ok(r) => r,
        Err(_) => Err(TIMEOUT_MESSAGE.into()),
    }
}
