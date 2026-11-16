use donoti_backend::config::{CONFIG, HOST};
use donoti_backend::core::app::{App, Routes};
use donoti_backend::core::{config, db, password, session};
use donoti_backend::{params, routes};

// users 테이블이 비어있으면 env의 초기 계정 생성
async fn bootstrap_initial_user() -> db::Result<()> {
    let count = {
        let c = db::conn();
        db::get(&c, "SELECT COUNT(*) AS c FROM users", params![])?
            .and_then(|r| db::int(&r, "c"))
            .unwrap_or(0)
    };
    if count > 0 {
        return Ok(());
    }
    let (Some(u), Some(p)) = (&CONFIG.initial_username, &CONFIG.initial_password) else {
        return Ok(());
    };
    let hash = password::hash(p.clone()).await;
    let c = db::conn();
    db::run(
        &c,
        "INSERT INTO users (username, password_hash) VALUES (?, ?)",
        params![u.clone(), hash],
    )?;
    Ok(())
}

fn build_routes() -> Routes {
    let mut r = Routes::new();
    r.add("GET", "", "/api/health", None, |_req| async {
        Ok(donoti_backend::core::app::Reply::ok(
            serde_json::json!({ "ok": true }),
        ))
    });
    routes::auth::register(&mut r);
    routes::push::register(&mut r);
    routes::notifications::register(&mut r);
    routes::settings::register(&mut r);
    routes::watches::register(&mut r);
    routes::stocks::register(&mut r);
    routes::links::register(&mut r);
    routes::movies::register(&mut r);
    r
}

#[tokio::main]
async fn main() {
    config::init_log();
    if let Err(e) = run().await {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    donoti_backend::db::open()?;
    donoti_backend::crawl_log::init();
    donoti_backend::db::migrate()?;
    bootstrap_initial_user().await?;
    session::init_store()?;
    let app = App {
        routes: build_routes(),
        session: session::Config {
            secret: CONFIG.session_secret.clone(),
            secure: config::production(),
        },
    };
    donoti_backend::notify::start().await?;
    donoti_backend::scheduler::start();
    donoti_backend::core::app::serve(app, HOST, CONFIG.port).await?;
    Ok(())
}
