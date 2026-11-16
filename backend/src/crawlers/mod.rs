// 크롤러 레지스트리 - 각 provider는 서로 다른 프로그램처럼 개별 구현해 여기에 등록한다
//
// provider별 크롤러. 같은 시각에 매칭된 같은 provider의 watch들은
// 배열로 묶여 한 번에 처리된다 (알림당 1크롤링이 아니라 종류당 1크롤링).
// - run: 배치 실행. 감지 시 watch별 enqueue 호출과 필요하면 watch.url(리다이렉트 URL) 자동 설정이 구현의 책무
// - validate_config: 등록·수정 시 config 검증 (excludeId = 수정 중인 watch id, 신규면 None).
//   Ok(None) = 문제 없음, Ok(Some(문구)) = 400, Err = TS 에서 TypeError(500)
// - initial_state: 등록·수정 시 watch.state 초기값 (None 이면 기존 state 유지)
use serde_json::Value;

use crate::core::db;

pub mod cgv;
pub mod dcinside;
pub mod emart;
pub mod fx;
pub mod http;
pub mod lotte;
pub mod megabox;
pub mod movie;
pub mod nintendo;
pub mod playstation;
pub mod retry;
pub mod stock;
pub mod stock_symbols;

/// watches 테이블 행
#[derive(Clone, Debug)]
pub struct WatchRow {
    pub id: i64,
    pub category: String,
    pub provider: String,
    pub name: String,
    pub url: Option<String>,
    /// JSON ScheduleRule[]
    pub schedule: String,
    /// JSON (provider별 폼 데이터)
    pub config: String,
    /// 크롤러 전용 상태 JSON
    pub state: Option<String>,
}

impl WatchRow {
    pub fn from_row(r: &db::Row) -> WatchRow {
        let s = |k: &str| crate::core::js::to_js_string(r.get(k));
        WatchRow {
            id: db::int(r, "id").unwrap_or(0),
            category: s("category"),
            provider: s("provider"),
            name: s("name"),
            url: db::text(r, "url").map(String::from),
            schedule: s("schedule"),
            config: s("config"),
            state: db::text(r, "state").map(String::from),
        }
    }

    /// !watch.url (null 또는 빈 문자열)
    pub fn url_empty(&self) -> bool {
        self.url.as_deref().is_none_or(str::is_empty)
    }

    /// watch.url ?? fallback
    pub fn url_or(&self, fallback: Option<&str>) -> Option<String> {
        self.url.clone().or(fallback.map(String::from))
    }
}

/// 리다이렉트 URL 자동 설정 (사용자가 직접 넣지 않으므로 비어있을 때 채움)
pub fn set_url_if_null(watch_id: i64, url: Option<&str>) -> Result<(), String> {
    let c = db::conn();
    db::run(
        &c,
        "UPDATE watches SET url = ? WHERE id = ? AND url IS NULL",
        &[db::P::from(url), db::P::from(watch_id)],
    )
    .map_err(|e| e.0)?;
    Ok(())
}

/// 크롤러 진행 로그 (TS 의 log = app.log.info). 시험 하네스는 LOG_SINK 로 모아 본다
pub fn log(msg: &str) {
    if let Some(sink) = LOG_SINK.get() {
        sink.lock().unwrap().push(msg.to_string());
    }
    tracing::info!("{msg}");
}

/// 시험용 로그 수집 (fixture 하네스만 설정)
pub static LOG_SINK: std::sync::OnceLock<std::sync::Mutex<Vec<String>>> =
    std::sync::OnceLock::new();

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Crawler {
    Fx,
    Stock,
    Emart,
    Nintendo,
    Playstation,
    Cgv,
    Lotte,
    Megabox,
    Dcinside,
}

// provider → 크롤러 (전 카탈로그 구현 완료)
pub fn get_crawler(provider: &str) -> Option<Crawler> {
    Some(match provider {
        "fx" => Crawler::Fx,
        "stock" => Crawler::Stock,
        "emart" => Crawler::Emart,
        "nintendo" => Crawler::Nintendo,
        "playstation" => Crawler::Playstation,
        "cgv" => Crawler::Cgv,
        "lotte" => Crawler::Lotte,
        "megabox" => Crawler::Megabox,
        "dcinside" => Crawler::Dcinside,
        _ => return None,
    })
}

impl Crawler {
    pub async fn run(self, watches: Vec<WatchRow>) -> Result<(), String> {
        match self {
            Crawler::Fx => fx::run(watches).await,
            Crawler::Stock => stock::run(watches).await,
            Crawler::Emart => emart::run(watches).await,
            Crawler::Nintendo => nintendo::run(watches).await,
            Crawler::Playstation => playstation::run(watches).await,
            Crawler::Cgv => cgv::run(watches).await,
            Crawler::Lotte => lotte::run(watches).await,
            Crawler::Megabox => megabox::run(watches).await,
            Crawler::Dcinside => dcinside::run(watches).await,
        }
    }

    pub fn validate_config(
        self,
        config: Option<&Value>,
        exclude_id: Option<f64>,
    ) -> Result<Option<String>, String> {
        match self {
            Crawler::Fx => fx::validate_config(config, exclude_id),
            Crawler::Stock => stock::validate_config(config, exclude_id),
            Crawler::Emart => emart::validate_config(config),
            Crawler::Nintendo => nintendo::validate_config(config),
            Crawler::Playstation => playstation::validate_config(config),
            Crawler::Cgv | Crawler::Lotte | Crawler::Megabox => movie_validate_config(config),
            Crawler::Dcinside => dcinside::validate_config(config),
        }
    }

    /// crawler?.initialState?.(config, previous) ?? null
    pub fn initial_state(
        self,
        config: Option<&Value>,
        previous: Option<&Value>,
    ) -> Result<Option<Value>, String> {
        match self {
            Crawler::Dcinside => dcinside::initial_state(config, previous),
            _ => Ok(None),
        }
    }
}

/// CGV·롯데·메가박스 공통 config 검증 (세 파일의 validateConfig 가 같다)
fn movie_validate_config(config: Option<&Value>) -> Result<Option<String>, String> {
    use crate::jsx::prop;
    let movie_no = prop(config, "movieNo")?;
    if !matches!(movie_no, Some(Value::String(s)) if !s.is_empty()) {
        return Ok(Some("영화를 선택해 주세요".into()));
    }
    if !matches!(prop(config, "stores")?, Some(Value::Array(a)) if !a.is_empty()) {
        return Ok(Some("지점을 1곳 이상 선택해 주세요".into()));
    }
    if !matches!(prop(config, "dates")?, Some(Value::Array(a)) if !a.is_empty()) {
        return Ok(Some("날짜를 1개 이상 선택해 주세요".into()));
    }
    if let (Some(Value::String(s)), Some(Value::String(e))) =
        (prop(config, "start")?, prop(config, "end")?)
        && crate::jsx::js_str_gt(s, e)
    {
        return Ok(Some("시작 시각이 종료 시각보다 늦어요".into()));
    }
    Ok(None)
}
