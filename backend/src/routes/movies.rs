// 영화 3사 등록 폼용 목록 프록시 (상영작·지점)
use serde_json::json;

use crate::core::app::{HResult, Pre, Reply, Req, Routes, require_auth};
use crate::crawlers::{cgv, lotte, megabox};

fn bad_gateway(error: &str, message: &str) -> Reply {
    Reply::json(502, json!({ "error": error, "message": message }))
}

async fn cgv_movies(_req: Req) -> HResult {
    Ok(match cgv::fetch_cgv_movies().await {
        Ok(v) => Reply::ok(v),
        Err(_) => bad_gateway("cgv_error", "CGV 상영작을 불러오지 못했어요"),
    })
}

async fn cgv_theaters(_req: Req) -> HResult {
    Ok(match cgv::fetch_cgv_theaters().await {
        Ok(v) => Reply::ok(v),
        Err(_) => bad_gateway("cgv_error", "CGV 지점을 불러오지 못했어요"),
    })
}

async fn lotte_movies(_req: Req) -> HResult {
    Ok(match lotte::fetch_lotte_movies().await {
        Ok(v) => Reply::ok(v),
        Err(_) => bad_gateway("lotte_error", "롯데시네마 상영작을 불러오지 못했어요"),
    })
}

async fn lotte_theaters(_req: Req) -> HResult {
    Ok(match lotte::fetch_lotte_theaters().await {
        Ok(v) => Reply::ok(v),
        Err(_) => bad_gateway("lotte_error", "롯데시네마 지점을 불러오지 못했어요"),
    })
}

async fn megabox_movies(_req: Req) -> HResult {
    Ok(match megabox::fetch_megabox_movies().await {
        Ok(v) => Reply::ok(v),
        Err(_) => bad_gateway("megabox_error", "메가박스 상영작을 불러오지 못했어요"),
    })
}

async fn megabox_theaters(_req: Req) -> HResult {
    Ok(match megabox::fetch_megabox_theaters().await {
        Ok(v) => Reply::ok(v),
        Err(_) => bad_gateway("megabox_error", "메가박스 지점을 불러오지 못했어요"),
    })
}

pub fn register(r: &mut Routes) {
    let a: Option<Pre> = Some(require_auth);
    r.add("GET", "/api/cgv", "/movies", a, cgv_movies);
    r.add("GET", "/api/cgv", "/theaters", a, cgv_theaters);
    r.add("GET", "/api/lotte", "/movies", a, lotte_movies);
    r.add("GET", "/api/lotte", "/theaters", a, lotte_theaters);
    r.add("GET", "/api/megabox", "/movies", a, megabox_movies);
    r.add("GET", "/api/megabox", "/theaters", a, megabox_theaters);
}
