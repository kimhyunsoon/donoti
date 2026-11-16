// 닌텐도 스토어(store.nintendo.co.kr) 가격·할인 감시 - 상품 링크 붙여넣기 방식
// 웹 스토어는 Magento 기반: 상품 페이지 HTML의 price_info JSON에 판매가(final)·정가(regular)가 있고
// eShop 세일도 여기에 special price로 반영된다 (할인 시 final < regular)
use std::sync::LazyLock;

use futures_util::future::join_all;
use regex::Regex;
use serde_json::{Value, json};

use super::http::{FetchReq, fetch};
use super::retry::{
    clear_crawl_failure, report_crawl_failure, save_state, set_state_key, with_retry,
};
use super::{WatchRow, log};
use crate::catalog::notification_title;
use crate::core::db::{self, P};
use crate::core::js;
use crate::jsx::{self, prop};
use crate::notify;

const STORE_HOST: &str = "store.nintendo.co.kr";
pub const CHROME_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0 Safari/537.36";
const TIMEOUT: u64 = 15_000;

static PRICE_INFO: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#""price_info":\{([\s\S]*?)"formatted_prices""#).unwrap());
static FINAL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#""final_price":([0-9.]+)"#).unwrap());
static REGULAR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#""regular_price":([0-9.]+)"#).unwrap());
pub static TITLE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<title>([^<]*)</title>").unwrap());

pub struct Product {
    pub name: String,
    pub price: f64,
    pub regular_price: f64,
}

fn cap1<'a>(re: &Regex, s: &'a str) -> Option<&'a str> {
    re.captures(s).and_then(|c| c.get(1)).map(|m| m.as_str())
}

// 상품 페이지에서 이름·판매가·정가 파싱
pub fn parse_product(html: &str) -> Result<Product, String> {
    // 첫 price_info가 메인 상품의 priceBox 설정 (formatted_prices 전까지가 숫자 필드 구간)
    let info = cap1(&PRICE_INFO, html).unwrap_or("");
    let fin = cap1(&FINAL, info).filter(|s| !s.is_empty());
    let regular = cap1(&REGULAR, info).filter(|s| !s.is_empty());
    let title = cap1(&TITLE, html).map(js::trim).filter(|s| !s.is_empty());
    let (Some(fin), Some(regular), Some(title)) = (fin, regular, title) else {
        return Err("nintendo_parse".into());
    };
    Ok(Product {
        name: title.to_string(),
        price: jsx::math_round(js::string_to_number(fin)),
        regular_price: jsx::math_round(js::string_to_number(regular)),
    })
}

async fn fetch_product(link: &str) -> Result<Product, String> {
    with_retry(&format!("닌텐도 상품 {link}"), |_| async move {
        let res = fetch(FetchReq::get(link, &[("User-Agent", CHROME_UA)], TIMEOUT)).await?;
        if !res.ok() {
            return Err(format!("nintendo_http_{}", res.status));
        }
        parse_product(&res.text())
    })
    .await
}

/// 닌텐도 스토어 상품 링크를 해석해 상품명·판매가·정가를 돌려준다. 실패 시 한국어 메시지로 Err
pub async fn resolve_nintendo_link(input: &str) -> Result<Value, String> {
    // 공유 텍스트에 상품명 등이 섞여 있어도 URL만 뽑아 쓴다
    let extracted = jsx::extract_url(input);
    let Ok(url) = url::Url::parse(&extracted) else {
        return Err("올바른 링크가 아니에요".into());
    };
    if url.host_str() != Some(STORE_HOST) {
        return Err("닌텐도 스토어(store.nintendo.co.kr) 상품 링크만 등록할 수 있어요".into());
    }
    let link = format!("https://{STORE_HOST}{}", url.path());
    match fetch_product(&link).await {
        Ok(p) => Ok(json!({
            "productId": url.path().trim_start_matches('/'),
            "name": p.name,
            "price": js::num(p.price),
            "regularPrice": js::num(p.regular_price),
            "link": link,
        })),
        Err(_) => Err("상품 정보를 불러오지 못했어요. 상품 페이지 링크인지 확인해 주세요".into()),
    }
}

/// 직전 알림 가격: typeof state.lastPrice === 'number' ? state.lastPrice : null
pub fn last_number(state: &Value, k: &str) -> Result<Option<f64>, String> {
    Ok(jsx::as_num(prop(Some(state), k)?))
}

pub fn won_n(n: f64) -> String {
    format!("{}원", jsx::to_locale_ko(n))
}

/// 할인율: Math.round((1 - price / regular) * 100)
pub fn discount_pct(price: f64, regular: f64) -> String {
    jsx::num_str(jsx::math_round((1.0 - price / regular) * 100.0))
}

/// UPDATE watches SET url = ? (config.link) / watch.url ?? config.link
pub fn link_url(watch: &WatchRow, link: Option<&Value>) -> Result<P, String> {
    if watch.url_empty() {
        let c = db::conn();
        db::run(
            &c,
            "UPDATE watches SET url = ? WHERE id = ? AND url IS NULL",
            &[db::from_json(link).map_err(|e| e.0)?, P::from(watch.id)],
        )
        .map_err(|e| e.0)?;
    }
    match &watch.url {
        Some(u) => Ok(P::from(u.as_str())),
        None => db::from_json(link).map_err(|e| e.0),
    }
}

// watch 하나 크롤링. '가격 변동 시에만 알림'(기본)은 직전 알림 가격과 다를 때만 발송 (1회차는 무조건)
async fn run_watch(watch: &mut WatchRow) -> Result<(), String> {
    let config = jsx::json_parse(&watch.config)?;
    let link = prop(Some(&config), "link")?.cloned();
    if !js::truthy(link.as_ref()) {
        log(&format!(
            "닌텐도 알림 설정 없음: watch {} \"{}\" - 건너뜀",
            watch.id, watch.name
        ));
        return Ok(());
    }

    let product = fetch_product(&jsx::tpl(link.as_ref())).await?;
    let mut state = jsx::json_parse(watch.state.as_deref().unwrap_or("{}"))?;
    let last_price = last_number(&state, "lastPrice")?;
    let changed = last_price.is_none_or(|l| l != product.price);

    if changed {
        // 다음 회차 비교 기준을 갱신 (retry.ts가 같은 state를 쓰므로 in-memory 행도 함께 갱신)
        set_state_key(&mut state, "lastPrice", js::num(product.price))?;
        save_state(watch, &state)?;
    }
    if prop(Some(&config), "onPriceChange")? != Some(&Value::Bool(false)) && !changed {
        log(&format!(
            "닌텐도 가격 변동 없음: \"{}\" {}원 - 알림 생략",
            watch.name,
            jsx::to_locale_ko(product.price)
        ));
        return Ok(());
    }

    let discounted = product.price < product.regular_price;
    let mut lines = vec![if discounted {
        format!(
            "{} · {}% 할인",
            won_n(product.price),
            discount_pct(product.price, product.regular_price)
        )
    } else {
        won_n(product.price)
    }];
    if discounted {
        lines.push(format!("정가 {}", won_n(product.regular_price)));
    }
    if let Some(l) = last_price
        && l != product.price
    {
        lines.push(format!("직전 알림 {} → {}", won_n(l), won_n(product.price)));
    }
    let body = lines.join("\n");

    let url = link_url(watch, link.as_ref())?;
    notify::enqueue_raw(
        P::from("nintendo"),
        P::from(notification_title("nintendo", &watch.name)),
        P::from(body.as_str()),
        url,
        P::from(watch.id),
    )
    .map_err(|e| e.0)?;
    log(&format!(
        "닌텐도 가격 확인: \"{}\" {}",
        watch.name,
        body.replace('\n', " / ")
    ));
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
                "닌텐도 크롤링 실패 (watch {}): {message}",
                watch.id
            ));
            let _ = report_crawl_failure(watch, &format!("닌텐도 수집 실패: {message}"));
        }
    }))
    .await;
    Ok(())
}

pub fn validate_config(config: Option<&Value>) -> Result<Option<String>, String> {
    if !matches!(prop(config, "productId")?, Some(Value::String(s)) if !s.is_empty()) {
        return Ok(Some("닌텐도 스토어 링크를 불러와 주세요".into()));
    }
    let prefix = format!("https://{STORE_HOST}/");
    if !matches!(prop(config, "link")?, Some(Value::String(s)) if s.starts_with(&prefix)) {
        return Ok(Some("닌텐도 스토어 링크를 불러와 주세요".into()));
    }
    Ok(None)
}
