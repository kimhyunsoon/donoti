// 메가박스 크롤러 - CGV·롯데와 동일한 방식 (영화 × 지점 N × 날짜 N × 시간범위, 지점당 1건)
// API: on/oh/ohb/SimpleBooking/selectBokdList.do (JSON POST + X-Requested-With, 쿠키 불필요)
// restSeatCnt = 잔여석. 상영표 조회 시 brchNo와 함께 areaCd가 필요해 지점 code는 'brchNo|areaCd'
use futures_util::FutureExt;
use futures_util::future::join_all;
use serde_json::{Value, json};

use super::http::{FetchReq, ReqBody, fetch};
use super::movie::{self, Hit, RowCache, Rows, iter_of, nonempty, valid_dates};
use super::retry::{clear_crawl_failure, report_crawl_failure, with_retry};
use super::{WatchRow, log, set_url_if_null};
use crate::catalog::notification_title;
use crate::core::js;
use crate::jsx::{self, prop};
use crate::notify;
use crate::watch_schedule::kst_now;

const API: &str = "https://megabox.co.kr/on/oh/ohb/SimpleBooking/selectBokdList.do";
const BOOKING_URL: &str = "https://megabox.co.kr/booking";

async fn call_megabox(label: &str, body: Value) -> Result<Value, String> {
    // 최대 3회 재시도
    let body = js::stringify(&body);
    with_retry(label, |_| {
        let body = body.clone();
        async move {
            let res = fetch(FetchReq {
                method: "POST",
                url: API.into(),
                headers: vec![
                    ("User-Agent".into(), "Mozilla/5.0".into()),
                    (
                        "Content-Type".into(),
                        "application/json; charset=UTF-8".into(),
                    ),
                    ("X-Requested-With".into(), "XMLHttpRequest".into()),
                    ("Referer".into(), BOOKING_URL.into()),
                ],
                body: Some(ReqBody::Text(body)),
                timeout_ms: 12_000,
                redirect_manual: false,
            })
            .await?;
            if !res.ok() {
                return Err(format!("megabox_http_{}", res.status));
            }
            res.json()
        }
    })
    .await
}

// ── 등록 폼용 목록 (영화·지점을 1콜로) ─────────────────────────

async fn fetch_booking_page() -> Result<Value, String> {
    let today = js::utf16_slice(&kst_now().stamp, 0, 10).replace('-', "");
    call_megabox(
        "메가박스 예매목록",
        json!({
            "playDe": today,
            "incomeMovieNo": "",
            "onLoad": "Y",
            "sellChnlCd": "",
            "incomeTheabKindCd": "",
            "incomeBrchNo1": "",
            "incomePlayDe": "",
        }),
    )
    .await
}

fn list(v: Option<&Value>, expr: &str) -> Result<Vec<Value>, String> {
    match jsx::nn(v) {
        None => Ok(vec![]),
        Some(Value::Array(a)) => Ok(a.clone()),
        Some(_) => Err(format!("({expr} ?? []).map is not a function")),
    }
}

/// 현재 예매 가능한 영화 목록
pub async fn fetch_megabox_movies() -> Result<Value, String> {
    let page = fetch_booking_page().await?;
    list(prop(Some(&page), "movieList")?, "page.movieList")?
        .iter()
        .map(|m| {
            Ok(jsx::obj(vec![
                ("code", prop(Some(m), "movieNo")?.cloned()),
                ("name", prop(Some(m), "movieNm")?.cloned()),
            ]))
        })
        .collect::<Result<Vec<_>, String>>()
        .map(Value::Array)
}

/// 전체 지점 목록 (지역명 포함). code는 'brchNo|areaCd'
pub async fn fetch_megabox_theaters() -> Result<Value, String> {
    let page = fetch_booking_page().await?;
    list(prop(Some(&page), "areaBrchList")?, "page.areaBrchList")?
        .iter()
        .map(|b| {
            Ok(jsx::obj(vec![
                (
                    "code",
                    Some(json!(format!(
                        "{}|{}",
                        jsx::tpl(prop(Some(b), "brchNo")?),
                        jsx::tpl(prop(Some(b), "areaCd")?)
                    ))),
                ),
                ("name", prop(Some(b), "brchNm")?.cloned()),
                ("area", prop(Some(b), "areaCdNm")?.cloned()),
            ]))
        })
        .collect::<Result<Vec<_>, String>>()
        .map(Value::Array)
}

// ── 상영표 ────────────────────────────────────────────────────

async fn fetch_screenings(movie_no: Value, store_code: String, ymd8: String) -> Rows {
    let mut parts = store_code.split('|');
    let brch_no = parts.next().map(String::from);
    let area_cd = parts.next().map(String::from);
    let res = call_megabox(
        &format!("메가박스 상영표 {store_code} {ymd8}"),
        jsx::obj(vec![
            ("arrMovieNo", Some(movie_no.clone())),
            ("playDe", Some(json!(ymd8))),
            ("brchNoListCnt", Some(json!(1))),
            ("brchNo1", brch_no.map(Value::String)),
            ("areaCd1", area_cd.map(Value::String)),
            ("spclbYn1", Some(json!("N"))),
            ("theabKindCd1", Some(json!("10"))),
            ("movieNo1", Some(movie_no)),
            ("sellChnlCd", Some(json!(""))),
        ]),
    )
    .await?;
    match jsx::nn(prop(Some(&res), "movieFormList")?) {
        None => Ok(vec![]),
        Some(Value::Array(a)) => Ok(a.clone()),
        Some(_) => Err("rows.filter is not a function".into()),
    }
}

// 관 표기: 상영관 이름 + 2D가 아닌 상영 포맷
fn screen_label(row: &Value) -> Result<String, String> {
    let kind = match prop(Some(row), "playKindNm")? {
        Some(Value::String(s)) => s.replace("&#40;", "(").replace("&#41;", ")"),
        None => return Err("Cannot read properties of undefined (reading 'replaceAll')".into()),
        Some(Value::Null) => {
            return Err("Cannot read properties of null (reading 'replaceAll')".into());
        }
        Some(_) => return Err("row.playKindNm.replaceAll is not a function".into()),
    };
    let name = jsx::tpl(prop(Some(row), "theabExpoNm")?);
    Ok(if kind.starts_with("2D") {
        name
    } else {
        format!("{name} {kind}")
    })
}

// watch 하나 크롤링. 지점당 1건 알림 (CGV·롯데와 동일 형식)
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
            "메가박스 알림 설정 없음: watch {} \"{}\" - 건너뜀",
            watch.id, watch.name
        ));
        return Ok(());
    }
    let movie_no = movie_no.unwrap();
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
        let code = match prop(Some(&store), "code")? {
            Some(Value::String(s)) => s.clone(),
            None => return Err("Cannot read properties of undefined (reading 'split')".into()),
            Some(Value::Null) => {
                return Err("Cannot read properties of null (reading 'split')".into());
            }
            Some(_) => return Err("storeCode.split is not a function".into()),
        };
        let store_name = jsx::tpl(prop(Some(&store), "name")?);
        let mut hits: Vec<Hit> = vec![];
        for date in &valid {
            let ymd8 = movie::replace_all(date, "-", "", "date")?;
            let date = jsx::tpl(Some(date));
            let (m2, c2, y2) = (movie_no.clone(), code.clone(), ymd8.clone());
            let rows = cache
                .get(
                    format!("{}:{code}:{ymd8}", jsx::tpl(Some(&movie_no))),
                    move || fetch_screenings(m2, c2, y2).boxed(),
                )
                .await;
            let rows = match rows {
                Ok(r) => r,
                Err(message) => {
                    log(&format!(
                        "메가박스 상영표 조회 실패 ({store_name} {date}): {message}"
                    ));
                    fetch_failed = true;
                    report_crawl_failure(
                        watch,
                        &format!("메가박스 상영표 조회 실패 ({store_name} {date}): {message}"),
                    )?;
                    continue;
                }
            };
            for r in &rows {
                let tm = prop(Some(r), "playStartTime")?;
                let ok = jsx::strict_eq(prop(Some(r), "rpstMovieNo")?, Some(&movie_no))
                    && jsx::js_ge(tm, Some(&start))
                    && (no_upper || jsx::js_le(tm, Some(&end)));
                if !ok {
                    continue;
                }
                let seats = prop(Some(r), "restSeatCnt")?;
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
            "megabox",
            &notification_title("megabox", &format!("{} · {store_name}", watch.name)),
            &body,
            watch.url_or(Some(BOOKING_URL)).as_deref(),
            watch.id,
        )
        .map_err(|e| e.0)?;
        log(&format!(
            "메가박스 상영 확인: \"{}\" {store_name} ({n}회)",
            watch.name
        ));
    }
    if !fetch_failed {
        clear_crawl_failure(watch)?;
    }
    Ok(())
}

pub async fn run(mut watches: Vec<WatchRow>) -> Result<(), String> {
    // 같은 회차의 (영화,지점,날짜) 상영표는 1회만 조회
    let cache = RowCache::default();
    let results = join_all(watches.iter_mut().map(|w| run_watch(w, &cache))).await;
    movie::settle(results, "메가박스", "megabox_all_failed")
}
