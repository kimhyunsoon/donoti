// 크롤러 fixture 재생 (Rust 쪽) - 저장된 응답으로 크롤러·링크 해석을 돌려 TS 하네스와 같은 모양의 결과를 낸다.
//   cargo run --example fixture_replay -- <scenario.json> <fixtures.json> <out.json>
// TS 쪽: plan/rust-backend/parity/donoti-fixtures/harness-ts.mjs (외부 요청 없음)
use std::collections::HashMap;
use std::sync::Mutex;

use serde_json::{Value, json};

use donoti_backend::core::{db, js};
use donoti_backend::crawlers::http::{FetchReq, FetchRes, ReqBody, set_hook};
use donoti_backend::crawlers::{
    LOG_SINK, WatchRow, cgv, dcinside, emart, get_crawler, lotte, megabox, nintendo, playstation,
};

static REQS: Mutex<Vec<Value>> = Mutex::new(Vec::new());

fn req_json(r: &FetchReq) -> Value {
    let mut headers: Vec<(String, String)> = r
        .headers
        .iter()
        .map(|(k, v)| (k.to_ascii_lowercase(), v.clone()))
        .collect();
    headers.sort();
    let body = match &r.body {
        None => Value::Null,
        Some(ReqBody::Text(t)) => json!(t),
        Some(ReqBody::Form(f)) => json!({ "form": f }),
    };
    json!({
        "method": r.method,
        "url": r.url,
        "headers": headers.iter().map(|(k, v)| json!([k, v])).collect::<Vec<_>>(),
        "body": body,
        "redirect": if r.redirect_manual { "manual" } else { "follow" },
        "timeout": r.timeout_ms,
    })
}

fn key_of(r: &Value) -> String {
    let body = if r["body"].is_null() {
        String::new()
    } else {
        js::stringify(&r["body"])
    };
    format!(
        "{} {} {body}",
        r["method"].as_str().unwrap(),
        r["url"].as_str().unwrap()
    )
}

fn crawl_log_lines(dir: &str) -> Vec<String> {
    let mut out = vec![];
    if let Ok(rd) = std::fs::read_dir(format!("{dir}/logs")) {
        let mut files: Vec<_> = rd.flatten().map(|e| e.path()).collect();
        files.sort();
        for f in files {
            for l in std::fs::read_to_string(f).unwrap_or_default().lines() {
                if !l.is_empty() {
                    out.push(l.get(22..).unwrap_or("").to_string());
                }
            }
        }
    }
    out
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (scenario, fixtures, out_file) = (&args[1], &args[2], &args[3]);
    let dir = std::env::temp_dir().join(format!("donoti-fx-rs-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let dir = dir.to_string_lossy().to_string();
    // SAFETY: 스레드를 만들기 전(설정 읽기 전)에 한 번
    unsafe { std::env::set_var("DATA_DIR", &dir) };
    donoti_backend::db::open().unwrap();
    donoti_backend::crawl_log::init();
    donoti_backend::db::migrate().unwrap();
    LOG_SINK.set(Mutex::new(vec![])).unwrap();

    let fx: HashMap<String, Vec<Value>> =
        serde_json::from_str(&std::fs::read_to_string(fixtures).unwrap()).unwrap();
    let served: Mutex<HashMap<String, usize>> = Mutex::new(HashMap::new());
    set_hook(move |r: &FetchReq| {
        let rec = req_json(r);
        let key = key_of(&rec);
        REQS.lock().unwrap().push(rec);
        let Some(list) = fx.get(&key) else {
            return Err("fixture_missing".into());
        };
        let mut s = served.lock().unwrap();
        let i = s.entry(key).and_modify(|i| *i += 1).or_insert(0);
        let e = &list[(*i).min(list.len() - 1)];
        if let Some(err) = e.get("error").and_then(|v| v.as_str()) {
            return Err(err.to_string());
        }
        Ok(FetchRes {
            status: e["status"].as_u64().unwrap() as u16,
            headers: e["headers"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .map(|h| {
                            (
                                h[0].as_str().unwrap().to_string(),
                                h[1].as_str().unwrap().to_string(),
                            )
                        })
                        .collect()
                })
                .unwrap_or_default(),
            body: e["body"].as_str().unwrap_or("").as_bytes().to_vec(),
        })
    });

    let sc: Value = serde_json::from_str(&std::fs::read_to_string(scenario).unwrap()).unwrap();
    let mut out = vec![];
    for step in sc["steps"].as_array().unwrap() {
        REQS.lock().unwrap().clear();
        LOG_SINK.get().unwrap().lock().unwrap().clear();
        let before = crawl_log_lines(&dir).len();
        let mut r = serde_json::Map::new();
        r.insert("name".into(), step["name"].clone());
        let res: Result<Option<Value>, String> = if let Some(call) = step["call"].as_str() {
            match call {
                "cgvMovies" => cgv::fetch_cgv_movies().await,
                "cgvTheaters" => cgv::fetch_cgv_theaters().await,
                "lotteMovies" => lotte::fetch_lotte_movies().await,
                "lotteTheaters" => lotte::fetch_lotte_theaters().await,
                "megaboxMovies" => megabox::fetch_megabox_movies().await,
                "megaboxTheaters" => megabox::fetch_megabox_theaters().await,
                _ => panic!("call {call}"),
            }
            .map(Some)
        } else if let Some(p) = step["resolve"].as_str() {
            let input = step["input"].as_str().unwrap_or("");
            match p {
                "emart" => emart::resolve_emart_link(input).await,
                "nintendo" => nintendo::resolve_nintendo_link(input).await,
                "playstation" => playstation::resolve_playstation_link(input).await,
                "dcinside" => dcinside::resolve_dcinside_link(input).await,
                _ => panic!("resolve {p}"),
            }
            .map(Some)
        } else {
            let provider = step["run"].as_str().unwrap();
            let ws = step["watches"].as_array().unwrap();
            let ids: Vec<String> = ws.iter().map(|w| w["id"].to_string()).collect();
            {
                let c = db::conn();
                for w in ws {
                    db::run(
                        &c,
                        "INSERT INTO watches (id, category, provider, name, url, schedule, config, state) VALUES (?, 'x', ?, ?, ?, '[]', ?, ?)",
                        &[
                            db::P::Num(w["id"].as_f64().unwrap()),
                            db::P::from(provider),
                            db::P::from(w["name"].as_str().unwrap()),
                            db::P::from(w["url"].as_str()),
                            db::P::from(js::stringify(&w["config"])),
                            db::P::from(w["state"].as_str()),
                        ],
                    )
                    .unwrap();
                }
            }
            let rows: Vec<WatchRow> = {
                let c = db::conn();
                db::all(
                    &c,
                    &format!(
                        "SELECT * FROM watches WHERE id IN ({}) ORDER BY id",
                        ids.join(",")
                    ),
                    &[],
                )
                .unwrap()
                .iter()
                .map(WatchRow::from_row)
                .collect()
            };
            let res = get_crawler(provider).unwrap().run(rows).await;
            let c = db::conn();
            let mut notis: Vec<Value> = db::all(
                &c,
                &format!(
                    "SELECT source, title, body, url, watch_id FROM notifications WHERE watch_id IN ({})",
                    ids.join(",")
                ),
                &[],
            )
            .unwrap()
            .into_iter()
            .map(Value::Object)
            .collect();
            notis.sort_by_key(js::stringify);
            r.insert("notifications".into(), Value::Array(notis));
            let w = db::all(
                &c,
                &format!(
                    "SELECT id, url, state, deleted_at IS NOT NULL AS deleted FROM watches WHERE id IN ({}) ORDER BY id",
                    ids.join(",")
                ),
                &[],
            )
            .unwrap();
            r.insert(
                "watches".into(),
                Value::Array(w.into_iter().map(Value::Object).collect()),
            );
            res.map(|_| None)
        };
        match res {
            Ok(Some(v)) => {
                r.insert("value".into(), v);
            }
            Ok(None) => {}
            Err(e) => {
                r.insert("error".into(), json!(e));
            }
        }
        let mut logs = LOG_SINK.get().unwrap().lock().unwrap().clone();
        logs.sort();
        r.insert("logs".into(), json!(logs));
        let mut cl: Vec<String> = crawl_log_lines(&dir).split_off(before);
        cl.sort();
        r.insert("crawlLog".into(), json!(cl));
        let mut reqs = REQS.lock().unwrap().clone();
        reqs.sort_by_key(key_of);
        r.insert("requests".into(), Value::Array(reqs));
        out.push(Value::Object(r));
    }
    std::fs::write(out_file, js::stringify(&Value::Array(out))).unwrap();
    println!(
        "{} steps → {out_file}",
        sc["steps"].as_array().unwrap().len()
    );
    let _ = std::fs::remove_dir_all(&dir);
}
