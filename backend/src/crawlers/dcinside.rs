// 디시인사이드 갤러리 새 글 감시 - 갤러리 링크 + 키워드(여러 개) 등록 방식
// 목록 페이지(gall.dcinside.com)는 정적 HTML이라 fetch + 정규식으로 충분하다.
// 글 번호(data-no)가 단조 증가하므로 직전 회차 최대 번호(state.lastNo) 이후 글만 새 글로 본다.
use std::sync::LazyLock;

use futures_util::FutureExt;
use futures_util::future::join_all;
use regex::Regex;
use serde_json::{Value, json};

use super::http::{FetchReq, fetch};
use super::movie::RowCache;
use super::nintendo::{CHROME_UA, TITLE};
use super::retry::{
    clear_crawl_failure, report_crawl_failure, save_state, set_state_key, with_retry,
};
use super::{WatchRow, log};
use crate::core::db::P;
use crate::core::js;
use crate::jsx::{self, JS_S, prop};
use crate::notify;

const HOST: &str = "gall.dcinside.com";
const UA: [(&str, &str); 2] = [
    ("User-Agent", CHROME_UA),
    ("Accept-Language", "ko-KR,ko;q=0.9"),
];
const TIMEOUT: u64 = 15_000;
// 새 글이 한 페이지를 넘길 때 더 넘겨보는 최대 페이지 수
const MAX_PAGES: u32 = 3;
// 한 회차에 watch 하나가 보내는 최대 알림 수 (키워드가 너무 넓을 때 폭주 방지)
const MAX_NOTIFY: usize = 10;
static TITLE_SUFFIX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(" - 커뮤니티 포털 디시인사이드{JS_S}*$")).unwrap());

#[derive(Clone, Debug)]
pub struct Post {
    pub no: f64,
    pub title: String,
    pub nick: String,
    /// 'YYYY-MM-DD HH:MM:SS' (목록의 title 속성)
    pub at: String,
    pub link: String,
}

// 갤러리 종류별 목록 경로 (정식 board / 마이너 mgallery / 미니 mini)
fn list_path(ty: &str) -> String {
    if ty == "board" {
        "/board/lists/".into()
    } else {
        format!("/{ty}/board/lists/")
    }
}

fn view_path(ty: &str) -> String {
    if ty == "board" {
        "/board/view/".into()
    } else {
        format!("/{ty}/board/view/")
    }
}

/// URLSearchParams.toString() 인코딩 (application/x-www-form-urlencoded)
fn form_encode(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

// 갤러리 목록 URL (recommendOnly면 개념글 탭)
pub fn list_url(ty: &str, id: &str, page: u32, recommend_only: bool) -> String {
    let mut params = vec![format!("id={}", form_encode(id))];
    if recommend_only {
        params.push("exception_mode=recommend".into());
    }
    if page > 1 {
        params.push(format!("page={page}"));
    }
    format!("https://{HOST}{}?{}", list_path(ty), params.join("&"))
}

static ENT_DEC: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"&#([0-9]+);").unwrap());
static ENT_HEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)&#x([0-9a-f]+);").unwrap());

/// String.fromCodePoint(n) - 범위 밖이면 RangeError
fn from_code_point(n: f64) -> Result<String, String> {
    if n.fract() != 0.0 || !(0.0..=1_114_111.0).contains(&n) {
        return Err(format!("Invalid code point {}", jsx::num_str(n)));
    }
    Ok(match char::from_u32(n as u32) {
        Some(c) => c.to_string(),
        // 서로게이트 반쪽은 Rust 문자열에 담을 수 없다 (U+FFFD)
        None => "\u{fffd}".into(),
    })
}

fn replace_fallible(
    re: &Regex,
    s: &str,
    f: impl Fn(&str) -> Result<String, String>,
) -> Result<String, String> {
    let mut out = String::new();
    let mut last = 0;
    for c in re.captures_iter(s) {
        let m = c.get(0).unwrap();
        out.push_str(&s[last..m.start()]);
        out.push_str(&f(&c[1])?);
        last = m.end();
    }
    out.push_str(&s[last..]);
    Ok(out)
}

// 목록에 쓰이는 HTML 엔티티 복원
pub fn decode_entities(text: &str) -> Result<String, String> {
    let t = replace_fallible(&ENT_DEC, text, |n| from_code_point(js::string_to_number(n)))?;
    let t = replace_fallible(&ENT_HEX, &t, |n| {
        from_code_point(
            u64::from_str_radix(n, 16)
                .map(|v| v as f64)
                .unwrap_or(f64::INFINITY),
        )
    })?;
    Ok(t.replace("&quot;", "\"")
        .replace("&#039;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ")
        .replace("&amp;", "&"))
}

static ROW_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"<tr class="ub-content us-post"[^>]*data-no="([0-9]+)"[\s\S]*?</tr>"#).unwrap()
});
static TITLE_HTML: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r#"<td class="gall_tit[^"]*">[\s\S]*?<a{JS_S}+href="[^"]*"[^>]*>([\s\S]*?)</a>"#
    ))
    .unwrap()
});
static TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<[^>]+>").unwrap());
static WS: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!("{JS_S}+")).unwrap());
static NICK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"data-nick="([^"]*)""#).unwrap());
static DATE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"<td class="gall_date" title="([^"]*)""#).unwrap());

// 목록 HTML → 일반 글 목록 (공지·광고·설문 행은 us-post 클래스가 없어 제외됨)
pub fn parse_posts(html: &str, ty: &str, id: &str) -> Result<Vec<Post>, String> {
    let mut posts = vec![];
    for m in ROW_RE.captures_iter(html) {
        let row = m.get(0).unwrap().as_str();
        let no = js::string_to_number(&m[1]);
        let Some(title_html) = TITLE_HTML
            .captures(row)
            .and_then(|c| c.get(1))
            .map(|x| x.as_str())
            .filter(|s| !s.is_empty())
        else {
            continue;
        };
        let title = decode_entities(&TAG.replace_all(title_html, ""))?;
        let title = js::trim(&WS.replace_all(&title, " ")).to_string();
        let nick = decode_entities(
            NICK.captures(row)
                .map(|c| c.get(1).unwrap().as_str())
                .unwrap_or(""),
        )?;
        let at = DATE
            .captures(row)
            .map(|c| c[1].to_string())
            .unwrap_or_default();
        posts.push(Post {
            no,
            title,
            nick,
            at,
            link: format!(
                "https://{HOST}{}?id={id}&no={}",
                view_path(ty),
                jsx::num_str(no)
            ),
        });
    }
    Ok(posts)
}

static REDIRECT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"location\.replace\("https://gall\.dcinside\.com/(?:(mgallery|mini)/)?board/lists"#,
    )
    .unwrap()
});

#[derive(Clone)]
pub struct ListPage {
    pub gallery_name: String,
    pub posts: Vec<Post>,
}

/// 목록 HTML 해석 (fetchList 의 본문 처리)
pub fn parse_list(html: &str, ty: &str, id: &str) -> Result<ListPage, String> {
    // 갤러리 종류(정식/마이너/미니)가 다르면 본문 없이 JS 리다이렉트만 온다 → 올바른 종류를 알려준다
    if let Some(r) = REDIRECT.captures(html) {
        return Err(format!(
            "dcinside_redirect_{}",
            r.get(1).map(|m| m.as_str()).unwrap_or("board")
        ));
    }
    let raw_title = TITLE
        .captures(html)
        .map(|c| c[1].to_string())
        .unwrap_or_default();
    if !TITLE_SUFFIX.is_match(&raw_title) {
        return Err("dcinside_parse".into());
    }
    let gallery_name = decode_entities(js::trim(&TITLE_SUFFIX.replace(&raw_title, "")))?;
    Ok(ListPage {
        gallery_name,
        posts: parse_posts(html, ty, id)?,
    })
}

// 목록 페이지 1장 조회 → 갤러리 이름 + 글 목록
async fn fetch_list(
    ty: String,
    id: String,
    page: u32,
    recommend_only: bool,
) -> Result<ListPage, String> {
    let url = list_url(&ty, &id, page, recommend_only);
    let html = with_retry(&format!("디시 목록 {id} p{page}"), |_| {
        let url = url.clone();
        async move {
            let res = fetch(FetchReq::get(url, &UA, TIMEOUT)).await?;
            if !res.ok() {
                return Err(format!("dcinside_http_{}", res.status));
            }
            Ok(res.text())
        }
    })
    .await?;
    parse_list(&html, &ty, &id)
}

static ID_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-Za-z0-9_-]+$").unwrap());
static REDIRECTED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^dcinside_redirect_(board|mgallery|mini)$").unwrap());

/// 갤러리 링크(PC·모바일·글 링크 모두 허용)를 해석해 갤러리 정보를 돌려준다. 실패 시 한국어 메시지로 Err
pub async fn resolve_dcinside_link(input: &str) -> Result<Value, String> {
    let extracted = jsx::extract_url(input);
    let Ok(url) = url::Url::parse(&extracted) else {
        return Err("올바른 링크가 아니에요".into());
    };
    let mut ty: Option<String> = None;
    let mut id: Option<String> = None;
    let path = url.path().to_string();
    if url.host_str() == Some(HOST) {
        // /board/lists/?id=x, /mgallery/board/view/?id=x&no=1, /mini/board/lists/?id=x
        let seg = path.split('/').nth(1).unwrap_or("");
        ty = match seg {
            "mgallery" | "mini" | "board" => Some(seg.to_string()),
            _ => None,
        };
        id = super::emart::search_param(&url, "id");
    } else if url.host_str() == Some("m.dcinside.com") {
        // 모바일: /board/<id>, /mini/<id>, /board/<id>/<no> - 마이너 여부는 접근해서 판별
        let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        match parts.first() {
            Some(&"mini") => ty = Some("mini".into()),
            Some(&"board") => ty = Some("board".into()),
            _ => {}
        }
        id = parts.get(1).map(|s| s.to_string());
    }
    let (Some(ty), Some(id)) = (ty, id.filter(|i| !i.is_empty() && ID_RE.is_match(i))) else {
        return Err("디시인사이드 갤러리 링크(gall.dcinside.com)만 등록할 수 있어요".into());
    };

    // 종류가 다르면(모바일 링크는 정식/마이너 구분이 없음) 리다이렉트 안내를 따라 한 번 더 시도
    let mut gallery_type = ty;
    for _ in 0..2 {
        match fetch_list(gallery_type.clone(), id.clone(), 1, false).await {
            Ok(page) => {
                return Ok(json!({
                    "galleryId": id,
                    "galleryType": gallery_type,
                    "galleryName": page.gallery_name,
                    "link": list_url(&gallery_type, &id, 1, false),
                    "latestNo": js::num(page.posts.first().map(|p| p.no).unwrap_or(0.0)),
                }));
            }
            Err(message) => {
                let redirected = REDIRECTED.captures(&message).map(|c| c[1].to_string());
                match redirected {
                    Some(r) if r != gallery_type => gallery_type = r,
                    _ => break,
                }
            }
        }
    }
    Err("갤러리를 불러오지 못했어요. 갤러리 주소를 확인해 주세요".into())
}

// 키워드 정규화 (대소문자·공백 무시)
fn norm(text: &str) -> String {
    WS.replace_all(&text.to_lowercase(), "").into_owned()
}

/// 같은 회차 안에서 (갤러리, 페이지, 개념글 여부) 조회를 한 번만 하기 위한 캐시
#[derive(Default)]
struct ListCache(RowCache);

impl ListCache {
    async fn get(
        &self,
        ty: &str,
        id: &str,
        page: u32,
        recommend_only: bool,
    ) -> Result<ListPage, String> {
        let key = format!(
            "{ty}/{id}/{page}/{}",
            if recommend_only { "r" } else { "a" }
        );
        let (t, i) = (ty.to_string(), id.to_string());
        let rows = self
            .0
            .get(key, move || {
                async move {
                    let p = fetch_list(t, i, page, recommend_only).await?;
                    Ok(vec![
                        Value::String(p.gallery_name.clone()),
                        posts_to_value(&p.posts),
                    ])
                }
                .boxed()
            })
            .await?;
        Ok(ListPage {
            gallery_name: rows[0].as_str().unwrap_or("").to_string(),
            posts: value_to_posts(&rows[1]),
        })
    }
}

fn posts_to_value(posts: &[Post]) -> Value {
    Value::Array(
        posts
            .iter()
            .map(|p| json!([p.no, p.title, p.nick, p.at, p.link]))
            .collect(),
    )
}

fn value_to_posts(v: &Value) -> Vec<Post> {
    v.as_array()
        .map(|a| {
            a.iter()
                .map(|p| Post {
                    no: p[0].as_f64().unwrap_or(f64::NAN),
                    title: p[1].as_str().unwrap_or("").into(),
                    nick: p[2].as_str().unwrap_or("").into(),
                    at: p[3].as_str().unwrap_or("").into(),
                    link: p[4].as_str().unwrap_or("").into(),
                })
                .collect()
        })
        .unwrap_or_default()
}

// watch 하나 크롤링. 기준 번호(state.lastNo)는 등록 시 initialState가 잡아두므로 첫 회차부터 알림이 간다.
// 기준이 없으면(구버전 등록분) 이번 회차 최대 번호만 기록하고 알림은 보내지 않는다
async fn run_watch(watch: &mut WatchRow, cache: &ListCache) -> Result<(), String> {
    let config = jsx::json_parse(&watch.config)?;
    let ty = prop(Some(&config), "galleryType")?.cloned();
    let id = prop(Some(&config), "galleryId")?.cloned();
    let keywords: Vec<String> = match jsx::nn(prop(Some(&config), "keywords")?) {
        None => vec![],
        Some(Value::Array(a)) => a
            .iter()
            .map(|k| match k {
                Value::String(s) => Ok(norm(s)),
                Value::Null => {
                    Err("Cannot read properties of null (reading 'toLowerCase')".to_string())
                }
                _ => Err("text.toLowerCase is not a function".to_string()),
            })
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .filter(|k| !k.is_empty())
            .collect(),
        Some(_) => return Err("(config.keywords ?? []).map is not a function".into()),
    };
    if !js::truthy(ty.as_ref()) || !js::truthy(id.as_ref()) || keywords.is_empty() {
        log(&format!(
            "디시 알림 설정 없음: watch {} \"{}\" - 건너뜀",
            watch.id, watch.name
        ));
        return Ok(());
    }
    let (ty, id) = (jsx::tpl(ty.as_ref()), jsx::tpl(id.as_ref()));
    let recommend_only = prop(Some(&config), "recommendOnly")? == Some(&Value::Bool(true));
    let mut state = jsx::json_parse(watch.state.as_deref().unwrap_or("{}"))?;
    let last_no = jsx::as_num(prop(Some(&state), "lastNo")?);

    // 직전 번호보다 새 글만 모은다 - 한 페이지를 넘기면 다음 페이지까지 (최대 MAX_PAGES)
    let mut fresh: Vec<Post> = vec![];
    let mut gallery_name = match jsx::nn(prop(Some(&config), "galleryName")?) {
        Some(v) => jsx::tpl(Some(v)),
        None => id.clone(),
    };
    let mut max_no = last_no.unwrap_or(0.0);
    for page in 1..=MAX_PAGES {
        let list = cache.get(&ty, &id, page, recommend_only).await?;
        gallery_name = list.gallery_name.clone();
        if list.posts.is_empty() {
            break;
        }
        // Math.max(maxNo, ...nos) - NaN 이 하나라도 있으면 NaN
        for p in &list.posts {
            max_no = if max_no.is_nan() || p.no.is_nan() {
                f64::NAN
            } else {
                max_no.max(p.no)
            };
        }
        let Some(last) = last_no else { break };
        let newer: Vec<Post> = list.posts.iter().filter(|p| p.no > last).cloned().collect();
        let n_newer = newer.len();
        fresh.extend(newer);
        // 페이지 끝까지 전부 새 글이면 다음 페이지에도 새 글이 있을 수 있다
        if n_newer < list.posts.len() {
            break;
        }
    }

    if max_no != last_no.unwrap_or(-1.0) {
        set_state_key(&mut state, "lastNo", js::num(max_no))?;
        save_state(watch, &state)?;
    }
    let link = prop(Some(&config), "link")?.cloned();
    if watch.url_empty() && js::truthy(link.as_ref()) {
        let c = crate::core::db::conn();
        crate::core::db::run(
            &c,
            "UPDATE watches SET url = ? WHERE id = ? AND url IS NULL",
            &[
                crate::core::db::from_json(link.as_ref()).map_err(|e| e.0)?,
                P::from(watch.id),
            ],
        )
        .map_err(|e| e.0)?;
    }
    let Some(_) = last_no else {
        log(&format!(
            "디시 기준 번호 기록: \"{}\" {gallery_name} #{} - 이후 새 글부터 알림",
            watch.name,
            jsx::num_str(max_no)
        ));
        return Ok(());
    };

    // 오래된 글부터 순서대로, 키워드 일치 글만
    let mut matched: Vec<&Post> = fresh
        .iter()
        .filter(|p| {
            let t = norm(&p.title);
            keywords.iter().any(|k| t.contains(k.as_str()))
        })
        .collect();
    matched.sort_by(|a, b| a.no.partial_cmp(&b.no).unwrap_or(std::cmp::Ordering::Equal));
    if matched.is_empty() {
        log(&format!(
            "디시 새 글 {}건 중 키워드 일치 없음: \"{}\"",
            fresh.len(),
            watch.name
        ));
        return Ok(());
    }
    let overflow = matched.len() as i64 - MAX_NOTIFY as i64;
    for post in &matched[matched.len().saturating_sub(MAX_NOTIFY)..] {
        let time = js::utf16_slice(&post.at, 11, 16);
        // 제목은 글 제목 그대로, 본문에 갤러리·글쓴이·시각
        let body = if time.is_empty() {
            format!("{gallery_name} · {}", post.nick)
        } else {
            format!("{gallery_name} · {} · {time}", post.nick)
        };
        notify::enqueue("dcinside", &post.title, &body, Some(&post.link), watch.id)
            .map_err(|e| e.0)?;
    }
    if overflow > 0 {
        notify::enqueue_raw(
            P::from("dcinside"),
            P::from(format!("{gallery_name} 새 글 {overflow}건 더 있어요")),
            P::from(format!(
                "키워드 일치 글이 많아 최근 {MAX_NOTIFY}건만 알렸어요"
            )),
            crate::core::db::from_json(link.as_ref()).map_err(|e| e.0)?,
            P::from(watch.id),
        )
        .map_err(|e| e.0)?;
    }
    log(&format!(
        "디시 새 글 알림: \"{}\" {}건 (새 글 {}건 중)",
        watch.name,
        matched.len(),
        fresh.len()
    ));
    Ok(())
}

pub async fn run(mut watches: Vec<WatchRow>) -> Result<(), String> {
    let cache = ListCache::default();
    join_all(watches.iter_mut().map(|watch| {
        let cache = &cache;
        async move {
            let r = async {
                run_watch(watch, cache).await?;
                clear_crawl_failure(watch)
            }
            .await;
            if let Err(message) = r {
                log(&format!("디시 크롤링 실패 (watch {}): {message}", watch.id));
                let _ = report_crawl_failure(watch, &format!("디시인사이드 수집 실패: {message}"));
            }
        }
    }))
    .await;
    Ok(())
}

pub fn validate_config(config: Option<&Value>) -> Result<Option<String>, String> {
    if !matches!(prop(config, "galleryId")?, Some(Value::String(s)) if !s.is_empty()) {
        return Ok(Some("갤러리 링크를 불러와 주세요".into()));
    }
    let ty = prop(config, "galleryType")?;
    if !matches!(
        ty.and_then(|v| v.as_str()),
        Some("board" | "mgallery" | "mini")
    ) {
        return Ok(Some("갤러리 링크를 불러와 주세요".into()));
    }
    let empty = vec![];
    let keywords = match prop(config, "keywords")? {
        Some(Value::Array(a)) => a,
        _ => &empty,
    };
    if keywords.iter().any(|k| !k.is_string()) {
        return Ok(Some("키워드가 올바르지 않아요".into()));
    }
    if keywords
        .iter()
        .filter(|k| !js::trim(k.as_str().unwrap_or("")).is_empty())
        .count()
        == 0
    {
        return Ok(Some("키워드를 1개 이상 입력해 주세요".into()));
    }
    if keywords.len() > 20 {
        return Ok(Some("키워드는 20개까지 등록할 수 있어요".into()));
    }
    if !matches!(prop(config, "latestNo")?, Some(Value::Number(_))) {
        return Ok(Some("갤러리 링크를 다시 불러와 주세요".into()));
    }
    Ok(None)
}

// 등록 시점의 최신 글 번호를 기준으로. 수정 시 갤러리가 그대로면 기존 기준 유지 (키워드만 바꿔도 재알림 없음)
pub fn initial_state(
    config: Option<&Value>,
    previous: Option<&Value>,
) -> Result<Option<Value>, String> {
    if let Some(p) = previous.filter(|p| js::truthy(Some(p)))
        && jsx::strict_eq(prop(Some(p), "galleryId")?, prop(config, "galleryId")?)
        && jsx::strict_eq(prop(Some(p), "galleryType")?, prop(config, "galleryType")?)
    {
        return Ok(None);
    }
    Ok(Some(jsx::obj(vec![(
        "lastNo",
        prop(config, "latestNo")?.cloned(),
    )])))
}
