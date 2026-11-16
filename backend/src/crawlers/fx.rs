// 환율 크롤러 - 네이버 금융 front-api (하나은행 고시 환율, 장중 수시 갱신)
use futures_util::future::join_all;
use serde_json::Value;

use super::http::{FetchReq, fetch};
use super::retry::{clear_crawl_failure, report_crawl_failure, with_retry};
use super::{WatchRow, log, set_url_if_null};
use crate::catalog::notification_title;
use crate::core::db::{self, P};
use crate::core::js;
use crate::jsx;
use crate::notify;

// 지원 통화 (frontend/src/catalog.ts FX_CURRENCIES와 동기 유지)
// per: 고시 기준 단위 (엔·루피아는 100 단위로 고시됨)
const CURRENCIES: &[(&str, &str, &str, u32)] = &[
    ("USD", "달러", "FX_USDKRW", 1),
    ("JPY", "엔", "FX_JPYKRW", 100),
    ("EUR", "유로", "FX_EURKRW", 1),
    ("CNY", "위안", "FX_CNYKRW", 1),
    ("IDR", "루피아", "FX_IDRKRW", 100),
    ("TWD", "대만달러", "FX_TWDKRW", 1),
    ("HKD", "홍콩달러", "FX_HKDKRW", 1),
];

/// CURRENCIES[currency] → (label, reuters). Object.prototype 이름이면 TS 에서 truthy 인데 속성이 없어 'undefined'
fn currency_info(currency: &str) -> Option<(&'static str, &'static str)> {
    if let Some((_, label, reuters, _)) = CURRENCIES.iter().find(|c| c.0 == currency) {
        return Some((label, reuters));
    }
    jsx::is_proto_key(currency).then_some(("undefined", "undefined"))
}

/// config.currency 를 CURRENCIES 키로 (문자열이 아니면 JS 처럼 문자열로 바꿔 찾는다)
fn currency_key(v: Option<&Value>) -> Option<String> {
    match v {
        None | Some(Value::Null) => None,
        Some(v) => Some(js::to_js_string(Some(v))),
    }
}

// 알림 클릭 시 이동할 네이버 환율 상세 (모바일)
fn detail_url(reuters: &str) -> String {
    format!("https://m.stock.naver.com/marketindex/exchange/{reuters}")
}

pub struct Quote {
    pub price: String,
    pub ratio: f64,
}

async fn fetch_quote(reuters: &str) -> Result<Quote, String> {
    with_retry(&format!("환율 {reuters}"), |_| async move {
        let res = fetch(FetchReq::get(
            format!(
                "https://m.stock.naver.com/front-api/marketIndex/productDetail?category=exchange&reutersCode={reuters}"
            ),
            &[("User-Agent", "Mozilla/5.0")],
            10_000,
        ))
        .await?;
        if !res.ok() {
            return Err(format!("naver_http_{}", res.status));
        }
        let data = res.json()?;
        let result = jsx::prop(Some(&data), "result")?;
        if !js::truthy(jsx::prop(Some(&data), "isSuccess")?)
            || !js::truthy(jsx::opt(result, "closePrice"))
        {
            return Err("naver_bad_response".into());
        }
        Ok(Quote {
            price: jsx::tpl(jsx::opt(result, "closePrice")),
            ratio: js::to_number(jsx::opt(result, "fluctuationsRatio")),
        })
    })
    .await
}

/// 전일 대비 표기 ('+0.5' / '-0.98' / '0')
pub fn ratio_text(ratio: f64) -> String {
    if ratio > 0.0 {
        format!("+{}", jsx::num_str(ratio))
    } else {
        jsx::num_str(ratio)
    }
}

// 같은 회차의 환율 알림들을 묶어 통화별 1회만 조회한다
pub async fn run(watches: Vec<WatchRow>) -> Result<(), String> {
    // 통화 → 해당 watch 목록 (Map 삽입 순서)
    let mut by_currency: Vec<(String, Vec<WatchRow>)> = vec![];
    for watch in watches {
        let config = jsx::json_parse(&watch.config)?;
        let currency = currency_key(jsx::prop(Some(&config), "currency")?).unwrap_or_default();
        if currency_info(&currency).is_none() {
            log(&format!(
                "환율 알림 통화 설정 없음: watch {} \"{}\" - 건너뜀",
                watch.id, watch.name
            ));
            continue;
        }
        match by_currency.iter_mut().find(|(c, _)| *c == currency) {
            Some((_, g)) => g.push(watch),
            None => by_currency.push((currency, vec![watch])),
        }
    }

    join_all(
        by_currency
            .into_iter()
            .map(|(currency, mut group)| async move {
                let (_, reuters) = currency_info(&currency).unwrap();
                let quote = match fetch_quote(reuters).await {
                    Ok(q) => q,
                    Err(message) => {
                        log(&format!("환율 조회 실패 ({currency}): {message}"));
                        for watch in group.iter_mut() {
                            report_crawl_failure(
                                watch,
                                &format!("환율 조회 실패 ({currency}): {message}"),
                            )?;
                        }
                        return Ok::<(), String>(());
                    }
                };
                for watch in group.iter_mut() {
                    clear_crawl_failure(watch)?;
                }
                // 묶음 알림은 본문 첫 줄만 쓰므로 전일 대비를 같은 줄에 붙인다
                let body = format!("{}원 ({}%)", quote.price, ratio_text(quote.ratio));
                let url = detail_url(reuters);
                for watch in &group {
                    // 리다이렉트 URL 자동 설정 (사용자가 직접 넣지 않으므로 비어있을 때 채움)
                    if watch.url_empty() {
                        set_url_if_null(watch.id, Some(&url))?;
                    }
                    notify::enqueue(
                        "fx",
                        &notification_title("fx", &watch.name),
                        &body,
                        watch.url_or(Some(&url)).as_deref(),
                        watch.id,
                    )
                    .map_err(|e| e.0)?;
                }
                log(&format!(
                    "환율 조회 완료: {currency} {}원 (알림 {}건)",
                    quote.price,
                    group.len()
                ));
                Ok(())
            }),
    )
    .await;
    Ok(())
}

pub fn validate_config(
    config: Option<&Value>,
    exclude_id: Option<f64>,
) -> Result<Option<String>, String> {
    let currency = jsx::prop(config, "currency")?;
    let Some(Value::String(currency)) = currency else {
        return Ok(Some("통화를 선택해 주세요".into()));
    };
    let Some((label, _)) = currency_info(currency) else {
        return Ok(Some("통화를 선택해 주세요".into()));
    };
    // 1통화 1알림 - 활성 목록에 같은 통화가 이미 있으면 거부
    let c = db::conn();
    let dup = db::get(
        &c,
        "SELECT id FROM watches
         WHERE provider = 'fx' AND deleted_at IS NULL AND id != ?
           AND json_extract(config, '$.currency') = ?",
        &[
            P::Num(exclude_id.unwrap_or(-1.0)),
            P::from(currency.as_str()),
        ],
    )
    .map_err(|e| e.0)?;
    if dup.is_some() {
        return Ok(Some(format!("{label} 환율 알림이 이미 있어요")));
    }
    Ok(None)
}
