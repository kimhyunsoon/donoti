// 롯데시네마 크롤러 - CGV와 동일한 방식 (영화 × 지점 N × 날짜 N × 시간범위, 지점당 1알림)
// API: LCWS/Ticketing/TicketingData.aspx (multipart form 'paramList' JSON, 쿠키 불필요)
// BookingSeatCount = 잔여석 (UI '잔여석 144 / 164'와 대조 확인)
use futures_util::FutureExt;
use futures_util::future::join_all;
use serde_json::{Map, Value, json};

use super::http::{FetchReq, ReqBody, fetch};
use super::movie::{self, Hit, RowCache, Rows, iter_of, nonempty, valid_dates};
use super::retry::{clear_crawl_failure, report_crawl_failure, with_retry};
use super::{WatchRow, log, set_url_if_null};
use crate::catalog::notification_title;
use crate::core::js;
use crate::jsx::{self, prop};
use crate::notify;
use crate::watch_schedule::kst_now;

const API: &str = "https://www.lottecinema.co.kr/LCWS/Ticketing/TicketingData.aspx";
const BOOKING_URL: &str = "https://www.lottecinema.co.kr/NLCHS/Ticketing";

async fn call_lotte(label: &str, params: &[(&str, &str)]) -> Result<Value, String> {
    // 최대 3회 재시도
    with_retry(label, |_| async {
        let mut p = Map::new();
        for (k, v) in [
            ("channelType", "HO"),
            ("osType", "W"),
            ("osVersion", "Mozilla/5.0"),
            ("multiLanguageID", "KR"),
            ("memberOnNo", "0"),
        ]
        .iter()
        .chain(params.iter())
        {
            p.insert(k.to_string(), json!(v));
        }
        let res = fetch(FetchReq {
            method: "POST",
            url: API.into(),
            headers: vec![
                ("User-Agent".into(), "Mozilla/5.0".into()),
                ("Referer".into(), BOOKING_URL.into()),
            ],
            body: Some(ReqBody::Form(vec![(
                "paramList".into(),
                js::stringify(&Value::Object(p)),
            )])),
            timeout_ms: 12_000,
            redirect_manual: false,
        })
        .await?;
        if !res.ok() {
            return Err(format!("lotte_http_{}", res.status));
        }
        res.json()
    })
    .await
}

// ── 등록 폼용 목록 (영화·지점을 1콜로) ─────────────────────────

async fn fetch_ticketing_page() -> Result<Value, String> {
    call_lotte(
        "롯데시네마 예매목록",
        &[("MethodName", "GetTicketingPageTOBE")],
    )
    .await
}

fn items<'a>(v: Option<&'a Value>, expr: &str, method: &str) -> Result<&'a Vec<Value>, String> {
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

/// 현재 예매 가능한 영화 목록
pub async fn fetch_lotte_movies() -> Result<Value, String> {
    let page = fetch_ticketing_page().await?;
    let it = prop(prop(prop(Some(&page), "Movies")?, "Movies")?, "Items")?;
    items(it, "page.Movies.Movies.Items", "map")?
        .iter()
        .map(|m| {
            Ok(jsx::obj(vec![
                ("code", prop(Some(m), "RepresentationMovieCode")?.cloned()),
                ("name", prop(Some(m), "MovieNameKR")?.cloned()),
            ]))
        })
        .collect::<Result<Vec<_>, String>>()
        .map(Value::Array)
}

/// 전체 지점 목록 (지역명 포함). code는 'divisionCode|detailDivisionCode|cinemaID'
pub async fn fetch_lotte_theaters() -> Result<Value, String> {
    let page = fetch_ticketing_page().await?;
    let areas = prop(
        prop(prop(Some(&page), "CinemaDivison")?, "AreaDivisions")?,
        "Items",
    )?;
    let mut area_names: Vec<(String, Value)> = vec![];
    for a in items(areas, "page.CinemaDivison.AreaDivisions.Items", "map")? {
        let k = format!(
            "{}|{}",
            jsx::tpl(prop(Some(a), "DivisionCode")?),
            jsx::tpl(prop(Some(a), "DetailDivisionCode")?)
        );
        let v = prop(Some(a), "GroupNameKR")?
            .cloned()
            .unwrap_or(Value::Null);
        match area_names.iter_mut().find(|(x, _)| *x == k) {
            Some(e) => e.1 = v,
            None => area_names.push((k, v)),
        }
    }
    let cinemas = prop(prop(prop(Some(&page), "Cinemas")?, "Cinemas")?, "Items")?;
    let mut out = vec![];
    // DivisionCode 1 = 지역별 극장 (2는 특별관 그룹으로 중복)
    for c in items(cinemas, "page.Cinemas.Cinemas.Items", "filter")? {
        if !jsx::strict_eq(prop(Some(c), "DivisionCode")?, Some(&json!(1))) {
            continue;
        }
        let div = format!(
            "{}|{}",
            jsx::tpl(prop(Some(c), "DivisionCode")?),
            jsx::tpl(prop(Some(c), "DetailDivisionCode")?)
        );
        let area = area_names
            .iter()
            .find(|(k, _)| *k == div)
            .map(|(_, v)| v.clone())
            .filter(|v| !v.is_null())
            .unwrap_or(json!(""));
        out.push(jsx::obj(vec![
            (
                "code",
                Some(json!(format!(
                    "{div}|{}",
                    jsx::tpl(prop(Some(c), "CinemaID")?)
                ))),
            ),
            ("name", prop(Some(c), "CinemaNameKR")?.cloned()),
            ("area", Some(area)),
        ]));
    }
    Ok(Value::Array(out))
}

// ── 상영표 ────────────────────────────────────────────────────

async fn fetch_play_sequence(cinema_code: String, date: String) -> Rows {
    let res = call_lotte(
        &format!("롯데시네마 상영표 {cinema_code} {date}"),
        &[
            ("MethodName", "GetPlaySequence"),
            ("playDate", &date),
            ("cinemaID", &cinema_code),
            ("representationMovieCode", ""),
        ],
    )
    .await?;
    match jsx::nn(prop(prop(Some(&res), "PlaySeqs")?, "Items")?) {
        None => Ok(vec![]),
        Some(Value::Array(a)) => Ok(a.clone()),
        Some(_) => Err("rows.filter is not a function".into()),
    }
}

// 관 표기: '1관' / '수퍼플렉스관' 등 특별관·특수 포맷은 함께
fn screen_label(row: &Value) -> Result<String, String> {
    let mut label = jsx::tpl(prop(Some(row), "ScreenNameKR")?);
    let div = prop(Some(row), "ScreenDivisionNameKR")?;
    if js::truthy(div) && div != Some(&json!("일반")) {
        label += &format!(" ({})", jsx::tpl(div));
    }
    let film = prop(Some(row), "FilmNameKR")?;
    if js::truthy(film) && film != Some(&json!("2D")) {
        label += &format!(" {}", jsx::tpl(film));
    }
    Ok(label)
}

// watch 하나 크롤링. 지점당 1건 알림 (CGV와 동일 형식)
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
            "롯데시네마 알림 설정 없음: watch {} \"{}\" - 건너뜀",
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
    // 종료가 23:59(끝까지)면 상한 없음 - 심야 회차는 '24:30'처럼 표기되므로 그대로 포함된다
    let no_upper = end == json!("23:59");

    let today = js::utf16_slice(&kst_now().stamp, 0, 10);
    let valid = valid_dates(&iter_of(dates, "dates")?, &today);

    let mut fetch_failed = false;
    for store in iter_of(stores, "stores")? {
        let code = jsx::tpl(prop(Some(&store), "code")?);
        let store_name = jsx::tpl(prop(Some(&store), "name")?);
        let mut hits: Vec<Hit> = vec![];
        for date in &valid {
            let date = jsx::tpl(Some(date));
            let (c2, d2) = (code.clone(), date.clone());
            let rows = cache
                .get(format!("{code}:{date}"), move || {
                    fetch_play_sequence(c2, d2).boxed()
                })
                .await;
            let rows = match rows {
                Ok(r) => r,
                Err(message) => {
                    log(&format!(
                        "롯데시네마 상영표 조회 실패 ({store_name} {date}): {message}"
                    ));
                    fetch_failed = true;
                    report_crawl_failure(
                        watch,
                        &format!("롯데시네마 상영표 조회 실패 ({store_name} {date}): {message}"),
                    )?;
                    continue;
                }
            };
            for r in &rows {
                let tm = prop(Some(r), "StartTime")?;
                let ok =
                    jsx::strict_eq(prop(Some(r), "RepresentationMovieCode")?, movie_no.as_ref())
                        && jsx::js_ge(tm, Some(&start))
                        && (no_upper || jsx::js_le(tm, Some(&end)));
                if !ok {
                    continue;
                }
                let seats = prop(Some(r), "BookingSeatCount")?;
                if jsx::js_le(seats, Some(&json!(0))) {
                    continue; // 매진 회차는 나열하지 않음
                }
                hits.push(Hit {
                    key: format!("{date}{}", jsx::tpl(tm)),
                    day: date.clone(),
                    text: format!(
                        "{} {} · {}석",
                        jsx::tpl(tm),
                        screen_label(r)?,
                        jsx::tpl(seats)
                    ),
                });
            }
        }
        // 예매 가능한 회차가 없으면 알림 없음
        if hits.is_empty() {
            continue;
        }
        let n = hits.len();
        let body = movie::hits_body(hits, movie::date_header_dash);

        if watch.url_empty() {
            set_url_if_null(watch.id, Some(BOOKING_URL))?;
        }
        notify::enqueue(
            "lotte",
            &notification_title("lotte", &format!("{} · {store_name}", watch.name)),
            &body,
            watch.url_or(Some(BOOKING_URL)).as_deref(),
            watch.id,
        )
        .map_err(|e| e.0)?;
        log(&format!(
            "롯데시네마 상영 확인: \"{}\" {store_name} ({n}회)",
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
    movie::settle(results, "롯데시네마", "lotte_all_failed")
}
