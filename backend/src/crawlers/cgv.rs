// CGV 크롤러 - 영화 1편 × 지점 N × 날짜 N × 시간범위의 상영·잔여석 감시
// 알림은 지점당 1건 (매칭 상영들의 관·시간·잔여석 나열)
// 예매 가능(잔여석 있는 상영)이 있을 때만 알림
use std::sync::Mutex;

use futures_util::FutureExt;
use futures_util::future::join_all;
use serde_json::{Value, json};

use super::http::{FetchReq, fetch};
use super::movie::{self, DAY_LABELS, Hit, RowCache, Rows, iter_of, nonempty, valid_dates};
use super::retry::{clear_crawl_failure, report_crawl_failure, with_retry};
use super::{WatchRow, log, set_url_if_null};
use crate::catalog::notification_title;
use crate::core::js;
use crate::jsx::{self, prop};
use crate::notify;
use crate::watch_schedule::kst_now;

const API: &str = "https://cgv.co.kr/api/v1";
const CO_CD: &str = "A420";
const UA: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/148.0.0.0 Safari/537.36";
const HEADERS: [(&str, &str); 4] = [
    ("User-Agent", UA),
    ("Accept", "application/json"),
    ("Accept-Language", "ko-KR"),
    ("Referer", "https://cgv.co.kr/cnm/movieBook/cinema"),
];

const BOOKING_URL: &str = "https://cgv.co.kr/cnm/movieBook/cinema";

// CGV는 쿠키(__cf_bm 등) 없는 반복 호출을 간헐적으로 HTML 에러 페이지로 차단한다.
// 예매 페이지에서 쿠키를 받아 유지하고, 차단 감지 시 쿠키 갱신 후 1회 재시도.
static COOKIE: Mutex<String> = Mutex::new(String::new());

async fn refresh_cookie() -> Result<(), String> {
    let res = fetch(FetchReq::get(
        BOOKING_URL,
        &[("User-Agent", UA), ("Accept-Language", "ko-KR")],
        10_000,
    ))
    .await?;
    let set_cookies = res.set_cookies();
    if !set_cookies.is_empty() {
        *COOKIE.lock().unwrap() = set_cookies
            .iter()
            .map(|c| c.split(';').next().unwrap_or("").to_string())
            .collect::<Vec<_>>()
            .join("; ");
    }
    Ok(())
}

async fn fetch_json(label: &str, url: &str) -> Result<Value, String> {
    // 최대 3회 재시도 - 2회차부터는 쿠키를 새로 받아 차단을 우회한다
    with_retry(label, |attempt| async move {
        if attempt > 0 {
            refresh_cookie().await?;
        }
        let mut req = FetchReq::get(url, &HEADERS, 10_000);
        let cookie = COOKIE.lock().unwrap().clone();
        if !cookie.is_empty() {
            req.headers.push(("Cookie".into(), cookie));
        }
        let res = fetch(req).await?;
        if !res.ok() {
            return Err(format!("cgv_http_{}", res.status));
        }
        let text = res.text();
        // 차단 시 200이어도 HTML 에러 페이지가 온다
        if !text.trim_start_matches(js::is_js_space).starts_with('{') {
            return Err("cgv_blocked".into());
        }
        jsx::json_parse(&text)
    })
    .await
}

/// (x ?? []).map(...) 대상
fn list(v: Option<&Value>, expr: &str) -> Result<Vec<Value>, String> {
    match jsx::nn(v) {
        None => Ok(vec![]),
        Some(Value::Array(a)) => Ok(a.clone()),
        Some(_) => Err(format!("({expr} ?? []).map is not a function")),
    }
}

// ── 등록 폼용 목록 ────────────────────────────────────────────

/// 현재 예매 가능한 상영작 목록
pub async fn fetch_cgv_movies() -> Result<Value, String> {
    let res = fetch_json(
        "CGV 상영작",
        &format!("{API}/booking/searchAtktTopPostrList?coCd={CO_CD}&movNm=&div=&attrCd="),
    )
    .await?;
    let rows = list(prop(Some(&res), "data")?, "res.data")?;
    rows.iter()
        .map(|m| {
            Ok(jsx::obj(vec![
                ("code", prop(Some(m), "movNo")?.cloned()),
                ("name", prop(Some(m), "movNm")?.cloned()),
            ]))
        })
        .collect::<Result<Vec<_>, String>>()
        .map(Value::Array)
}

/// 전체 지점 목록 (지역명 포함)
pub async fn fetch_cgv_theaters() -> Result<Value, String> {
    let res = fetch_json(
        "CGV 지점목록",
        &format!("{API}/content/site/searchAllRegionAndSite?coCd={CO_CD}"),
    )
    .await?;
    let data = prop(Some(&res), "data")?;
    let mut area_names: Vec<(Value, Value)> = vec![];
    for r in list(prop(data, "regionInfo")?, "res.data.regionInfo")? {
        let k = prop(Some(&r), "comCdval")?.cloned().unwrap_or(Value::Null);
        let v = prop(Some(&r), "comCdvalNm")?.cloned();
        match area_names
            .iter_mut()
            .find(|(x, _)| jsx::same_value_zero(x, &k))
        {
            Some(e) => e.1 = v.unwrap_or(Value::Null),
            None => area_names.push((k, v.unwrap_or(Value::Null))),
        }
    }
    list(prop(data, "siteInfo")?, "res.data.siteInfo")?
        .iter()
        .map(|s| {
            let g = prop(Some(s), "regnGrpCd")?.cloned().unwrap_or(Value::Null);
            let area = area_names
                .iter()
                .find(|(k, _)| jsx::same_value_zero(k, &g))
                .map(|(_, v)| v.clone())
                .filter(|v| !v.is_null())
                .unwrap_or(json!(""));
            Ok(jsx::obj(vec![
                ("code", prop(Some(s), "siteNo")?.cloned()),
                ("name", prop(Some(s), "siteNm")?.cloned()),
                ("area", Some(area)),
            ]))
        })
        .collect::<Result<Vec<_>, String>>()
        .map(Value::Array)
}

// ── 상영표 ────────────────────────────────────────────────────

// 지점·날짜별 전체 상영표
async fn fetch_screenings(site_no: String, ymd8: String) -> Rows {
    let res = fetch_json(
        &format!("CGV 상영표 {site_no} {ymd8}"),
        &format!(
            "{API}/booking/searchMovScnInfo?coCd={CO_CD}&siteNo={site_no}&scnYmd={ymd8}&rtctlScopCd=08"
        ),
    )
    .await?;
    list(prop(Some(&res), "data")?, "res.data")
}

// 'HH:MM' → 'HHMM' (상영표의 scnsrtTm 형식)
fn to_hm4(v: &Value) -> Result<String, String> {
    match v {
        Value::String(s) => Ok(s.replacen(':', "", 1)),
        _ => Err("hhmm.replace is not a function".into()),
    }
}

// '20260821' → '8/21 (금)'
fn date_header(ymd8: &str) -> String {
    let sl = |a, b| js::utf16_slice(ymd8, a, b);
    let date = format!("{}-{}-{}", sl(0, 4), sl(4, 6), sl(6, 8));
    let day = movie::local_date(&date)
        .map(|(_, _, w)| DAY_LABELS[w])
        .unwrap_or("undefined");
    format!(
        "{}/{} ({day})",
        jsx::num_str(js::string_to_number(&sl(4, 6))),
        jsx::num_str(js::string_to_number(&sl(6, 8)))
    )
}

// watch 하나 크롤링. 지점당 1건 알림
async fn run_watch(watch: &mut WatchRow, cache: &RowCache) -> Result<(), String> {
    let config = jsx::json_parse(&watch.config)?;
    if config.is_null() {
        return Err("Cannot destructure property 'movieNo' of 'config' as it is null.".into());
    }
    let movie_no = prop(Some(&config), "movieNo")?.cloned();
    let stores = prop(Some(&config), "stores")?;
    let dates = prop(Some(&config), "dates")?;
    if !js::truthy(movie_no.as_ref()) || !nonempty(stores) || !nonempty(dates) {
        log(&format!(
            "CGV 알림 설정 없음: watch {} \"{}\" - 건너뜀",
            watch.id, watch.name
        ));
        return Ok(());
    }
    let start = jsx::nn(prop(Some(&config), "start")?)
        .cloned()
        .unwrap_or(json!("00:00"));
    let end = jsx::nn(prop(Some(&config), "end")?)
        .cloned()
        .unwrap_or(json!("23:59"));
    let start_tm = json!(to_hm4(&start)?);
    let end_tm = json!(to_hm4(&end)?);
    // 종료가 23:59(끝까지)면 상한 없음 - 심야 회차는 '24:30'처럼 표기되므로 그대로 포함된다
    let no_upper = end_tm == json!("2359");
    let imax_only = js::truthy(prop(Some(&config), "imaxOnly")?);

    // 오늘(KST) 이후 날짜만 조회
    let today = js::utf16_slice(&kst_now().stamp, 0, 10);
    let valid: Vec<String> = valid_dates(&iter_of(dates, "dates")?, &today)
        .iter()
        .map(|d| movie::replace_all(d, "-", "", "d"))
        .collect::<Result<_, String>>()?;

    let mut fetch_failed = false;
    for store in iter_of(stores, "stores")? {
        let code = jsx::tpl(prop(Some(&store), "code")?);
        let store_name = jsx::tpl(prop(Some(&store), "name")?);
        // 잔여석 있는 회차 수집 후 시간순 정렬, 날짜 헤더로 그룹핑
        let mut hits: Vec<Hit> = vec![];
        for ymd8 in &valid {
            let (c2, y2) = (code.clone(), ymd8.clone());
            let rows = cache
                .get(format!("{code}:{ymd8}"), move || {
                    fetch_screenings(c2, y2).boxed()
                })
                .await;
            let rows = match rows {
                Ok(r) => r,
                Err(message) => {
                    log(&format!(
                        "CGV 상영표 조회 실패 ({store_name} {ymd8}): {message}"
                    ));
                    fetch_failed = true;
                    report_crawl_failure(
                        watch,
                        &format!("CGV 상영표 조회 실패 ({store_name} {ymd8}): {message}"),
                    )?;
                    continue;
                }
            };
            for r in &rows {
                let tm = prop(Some(r), "scnsrtTm")?;
                let ok = jsx::strict_eq(prop(Some(r), "movNo")?, movie_no.as_ref())
                    && jsx::js_ge(tm, Some(&start_tm))
                    && (no_upper || jsx::js_le(tm, Some(&end_tm)))
                    && (!imax_only
                        || jsx::str_includes(
                            prop(Some(r), "movkndDsplNm")?,
                            "IMAX",
                            "r.movkndDsplNm",
                        )?
                        || jsx::str_includes(
                            prop(Some(r), "expoScnsNm")?,
                            "IMAX",
                            "r.expoScnsNm",
                        )?);
                if !ok {
                    continue;
                }
                let seats = js::to_number(prop(Some(r), "frSeatCnt")?);
                if seats <= 0.0 {
                    continue; // 매진 회차는 나열하지 않음
                }
                hits.push(Hit {
                    key: format!("{ymd8}{}", jsx::tpl(tm)),
                    day: ymd8.clone(),
                    text: format!(
                        "{}:{} {} · {}석",
                        jsx::str_slice(tm, 0, 2, "r.scnsrtTm")?,
                        jsx::str_slice(tm, 2, 4, "r.scnsrtTm")?,
                        jsx::tpl(prop(Some(r), "expoScnsNm")?),
                        jsx::num_str(seats)
                    ),
                });
            }
        }
        // 예매 가능한 회차가 없으면 알림 없음
        if hits.is_empty() {
            continue;
        }
        let n = hits.len();
        let body = movie::hits_body(hits, date_header);

        if watch.url_empty() {
            set_url_if_null(watch.id, Some(BOOKING_URL))?;
        }
        notify::enqueue(
            "cgv",
            &notification_title("cgv", &format!("{} · {store_name}", watch.name)),
            &body,
            watch.url_or(Some(BOOKING_URL)).as_deref(),
            watch.id,
        )
        .map_err(|e| e.0)?;
        log(&format!(
            "CGV 상영 확인: \"{}\" {store_name} ({n}회)",
            watch.name
        ));
    }
    if !fetch_failed {
        clear_crawl_failure(watch)?;
    }
    Ok(())
}

pub async fn run(mut watches: Vec<WatchRow>) -> Result<(), String> {
    // 같은 회차의 (지점,날짜) 상영표는 1회만 조회
    let cache = RowCache::default();
    let results = join_all(watches.iter_mut().map(|w| run_watch(w, &cache))).await;
    movie::settle(results, "CGV", "cgv_all_failed")
}
