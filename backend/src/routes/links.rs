// 공유 링크 해석 (알림 등록 폼에서 상품·지점·갤러리 확인용) - 이마트·닌텐도·PS·디시
use serde_json::{Value, json};

use crate::core::app::{HResult, Pre, Reply, Req, Routes, require_auth};
use crate::crawlers::{dcinside, emart, nintendo, playstation};
use crate::jsx;

fn invalid(message: String) -> Reply {
    Reply::json(400, json!({ "error": "invalid_link", "message": message }))
}

/// try { return await resolveX(req.body.url ?? '') } catch (err) { 400 invalid_link }
/// (req.body.url 읽기도 try 안이라 본문이 없으면 TypeError 문구로 400)
async fn resolve<F, Fut>(req: &Req, f: F) -> HResult
where
    F: FnOnce(String) -> Fut,
    Fut: std::future::Future<Output = Result<Value, String>>,
{
    let input = match jsx::resolve_input(req.body().as_ref()) {
        Ok(i) => i,
        Err(m) => return Ok(invalid(m)),
    };
    match f(input).await {
        Ok(v) => Ok(Reply::ok(v)),
        Err(m) => Ok(invalid(m)),
    }
}

async fn emart_resolve(req: Req) -> HResult {
    resolve(&req, |i| async move { emart::resolve_emart_link(&i).await }).await
}

async fn nintendo_resolve(req: Req) -> HResult {
    resolve(&req, |i| async move {
        nintendo::resolve_nintendo_link(&i).await
    })
    .await
}

async fn playstation_resolve(req: Req) -> HResult {
    resolve(&req, |i| async move {
        playstation::resolve_playstation_link(&i).await
    })
    .await
}

async fn dcinside_resolve(req: Req) -> HResult {
    resolve(&req, |i| async move {
        dcinside::resolve_dcinside_link(&i).await
    })
    .await
}

pub fn register(r: &mut Routes) {
    let a: Option<Pre> = Some(require_auth);
    r.add("POST", "/api/emart", "/resolve", a, emart_resolve);
    r.add("POST", "/api/nintendo", "/resolve", a, nintendo_resolve);
    r.add(
        "POST",
        "/api/playstation",
        "/resolve",
        a,
        playstation_resolve,
    );
    r.add("POST", "/api/dcinside", "/resolve", a, dcinside_resolve);
}
