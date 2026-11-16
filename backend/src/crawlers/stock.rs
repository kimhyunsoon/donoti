// 주식 크롤러 - 네이버 증권 시세 API (국내 m.stock / 해외 api.stock)
// 종목은 stock_symbols 마스터로 관리, watch.config = { code }
use futures_util::future::join_all;
use serde_json::Value;

use super::fx::{Quote, ratio_text};
use super::http::{FetchReq, fetch};
use super::retry::{clear_crawl_failure, report_crawl_failure, with_retry};
use super::{WatchRow, log, set_url_if_null};
use crate::catalog::notification_title;
use crate::core::db::{self, P};
use crate::core::js;
use crate::jsx;
use crate::notify;

#[derive(Clone)]
struct StockSymbol {
    code: String,
    name: String,
    kind: String,
    market: String,
}

// 시세 API 주소 (market·kind에 따라 도메인·경로가 다르다)
fn quote_url(s: &StockSymbol) -> String {
    let host = if s.market == "domestic" {
        "https://m.stock.naver.com/api"
    } else {
        "https://api.stock.naver.com"
    };
    let k = if s.kind == "index" { "index" } else { "stock" };
    format!("{host}/{k}/{}/basic", s.code)
}

// 알림 클릭 시 이동할 네이버 증권 상세 (모바일)
fn detail_url(s: &StockSymbol) -> String {
    let section = if s.market == "domestic" {
        "domestic"
    } else {
        "worldstock"
    };
    let k = if s.kind == "index" { "index" } else { "stock" };
    format!("https://m.stock.naver.com/{section}/{k}/{}/total", s.code)
}

// 시세 표기: 국내 종목·ETF '278,500원' / 해외 종목·ETF '$311.30' / 지수 '6,890.01'
fn format_price(s: &StockSymbol, price: &str) -> String {
    if s.kind == "index" {
        return price.to_string();
    }
    if s.market == "domestic" {
        format!("{price}원")
    } else {
        format!("${price}")
    }
}

async fn fetch_quote(s: &StockSymbol) -> Result<Quote, String> {
    with_retry(&format!("주식 {}", s.code), |_| async move {
        let res = fetch(FetchReq::get(
            quote_url(s),
            &[("User-Agent", "Mozilla/5.0")],
            10_000,
        ))
        .await?;
        if !res.ok() {
            return Err(format!("naver_http_{} ({})", res.status, s.code));
        }
        let data = res.json()?;
        let close = jsx::prop(Some(&data), "closePrice")?;
        if !js::truthy(close) {
            return Err(format!("naver_bad_response ({})", s.code));
        }
        let ratio = jsx::nn(jsx::opt(Some(&data), "fluctuationsRatio"));
        Ok(Quote {
            price: jsx::tpl(close),
            ratio: ratio.map(|r| js::to_number(Some(r))).unwrap_or(0.0),
        })
    })
    .await
}

fn find_symbol(watch: &WatchRow) -> Result<Option<StockSymbol>, String> {
    let config = jsx::json_parse(&watch.config)?;
    let code = jsx::nn(jsx::prop(Some(&config), "code")?)
        .cloned()
        .unwrap_or(Value::String(String::new()));
    let c = db::conn();
    let row = db::get(
        &c,
        "SELECT * FROM stock_symbols WHERE code = ?",
        &[db::from_json(Some(&code)).map_err(|e| e.0)?],
    )
    .map_err(|e| e.0)?;
    Ok(row.map(|r| StockSymbol {
        code: js::to_js_string(r.get("code")),
        name: js::to_js_string(r.get("name")),
        kind: js::to_js_string(r.get("kind")),
        market: js::to_js_string(r.get("market")),
    }))
}

// 같은 회차의 주식 알림들을 묶어 종목별 1회만 조회한다
pub async fn run(watches: Vec<WatchRow>) -> Result<(), String> {
    // 종목 코드 → watch 목록 (Map 삽입 순서)
    let mut by_code: Vec<(StockSymbol, Vec<WatchRow>)> = vec![];
    for watch in watches {
        let Some(symbol) = find_symbol(&watch)? else {
            log(&format!(
                "주식 알림 종목 설정 없음: watch {} \"{}\" - 건너뜀",
                watch.id, watch.name
            ));
            continue;
        };
        match by_code.iter_mut().find(|(s, _)| s.code == symbol.code) {
            Some((_, g)) => g.push(watch),
            None => by_code.push((symbol, vec![watch])),
        }
    }

    join_all(by_code.into_iter().map(|(symbol, mut group)| async move {
        let quote = match fetch_quote(&symbol).await {
            Ok(q) => q,
            Err(message) => {
                log(&format!("주식 조회 실패 ({}): {message}", symbol.code));
                for watch in group.iter_mut() {
                    report_crawl_failure(
                        watch,
                        &format!("주식 조회 실패 ({}): {message}", symbol.name),
                    )?;
                }
                return Ok::<(), String>(());
            }
        };
        for watch in group.iter_mut() {
            clear_crawl_failure(watch)?;
        }
        // 묶음 알림은 본문 첫 줄만 쓰므로 전일 대비를 같은 줄에 붙인다
        let body = format!(
            "{} ({}%)",
            format_price(&symbol, &quote.price),
            ratio_text(quote.ratio)
        );
        let url = detail_url(&symbol);
        for watch in &group {
            // 리다이렉트 URL 자동 설정 (사용자가 직접 넣지 않으므로 비어있을 때 채움)
            if watch.url_empty() {
                set_url_if_null(watch.id, Some(&url))?;
            }
            notify::enqueue(
                "stock",
                &notification_title("stock", &watch.name),
                &body,
                watch.url_or(Some(&url)).as_deref(),
                watch.id,
            )
            .map_err(|e| e.0)?;
        }
        log(&format!(
            "주식 조회 완료: {}({}) {} (알림 {}건)",
            symbol.name,
            symbol.code,
            quote.price,
            group.len()
        ));
        Ok(())
    }))
    .await;
    Ok(())
}

pub fn validate_config(
    config: Option<&Value>,
    exclude_id: Option<f64>,
) -> Result<Option<String>, String> {
    let code = jsx::prop(config, "code")?;
    let Some(Value::String(code)) = code.filter(|c| c.as_str() != Some("")) else {
        return Ok(Some("종목을 선택해 주세요".into()));
    };
    let c = db::conn();
    let Some(symbol) = db::get(
        &c,
        "SELECT name FROM stock_symbols WHERE code = ?",
        &[P::from(code.as_str())],
    )
    .map_err(|e| e.0)?
    else {
        return Ok(Some("알 수 없는 종목이에요".into()));
    };
    // 1종목 1알림 - 활성 목록에 같은 종목이 이미 있으면 거부
    let dup = db::get(
        &c,
        "SELECT id FROM watches
         WHERE provider = 'stock' AND deleted_at IS NULL AND id != ?
           AND json_extract(config, '$.code') = ?",
        &[P::Num(exclude_id.unwrap_or(-1.0)), P::from(code.as_str())],
    )
    .map_err(|e| e.0)?;
    if dup.is_some() {
        return Ok(Some(format!(
            "{} 알림이 이미 있어요",
            js::to_js_string(symbol.get("name"))
        )));
    }
    Ok(None)
}
