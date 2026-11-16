// 이마트 크롤러 - 이마트 앱 공유 링크 기반 재고·구매가능 감시
// 지원 유형: digital(디지털그랩 픽업) / product(일반 매장 상품)
// 오더픽·와인그랩 등 나머지는 등록 시 거부 (앱 전용이라 크롤링 불가)
use std::sync::LazyLock;

use futures_util::future::join_all;
use regex::Regex;
use serde_json::{Value, json};

use super::http::{FetchReq, ReqBody, fetch};
use super::retry::{clear_crawl_failure, report_crawl_failure, with_retry};
use super::{WatchRow, log};
use crate::catalog::notification_title;
use crate::core::db::{self, P};
use crate::core::js;
use crate::jsx::{self, prop};
use crate::notify;
use crate::params;

const API: &str = "https://api-eapp.emart.com";
const UA: (&str, &str) = ("User-Agent", "Mozilla/5.0");
const TIMEOUT: u64 = 10_000;

// 지역 코드 → 지역명 (digital-grab store 응답 기준)
fn area_name(code: Option<&Value>) -> Option<&'static str> {
    Some(match code?.as_str()? {
        "A" => "서울",
        "B" => "부산",
        "C" => "인천",
        "D" => "대구",
        "E" => "광주",
        "F" => "대전",
        "G" => "울산",
        "H" => "강원",
        "I" => "경기",
        "J" => "경남",
        "K" => "경북",
        "L" => "전남",
        "M" => "전북",
        "N" => "충남",
        "O" => "충북",
        "P" => "제주",
        "Q" => "세종",
        _ => return None,
    })
}

async fn get_json(label: &str, url: &str, post_body: Option<&str>) -> Result<Value, String> {
    // 최대 3회 재시도 (4xx는 미취급 등 정상 흐름이라 withRetry가 즉시 중단)
    with_retry(label, |_| async move {
        let res = fetch(FetchReq {
            method: if post_body.is_some() { "POST" } else { "GET" },
            url: url.to_string(),
            headers: vec![
                (UA.0.into(), UA.1.into()),
                ("Content-Type".into(), "application/json".into()),
            ],
            body: post_body.map(|b| ReqBody::Text(b.into())),
            timeout_ms: TIMEOUT,
            redirect_manual: false,
        })
        .await?;
        if !res.ok() {
            return Err(format!("emart_http_{}", res.status));
        }
        res.json()
    })
    .await
}

fn res_status_200(res: &Value) -> Result<bool, String> {
    Ok(prop(Some(res), "status")?.and_then(|v| v.as_f64()) == Some(200.0))
}

// ── 디지털그랩 (픽업) ─────────────────────────────────────────

struct DigitalItem {
    name: Option<Value>,
    price: Option<Value>,
    available: bool,
}

/// 지점별 픽업 상품 조회. 미취급·조회 불가 지점은 None
async fn fetch_digital_item(sku: &str, store_code: &str) -> Option<DigitalItem> {
    let r: Result<Option<DigitalItem>, String> = async {
        let res = get_json(
            &format!("이마트 픽업 {sku}@{store_code}"),
            &format!("{API}/panda/api/v1/digital-grab/item?skuCode={sku}&storeCode={store_code}"),
            None,
        )
        .await?;
        let data = prop(Some(&res), "data")?;
        if !res_status_200(&res)? || !js::truthy(data) {
            return Ok(None);
        }
        Ok(Some(DigitalItem {
            name: prop(data, "name")?.cloned(),
            price: prop(data, "salePrice")?.cloned(),
            // buyLimit 수치는 품절 지점에서도 양수일 수 있어 오탐을 낸다 (하남 사례) - itemStatus만 신뢰
            available: prop(data, "itemStatus")? == Some(&json!("N")),
        }))
    }
    .await;
    r.ok().flatten()
}

/// [...a, ...b] 의 스프레드 - 배열이 아니면 TypeError
fn spread(v: Option<&Value>, expr: &str) -> Result<Vec<Value>, String> {
    match jsx::nn(v) {
        None => Ok(vec![]),
        Some(Value::Array(a)) => Ok(a.clone()),
        Some(Value::String(s)) => Ok(s.chars().map(|c| json!(c.to_string())).collect()),
        Some(_) => Err(format!("{expr} is not iterable")),
    }
}

// 픽업 상품 취급 지점 목록 (이마트 + 트레이더스)
async fn fetch_digital_stores(sku: &str) -> Result<Vec<Value>, String> {
    let res = get_json(
        &format!("이마트 픽업 지점목록 {sku}"),
        &format!("{API}/panda/api/v1/digital-grab/store?skuCode={sku}"),
        None,
    )
    .await?;
    let data = prop(Some(&res), "data")?;
    let mut all = spread(prop(data, "emart")?, "res.data.emart")?;
    all.extend(spread(prop(data, "traders")?, "res.data.traders")?);
    all.iter()
        .map(|s| {
            Ok(jsx::obj(vec![
                ("code", prop(Some(s), "storeCode")?.cloned()),
                ("name", prop(Some(s), "storeName")?.cloned()),
                ("area", prop(Some(s), "areaName")?.cloned()),
            ]))
        })
        .collect()
}

// ── 일반 매장 상품 ────────────────────────────────────────────

struct ProductInfo {
    name: Option<Value>,
    price: Value,
}

async fn fetch_product_info(sku: &str, store_id: &str) -> Option<ProductInfo> {
    let r: Result<Option<ProductInfo>, String> = async {
        let res = get_json(
            &format!("이마트 상품 {sku}@{store_id}"),
            &format!("{API}/product/api/v1/info/1100/{sku}?storeId={store_id}"),
            Some("{}"),
        )
        .await?;
        let data = prop(Some(&res), "data")?;
        if !res_status_200(&res)? || !js::truthy(data) {
            return Ok(None);
        }
        Ok(Some(ProductInfo {
            name: prop(data, "name")?.cloned(),
            price: jsx::nn(jsx::opt(prop(data, "price")?, "sellPrice"))
                .cloned()
                .unwrap_or(json!(0)),
        }))
    }
    .await;
    r.ok().flatten()
}

/// `x.filter`/`x.map` 대상 배열 - 배열이 아니면 TypeError
fn arr<'a>(v: Option<&'a Value>, expr: &str, method: &str) -> Result<&'a Vec<Value>, String> {
    match v {
        Some(Value::Array(a)) => Ok(a),
        None => Err(format!(
            "Cannot read properties of undefined (reading '{method}')"
        )),
        Some(Value::Null) => Err(format!(
            "Cannot read properties of null (reading '{method}')"
        )),
        Some(_) => Err(format!("{expr}.{method} is not a function")),
    }
}

// 이마트 전 지점 목록
async fn fetch_product_branches() -> Result<Vec<Value>, String> {
    let res = get_json(
        "이마트 지점목록",
        &format!("{API}/search/api/v1/store/branch?storeType=E"),
        None,
    )
    .await?;
    let data = arr(prop(Some(&res), "data")?, "res.data", "filter")?;
    let mut out = vec![];
    for b in data {
        if prop(Some(b), "jijumStatus")? != Some(&json!("OPEN")) {
            continue;
        }
        out.push(jsx::obj(vec![
            ("code", prop(Some(b), "storeId")?.cloned()),
            ("name", prop(Some(b), "storeNm")?.cloned()),
            (
                "area",
                Some(json!(area_name(prop(Some(b), "areaCode")?).unwrap_or(""))),
            ),
        ]));
    }
    Ok(out)
}

// 지점별 재고 수량 (1콜로 전 지점) - Map 키는 SameValueZero, 나중 값이 이긴다
async fn fetch_stock_map(sku: &str) -> Result<Vec<(Value, Value)>, String> {
    let res = get_json(
        &format!("이마트 재고 {sku}"),
        &format!("{API}/search/api/v1/store/stock/branch?storeType=E&skuCode={sku}"),
        None,
    )
    .await?;
    let data = arr(prop(Some(&res), "data")?, "res.data", "map")?;
    let mut m: Vec<(Value, Value)> = vec![];
    for b in data {
        let k = prop(Some(b), "storeId")?.cloned().unwrap_or(Value::Null);
        let v = prop(Some(b), "stockQty")?.cloned();
        // undefined 키/값은 Null 로 근사 (Map 에 undefined 저장)
        let v = v.unwrap_or(Value::Null);
        match m.iter_mut().find(|(x, _)| jsx::same_value_zero(x, &k)) {
            Some(e) => e.1 = v,
            None => m.push((k, v)),
        }
    }
    Ok(m)
}

// ── 공유 링크 해석 (등록 폼용) ─────────────────────────────────

const UNSUPPORTED_MESSAGE: &str =
    "지원하지 않는 링크예요. 이마트 앱의 픽업(디지털그랩)·매장 상품 공유 링크만 등록할 수 있어요";

static DIGITS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[0-9]+$").unwrap());

/// URLSearchParams.get(k) ?? ''
pub fn search_param(url: &url::Url, k: &str) -> Option<String> {
    url.query_pairs()
        .find(|(n, _)| n == k)
        .map(|(_, v)| v.into_owned())
}

/// 이마트 공유 링크(emart.kr 단축 또는 eapp.emart.com 직접)를 해석해
/// 상품 정보와 선택 가능한 지점 목록을 돌려준다. 실패 시 한국어 메시지로 Err
pub async fn resolve_emart_link(input: &str) -> Result<Value, String> {
    // 공유 텍스트에 상품명 등이 섞여 있어도 URL만 뽑아 쓴다
    let extracted = jsx::extract_url(input);
    let Ok(mut url) = url::Url::parse(&extracted) else {
        return Err("올바른 링크가 아니에요".into());
    };

    // 단축링크는 302 Location의 appLink.do?link=... 에 실제 경로가 들어있다
    if url.host_str() == Some("emart.kr") {
        let res = fetch(FetchReq {
            redirect_manual: true,
            ..FetchReq::get(url.as_str(), &[UA], TIMEOUT)
        })
        .await?;
        let location = res.header("location").unwrap_or_default();
        let Some(idx) = location.find("link=") else {
            return Err(UNSUPPORTED_MESSAGE.into());
        };
        // link= 뒤는 인코딩 없이 ?·&가 그대로 섞여 있어 통째로 다시 파싱한다
        let base = url::Url::parse("https://eapp.emart.com").unwrap();
        url = base
            .join(&location[idx + 5..])
            .map_err(|_| "Invalid URL".to_string())?;
    }
    if !url.host_str().unwrap_or("").ends_with("emart.com") {
        return Err(UNSUPPORTED_MESSAGE.into());
    }

    let path = url.path().to_string();
    if path.starts_with("/webapp/digital/view/") {
        let sku = path.split('/').next_back().unwrap_or("").to_string();
        let store_code = search_param(&url, "storeCode").unwrap_or_default();
        if !DIGITS.is_match(&sku) || store_code.is_empty() {
            return Err(UNSUPPORTED_MESSAGE.into());
        }
        // Promise.all: 지점목록이 먼저 실패하면 상품 조회(재시도 포함)는 뒤에서 계속 돌고 바로 throw
        let item = tokio::spawn({
            let (sku, store_code) = (sku.clone(), store_code.clone());
            async move { fetch_digital_item(&sku, &store_code).await }
        });
        let stores = fetch_digital_stores(&sku).await?;
        let Some(item) = item.await.ok().flatten() else {
            return Err("상품 정보를 불러오지 못했어요. 잠시 후 다시 시도해 주세요".into());
        };
        return Ok(jsx::obj(vec![
            ("type", Some(json!("digital"))),
            ("sku", Some(json!(sku))),
            ("name", item.name),
            ("price", item.price),
            ("link", Some(json!(extracted))),
            ("linkedStoreCode", Some(json!(store_code))),
            ("stores", Some(Value::Array(stores))),
        ]));
    }

    if path.starts_with("/webapp/product/view") {
        let sku = search_param(&url, "sku_code").unwrap_or_default();
        let store_id = search_param(&url, "storeId").unwrap_or_default();
        if !DIGITS.is_match(&sku) || store_id.is_empty() {
            return Err(UNSUPPORTED_MESSAGE.into());
        }
        let info = tokio::spawn({
            let (sku, store_id) = (sku.clone(), store_id.clone());
            async move { fetch_product_info(&sku, &store_id).await }
        });
        let stores = fetch_product_branches().await?;
        let Some(info) = info.await.ok().flatten() else {
            return Err("상품 정보를 불러오지 못했어요. 잠시 후 다시 시도해 주세요".into());
        };
        return Ok(jsx::obj(vec![
            ("type", Some(json!("product"))),
            ("sku", Some(json!(sku))),
            ("name", info.name),
            ("price", Some(info.price)),
            ("link", Some(json!(extracted))),
            ("linkedStoreCode", Some(json!(store_id))),
            ("stores", Some(Value::Array(stores))),
        ]));
    }

    // 오더픽·와인그랩 등 나머지 전부 (앱 전용이라 크롤링 불가)
    Err(UNSUPPORTED_MESSAGE.into())
}

// ── 크롤러 ────────────────────────────────────────────────────

// '이마트 ' 접두는 중복이라 제거
fn short_store_name(name: Option<&Value>) -> Result<String, String> {
    match name {
        Some(Value::String(s)) => Ok(s.strip_prefix("이마트 ").unwrap_or(s).to_string()),
        None => Err("Cannot read properties of undefined (reading 'replace')".into()),
        Some(Value::Null) => Err("Cannot read properties of null (reading 'replace')".into()),
        Some(_) => Err("name.replace is not a function".into()),
    }
}

/// price > 0 (JS 비교: 숫자로 바꿔서)
fn positive(v: &Value) -> bool {
    js::to_number(Some(v)) > 0.0
}

// watch 하나 크롤링. 구매 가능 지점이 하나라도 있으면 매회 알림 (직전 상태와 무관)
async fn run_watch(watch: &WatchRow) -> Result<(), String> {
    let config = jsx::json_parse(&watch.config)?;
    if config.is_null() {
        return Err("Cannot destructure property 'type' of 'config' as it is null.".into());
    }
    let ty = prop(Some(&config), "type")?;
    let sku = prop(Some(&config), "sku")?;
    let stores = prop(Some(&config), "stores")?;
    let stores_len = match stores {
        Some(Value::Array(a)) => a.len(),
        Some(Value::String(s)) => js::utf16_len(s),
        _ => 0,
    };
    if !js::truthy(ty) || !js::truthy(sku) || stores_len == 0 {
        log(&format!(
            "이마트 알림 설정 없음: watch {} \"{}\" - 건너뜀",
            watch.id, watch.name
        ));
        return Ok(());
    }
    let stores = arr(stores, "stores", "map")?;
    let sku = jsx::tpl(sku);
    let ty_digital = ty == Some(&json!("digital"));

    // 재고 있을 때만 알림 (기본 on) - 끄면 품절 상태도 매회 알림
    let notify_out_of_stock = prop(Some(&config), "onlyInStock")? == Some(&json!(false));
    let mut price = json!(0);
    // 구매 가능 지점이 있었는지
    let mut in_stock = true;
    // 지점별 한 줄 ('구로점' / '은평점 · 2개')
    let mut store_lines: Vec<String> = vec![];
    if ty_digital {
        let codes: Vec<String> = stores
            .iter()
            .map(|s| Ok(jsx::tpl(prop(Some(s), "code")?)))
            .collect::<Result<_, String>>()?;
        let quotes = join_all(codes.iter().map(|c| fetch_digital_item(&sku, c))).await;
        let available: Vec<&Value> = stores
            .iter()
            .zip(&quotes)
            .filter(|(_, q)| q.as_ref().is_some_and(|q| q.available))
            .map(|(s, _)| s)
            .collect();
        if !available.is_empty() {
            price = quotes
                .iter()
                .flatten()
                .find(|q| q.available)
                .and_then(|q| jsx::nn(q.price.as_ref()).cloned())
                .unwrap_or(json!(0));
            store_lines = available
                .iter()
                .map(|s| short_store_name(prop(Some(s), "name")?))
                .collect::<Result<_, String>>()?;
        } else if notify_out_of_stock {
            in_stock = false;
            price = quotes
                .iter()
                .flatten()
                .next()
                .and_then(|q| jsx::nn(q.price.as_ref()).cloned())
                .unwrap_or(json!(0));
        } else {
            return Ok(());
        }
    } else {
        let stock_map = fetch_stock_map(&sku).await?;
        let mut available: Vec<(&Value, Value)> = vec![];
        for s in stores {
            let code = prop(Some(s), "code")?.cloned().unwrap_or(Value::Null);
            let qty = stock_map
                .iter()
                .find(|(k, _)| jsx::same_value_zero(k, &code))
                .map(|(_, v)| v.clone())
                .filter(|v| !v.is_null())
                .unwrap_or(json!(0));
            if positive(&qty) {
                available.push((s, qty));
            }
        }
        if !available.is_empty() {
            let first = jsx::tpl(prop(Some(available[0].0), "code")?);
            let info = fetch_product_info(&sku, &first).await;
            price = info.map(|i| i.price).unwrap_or(json!(0));
            store_lines = available
                .iter()
                .map(|(s, qty)| {
                    Ok(format!(
                        "{} · {}개",
                        short_store_name(prop(Some(s), "name")?)?,
                        jsx::tpl(Some(qty))
                    ))
                })
                .collect::<Result<_, String>>()?;
        } else if notify_out_of_stock {
            in_stock = false;
            let first = jsx::tpl(prop(stores.first(), "code")?);
            let info = fetch_product_info(&sku, &first).await;
            price = info.map(|i| i.price).unwrap_or(json!(0));
        } else {
            return Ok(());
        }
    }

    // 첫 줄 상태·가격, 이후 지점별 한 줄씩
    let price_text = if positive(&price) {
        format!(" · {}원", jsx::locale_of(Some(&price)))
    } else {
        String::new()
    };
    let head = if in_stock {
        format!("구매 가능{price_text}")
    } else {
        format!(
            "{}{price_text}",
            if ty_digital {
                "품절"
            } else {
                "재고 없음"
            }
        )
    };
    let mut shown: Vec<String> = store_lines.iter().take(6).cloned().collect();
    if store_lines.len() > 6 {
        shown.push(format!("외 {}곳", store_lines.len() - 6));
    }
    let mut body = std::iter::once(head)
        .chain(shown)
        .collect::<Vec<_>>()
        .join("\n");

    // 재고 확인되면 종료 옵션 - 알림 후 목록에서 제거 (종료된 알림에서 복구 가능)
    if js::truthy(prop(Some(&config), "endOnStock")?) && in_stock {
        let c = db::conn();
        db::run(
            &c,
            "UPDATE watches SET deleted_at = datetime('now'), updated_at = datetime('now')
       WHERE id = ? AND deleted_at IS NULL",
            params![watch.id],
        )
        .map_err(|e| e.0)?;
        drop(c);
        body.push_str("\n알림 종료");
        log(&format!(
            "재고 확인으로 알림 종료: watch {} \"{}\"",
            watch.id, watch.name
        ));
    }

    let link = prop(Some(&config), "link")?;
    if watch.url_empty() && js::truthy(link) {
        let c = db::conn();
        db::run(
            &c,
            "UPDATE watches SET url = ? WHERE id = ? AND url IS NULL",
            &[db::from_json(link).map_err(|e| e.0)?, P::from(watch.id)],
        )
        .map_err(|e| e.0)?;
    }
    let url = match &watch.url {
        Some(u) => P::from(u.as_str()),
        None => db::from_json(link).map_err(|e| e.0)?,
    };
    notify::enqueue_raw(
        P::from("emart"),
        P::from(notification_title("emart", &watch.name)),
        P::from(body.as_str()),
        url,
        P::from(watch.id),
    )
    .map_err(|e| e.0)?;
    log(&format!("이마트 재고 확인: \"{}\" {body}", watch.name));
    Ok(())
}

pub async fn run(mut watches: Vec<WatchRow>) -> Result<(), String> {
    join_all(watches.iter_mut().map(|watch| async move {
        let r = async {
            run_watch(watch).await?;
            clear_crawl_failure(watch)
        }
        .await;
        if let Err(message) = r {
            log(&format!(
                "이마트 크롤링 실패 (watch {}): {message}",
                watch.id
            ));
            let _ = report_crawl_failure(watch, &format!("이마트 수집 실패: {message}"));
        }
    }))
    .await;
    Ok(())
}

pub fn validate_config(config: Option<&Value>) -> Result<Option<String>, String> {
    let ty = prop(config, "type")?;
    if ty != Some(&json!("digital")) && ty != Some(&json!("product")) {
        return Ok(Some("이마트 공유 링크를 불러와 주세요".into()));
    }
    if !matches!(prop(config, "sku")?, Some(Value::String(s)) if !s.is_empty()) {
        return Ok(Some("이마트 공유 링크를 불러와 주세요".into()));
    }
    if !matches!(prop(config, "stores")?, Some(Value::Array(a)) if !a.is_empty()) {
        return Ok(Some("지점을 1곳 이상 선택해 주세요".into()));
    }
    Ok(None)
}
