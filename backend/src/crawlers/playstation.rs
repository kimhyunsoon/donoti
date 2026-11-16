// 플레이스테이션 스토어(store.playstation.com) 가격·할인 감시 - 상품 링크 붙여넣기 방식
// Next.js SSR 페이지의 cta 배타랑(__NEXT_DATA__ → batarangs.cta.text) 안 apollo 캐시에
// Product → webctas → price(basePriceValue=정가, discountedValue=판매가, endTime=할인 종료 epoch ms)가 있다
// 일반가/PS Plus 전용가 CTA는 serviceBranding(NONE/PS_PLUS)으로 구분한다
// (전용가 CTA가 목록 앞에 오고 isTiedToSubscription도 false라 branding만 신뢰할 것)
// concept 링크는 대표(기본) 에디션을, product 링크는 해당 에디션을 추적한다
// mode 'plusMonthly'는 링크 없이 PS Plus 월간 무료 게임(에센셜) 목록을 감시한다
use std::collections::HashSet;
use std::sync::LazyLock;

use futures_util::future::join_all;
use regex::Regex;
use serde_json::{Value, json};

use super::http::{FetchReq, fetch};
use super::nintendo::{CHROME_UA, discount_pct, link_url};
use super::retry::{
    clear_crawl_failure, report_crawl_failure, save_state, set_state_key, with_retry,
};
use super::{WatchRow, log, set_url_if_null};
use crate::catalog::notification_title;
use crate::core::db::P;
use crate::core::js;
use crate::jsx::{self, JS_S, prop};
use crate::notify;

const STORE_HOST: &str = "store.playstation.com";
// PS Plus 월간 무료 게임 목록 페이지 (스토어 카테고리 API는 폐기되어 빈 응답 - 이 페이지가 유일한 공개 소스)
const PLUS_MONTHLY_URL: &str = "https://www.playstation.com/ko-kr/ps-plus/whats-new/";
const UA: [(&str, &str); 2] = [
    ("User-Agent", CHROME_UA),
    ("Accept-Language", "ko-KR,ko;q=0.9"),
];
const TIMEOUT: u64 = 15_000;

pub struct FetchedProduct {
    pub name: String,
    pub price: Option<Value>,
    pub regular_price: Option<Value>,
    /// PS Plus 전용가 (일반 판매가와 다를 때만, 없으면 null)
    pub plus_price: Value,
    /// 할인 종료 시각 (epoch ms, 없으면 null)
    pub discount_ends_at: Option<f64>,
}

// 이름 뒤의 지원 언어 나열 괄호 제거 ('마블 울버린 (중국어(간체자), 한국어, ...)')
static CLEAN_NAME: LazyLock<fancy_regex::Regex> = LazyLock::new(|| {
    fancy_regex::Regex::new(&format!(
        r"{JS_S}*\((?=[^)]*(?:한국어|영어|일본어|중국어))[^)]*(?:\([^)]*\)[^)]*)*\){JS_S}*$"
    ))
    .unwrap()
});

pub fn clean_name(name: &str) -> String {
    let replaced = match CLEAN_NAME.find(name) {
        Ok(Some(m)) => format!("{}{}", &name[..m.start()], &name[m.end()..]),
        _ => name.to_string(),
    };
    js::trim(&replaced).to_string()
}

static NEXT_DATA: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"<script id="__NEXT_DATA__" type="application/json"[^>]*>([\s\S]*?)</script>"#)
        .unwrap()
});
static CTA_JSON: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"<script[^>]*type="application/json">([\s\S]*?)</script>"#).unwrap()
});

/// p.serviceBranding?.includes(x) (배열·문자열)
fn branding_includes(p: &Value, x: &str) -> Result<bool, String> {
    Ok(match jsx::opt(Some(p), "serviceBranding") {
        None | Some(Value::Null) => false,
        Some(Value::Array(a)) => a.iter().any(|v| v.as_str() == Some(x)),
        Some(Value::String(s)) => s.contains(x),
        Some(_) => return Err("p.serviceBranding?.includes is not a function".into()),
    })
}

/// cache[key] (cache 가 undefined 면 TypeError)
fn cache_get<'a>(cache: Option<&'a Value>, key: &str) -> Result<Option<&'a Value>, String> {
    prop(cache, key)
}

// 상품 페이지에서 이름·판매가·정가·할인 종료 시각 파싱
pub fn parse_product(html: &str) -> Result<FetchedProduct, String> {
    let next = NEXT_DATA
        .captures(html)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str())
        .filter(|s| !s.is_empty());
    let Some(next) = next else {
        return Err("playstation_parse_next".into());
    };
    let next_v = jsx::json_parse(next)?;
    let page_props = prop(prop(Some(&next_v), "props")?, "pageProps")?;
    let text = jsx::opt(
        jsx::opt(jsx::nn(prop(page_props, "batarangs")?), "cta"),
        "text",
    );
    let cta_json = match jsx::nn(text) {
        None => None,
        Some(Value::String(t)) => CTA_JSON
            .captures(t)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str().to_string()),
        Some(_) => return Err("pageProps.batarangs?.cta?.text?.match is not a function".into()),
    };
    let Some(cta_json) = cta_json.filter(|s| !s.is_empty()) else {
        return Err("playstation_parse_cta".into());
    };
    let cta_v = jsx::json_parse(&cta_json)?;
    let cache = prop(Some(&cta_v), "cache")?;

    // concept 페이지는 대표 상품, product 페이지는 해당 상품
    let concept_id = prop(page_props, "conceptId")?;
    let product_key: Option<Value> = if js::truthy(concept_id) {
        let concept = cache_get(cache, &format!("Concept:{}", jsx::tpl(concept_id)))?;
        jsx::opt(
            jsx::nn(jsx::opt(jsx::nn(concept), "defaultProduct")),
            "__ref",
        )
        .cloned()
    } else {
        Some(json!(format!(
            "Product:{}",
            jsx::tpl(prop(page_props, "productId")?)
        )))
    };
    let product = match &product_key {
        Some(k) if js::truthy(Some(k)) => cache_get(cache, &jsx::tpl(Some(k)))?,
        _ => None,
    };
    let name = jsx::opt(jsx::nn(product), "name");
    if !js::truthy(name) {
        return Err("playstation_parse_product".into());
    }

    // 일반가는 serviceBranding NONE, PS Plus 전용가는 PS_PLUS (전용가 CTA가 먼저 오므로 순서로 고르면 안 됨)
    let webctas = match jsx::nn(prop(product, "webctas")?) {
        None => vec![],
        Some(Value::Array(a)) => a.clone(),
        Some(_) => return Err("(product.webctas ?? []).map is not a function".into()),
    };
    let mut prices: Vec<&Value> = vec![];
    for c in &webctas {
        let r = jsx::tpl(prop(Some(c), "__ref")?);
        let p = jsx::opt(jsx::nn(cache_get(cache, &r)?), "price");
        if let Some(p) = jsx::nn(p)
            && matches!(jsx::opt(Some(p), "basePriceValue"), Some(Value::Number(_)))
        {
            prices.push(p);
        }
    }
    let mut price_idx = None;
    for (i, p) in prices.iter().enumerate() {
        if branding_includes(p, "NONE")? {
            price_idx = Some(i);
            break;
        }
    }
    if price_idx.is_none() {
        for (i, p) in prices.iter().enumerate() {
            if !branding_includes(p, "PS_PLUS")? {
                price_idx = Some(i);
                break;
            }
        }
    }
    if price_idx.is_none() && !prices.is_empty() {
        price_idx = Some(0);
    }
    let Some(pi) = price_idx else {
        return Err("playstation_parse_price".into());
    };
    let price = prices[pi];
    let mut plus = None;
    for (i, p) in prices.iter().enumerate() {
        if i != pi && branding_includes(p, "PS_PLUS")? {
            plus = Some(*p);
            break;
        }
    }
    let name = match name {
        Some(Value::String(s)) => clean_name(s),
        _ => return Err("name.replace is not a function".into()),
    };
    let discounted = jsx::opt(Some(price), "discountedValue");
    let plus_price = match plus {
        Some(p) if jsx::strict_ne(jsx::opt(Some(p), "discountedValue"), discounted) => {
            jsx::opt(Some(p), "discountedValue")
                .cloned()
                .unwrap_or(Value::Null)
        }
        _ => Value::Null,
    };
    let end_time = jsx::opt(Some(price), "endTime");
    Ok(FetchedProduct {
        name,
        price: discounted.cloned(),
        regular_price: jsx::opt(Some(price), "basePriceValue").cloned(),
        plus_price,
        discount_ends_at: if js::truthy(end_time) {
            Some(js::to_number(end_time))
        } else {
            None
        },
    })
}

async fn fetch_product(link: &str) -> Result<FetchedProduct, String> {
    with_retry(&format!("PS 스토어 {link}"), |_| async move {
        let res = fetch(FetchReq::get(link, &UA, TIMEOUT)).await?;
        if !res.ok() {
            return Err(format!("playstation_http_{}", res.status));
        }
        parse_product(&res.text())
    })
    .await
}

static PATH_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"/(concept|product)/([A-Za-z0-9_-]+)").unwrap());

/// 플레이스테이션 스토어 상품 링크를 해석해 상품명·판매가·정가를 돌려준다. 실패 시 한국어 메시지로 Err
pub async fn resolve_playstation_link(input: &str) -> Result<Value, String> {
    // 공유 텍스트에 상품명 등이 섞여 있어도 URL만 뽑아 쓴다
    let extracted = jsx::extract_url(input);
    let Ok(url) = url::Url::parse(&extracted) else {
        return Err("올바른 링크가 아니에요".into());
    };
    // 다른 국가 링크도 한국 스토어로 정규화 (가격은 ko-kr 기준)
    let path = PATH_RE.captures(url.path());
    let (Some(path), true) = (path, url.host_str() == Some(STORE_HOST)) else {
        return Err(
            "플레이스테이션 스토어(store.playstation.com) 게임 링크만 등록할 수 있어요".into(),
        );
    };
    let (kind, id) = (&path[1], &path[2]);
    let link = format!("https://{STORE_HOST}/ko-kr/{kind}/{id}");
    match fetch_product(&link).await {
        Ok(p) => Ok(jsx::obj(vec![
            ("productId", Some(json!(format!("{kind}/{id}")))),
            ("name", Some(json!(p.name))),
            ("price", p.price),
            ("regularPrice", p.regular_price),
            ("plusPrice", Some(p.plus_price)),
            ("link", Some(json!(link))),
        ])),
        Err(_) => Err("상품 정보를 불러오지 못했어요. 게임 페이지 링크인지 확인해 주세요".into()),
    }
}

// 할인 종료 시각 표기 (KST '1월 12일 23:59까지')
pub fn format_ends_at(epoch_ms: f64) -> String {
    use chrono::{Datelike, Timelike};
    let t = epoch_ms.trunc() + 9.0 * 3_600_000.0;
    let dt = (t.is_finite() && t.abs() <= 8.64e15)
        .then(|| chrono::DateTime::from_timestamp_millis(t as i64))
        .flatten();
    let Some(k) = dt else {
        return "NaN월 NaN일 NaN:NaN까지".into();
    };
    format!(
        "{}월 {}일 {:02}:{:02}까지",
        k.month(),
        k.day(),
        k.hour(),
        k.minute()
    )
}

// watch 하나 크롤링. '가격 변동 시에만 알림'(기본)은 직전 알림과 일반가·전용가가 다를 때만 발송 (1회차는 무조건)
async fn run_watch(watch: &mut WatchRow, config: &Value) -> Result<(), String> {
    let link = prop(Some(config), "link")?.cloned();
    if !js::truthy(link.as_ref()) {
        log(&format!(
            "PS 스토어 알림 설정 없음: watch {} \"{}\" - 건너뜀",
            watch.id, watch.name
        ));
        return Ok(());
    }

    let product = fetch_product(&jsx::tpl(link.as_ref())).await?;
    let mut state = jsx::json_parse(watch.state.as_deref().unwrap_or("{}"))?;
    let last_price = jsx::as_num(prop(Some(&state), "lastPrice")?).map(|f| json!(f));
    let last_plus = jsx::as_num(prop(Some(&state), "lastPlusPrice")?)
        .map(js::num)
        .unwrap_or(Value::Null);
    let last_price = last_price.map(|v| js::num(v.as_f64().unwrap()));
    let changed = last_price.is_none()
        || jsx::strict_ne(last_price.as_ref(), product.price.as_ref())
        || jsx::strict_ne(Some(&last_plus), Some(&product.plus_price));

    if changed {
        // 다음 회차 비교 기준을 갱신 (retry.ts가 같은 state를 쓰므로 in-memory 행도 함께 갱신)
        match &product.price {
            Some(p) => set_state_key(&mut state, "lastPrice", p.clone())?,
            None => {
                set_state_key(&mut state, "lastPrice", Value::Null)?;
                if let Value::Object(m) = &mut state {
                    m.shift_remove("lastPrice");
                }
            }
        }
        set_state_key(&mut state, "lastPlusPrice", product.plus_price.clone())?;
        save_state(watch, &state)?;
    }
    if prop(Some(config), "onPriceChange")? != Some(&Value::Bool(false)) && !changed {
        log(&format!(
            "PS 스토어 가격 변동 없음: \"{}\" {}원 - 알림 생략",
            watch.name,
            jsx::locale_of(product.price.as_ref())
        ));
        return Ok(());
    }

    let price = product.price.as_ref();
    let regular = product.regular_price.as_ref();
    let discounted = jsx::js_lt(price, regular);
    let mut lines = vec![if discounted {
        format!(
            "{} · {}% 할인",
            jsx::won(price)?,
            discount_pct(js::to_number(price), js::to_number(regular))
        )
    } else {
        jsx::won(price)?
    }];
    if discounted {
        lines.push(
            match product
                .discount_ends_at
                .filter(|t| *t != 0.0 && !t.is_nan())
            {
                Some(t) => format!("정가 {} · {}", jsx::won(regular)?, format_ends_at(t)),
                None => format!("정가 {}", jsx::won(regular)?),
            },
        );
    }
    if !product.plus_price.is_null() {
        lines.push(format!(
            "PS Plus 전용가 {}",
            jsx::won(Some(&product.plus_price))?
        ));
    }
    if let Some(l) = &last_price
        && jsx::strict_ne(Some(l), price)
    {
        lines.push(format!(
            "직전 알림 {} → {}",
            jsx::won(Some(l))?,
            jsx::won(price)?
        ));
    }
    let body = lines.join("\n");

    let url = link_url(watch, link.as_ref())?;
    notify::enqueue_raw(
        P::from("playstation"),
        P::from(notification_title("playstation", &watch.name)),
        P::from(body.as_str()),
        url,
        P::from(watch.id),
    )
    .map_err(|e| e.0)?;
    log(&format!(
        "PS 스토어 가격 확인: \"{}\" {}",
        watch.name,
        body.replace('\n', " / ")
    ));
    Ok(())
}

// ── PS Plus 월간 무료 게임 ────────────────────────────────────

pub struct MonthlyGame {
    pub slug: String,
    pub title: String,
}

// whats-new 페이지의 id="monthly-games" 섹션에서 이달의 무료 게임 목록 파싱.
// 카드 마크업이 게임 카탈로그 섹션과 동일해, 앵커 뒤 첫 게임 링크부터
// 링크 간격이 확 벌어지는 지점(다음 섹션)까지를 월간 게임 클러스터로 본다
const MONTHLY_CLUSTER_GAP: usize = 8_000;

static GAME_LINK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"href="/ko-kr/games/([a-z0-9-]+)/""#).unwrap());
static CARD_TITLE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"<h[23][^>]*>{JS_S}*([^<]+?){JS_S}*</h[23]>")).unwrap());

/// 위치·길이는 JS 처럼 UTF-16 단위
pub fn parse_monthly(html: &str) -> Result<Vec<MonthlyGame>, String> {
    let h16: Vec<u16> = html.encode_utf16().collect();
    let needle: Vec<u16> = "id=\"monthly-games\"".encode_utf16().collect();
    let Some(anchor) = h16
        .windows(needle.len())
        .position(|w| w == needle.as_slice())
    else {
        return Err("playstation_parse_monthly".into());
    };
    let seg16 = &h16[anchor..(anchor + 60_000).min(h16.len())];
    let seg = String::from_utf16_lossy(seg16);

    let mut games: Vec<MonthlyGame> = vec![];
    let mut seen: HashSet<String> = HashSet::new();
    let mut prev_at: Option<usize> = None;
    for m in GAME_LINK.captures_iter(&seg) {
        let idx = jsx::u16_index(&seg, m.get(0).unwrap().start());
        if let Some(p) = prev_at
            && idx - p > MONTHLY_CLUSTER_GAP
        {
            break;
        }
        prev_at = Some(idx);
        let slug = m[1].to_string();
        if !seen.insert(slug.clone()) {
            continue;
        }
        // 카드 제목은 링크 바로 앞 h2/h3
        let ctx = String::from_utf16_lossy(&seg16[idx.saturating_sub(800)..idx]);
        let title = CARD_TITLE
            .captures_iter(&ctx)
            .last()
            .map(|c| c[1].to_string())
            .unwrap_or_else(|| slug.replace('-', " "));
        games.push(MonthlyGame { slug, title });
    }
    if games.is_empty() {
        return Err("playstation_parse_monthly".into());
    }
    Ok(games)
}

async fn fetch_monthly_games() -> Result<Vec<MonthlyGame>, String> {
    with_retry("PS Plus 무료 게임", |_| async {
        let res = fetch(FetchReq::get(PLUS_MONTHLY_URL, &UA, TIMEOUT)).await?;
        if !res.ok() {
            return Err(format!("playstation_http_{}", res.status));
        }
        parse_monthly(&res.text())
    })
    .await
}

// 무료 게임 목록 감시. '목록 변동 시에만 알림'(기본)은 직전 알림과 다를 때만 발송 (1회차는 무조건)
async fn run_monthly_watch(watch: &mut WatchRow, config: &Value) -> Result<(), String> {
    let games = fetch_monthly_games().await?;
    let mut slugs: Vec<&str> = games.iter().map(|g| g.slug.as_str()).collect();
    slugs.sort_by(|a, b| jsx::js_str_cmp(a, b));
    let key = slugs.join(",");

    let mut state = jsx::json_parse(watch.state.as_deref().unwrap_or("{}"))?;
    let changed = prop(Some(&state), "lastGames")? != Some(&Value::String(key.clone()));
    if changed {
        set_state_key(&mut state, "lastGames", Value::String(key))?;
        save_state(watch, &state)?;
    }
    if prop(Some(config), "onListChange")? != Some(&Value::Bool(false)) && !changed {
        log(&format!(
            "PS Plus 무료 게임 변동 없음: {}종 - 알림 생략",
            games.len()
        ));
        return Ok(());
    }

    let body = std::iter::once(format!("이달의 무료 게임 {}종", games.len()))
        .chain(games.iter().map(|g| g.title.clone()))
        .collect::<Vec<_>>()
        .join("\n");
    if watch.url_empty() {
        set_url_if_null(watch.id, Some(PLUS_MONTHLY_URL))?;
    }
    notify::enqueue(
        "playstation",
        &notification_title("playstation", &watch.name),
        &body,
        watch.url_or(Some(PLUS_MONTHLY_URL)).as_deref(),
        watch.id,
    )
    .map_err(|e| e.0)?;
    log(&format!(
        "PS Plus 무료 게임 확인: {}",
        body.replace('\n', " / ")
    ));
    Ok(())
}

pub async fn run(mut watches: Vec<WatchRow>) -> Result<(), String> {
    join_all(watches.iter_mut().map(|watch| async move {
        let r = async {
            let config = jsx::json_parse(&watch.config)?;
            let mode = prop(Some(&config), "mode")?;
            if mode == Some(&json!("plusMonthly")) {
                run_monthly_watch(watch, &config).await?;
            } else {
                run_watch(watch, &config).await?;
            }
            clear_crawl_failure(watch)
        }
        .await;
        if let Err(message) = r {
            log(&format!(
                "PS 스토어 크롤링 실패 (watch {}): {message}",
                watch.id
            ));
            let _ = report_crawl_failure(watch, &format!("PS 스토어 수집 실패: {message}"));
        }
    }))
    .await;
    Ok(())
}

pub fn validate_config(config: Option<&Value>) -> Result<Option<String>, String> {
    // 무료 게임 목록 감시는 링크가 필요 없다
    if prop(config, "mode")? == Some(&json!("plusMonthly")) {
        return Ok(None);
    }
    if !matches!(prop(config, "productId")?, Some(Value::String(s)) if !s.is_empty()) {
        return Ok(Some("플레이스테이션 스토어 링크를 불러와 주세요".into()));
    }
    let prefix = format!("https://{STORE_HOST}/");
    if !matches!(prop(config, "link")?, Some(Value::String(s)) if s.starts_with(&prefix)) {
        return Ok(Some("플레이스테이션 스토어 링크를 불러와 주세요".into()));
    }
    Ok(None)
}
