use std::sync::LazyLock;

use crate::core::config as c;
use crate::core::webpush::{self, Vapid};

// 개인용 단일 서버 전제 - 바뀔 일 없는 값은 상수로 고정
// 포트는 로컬에서 다른 프로그램(dogroo 4746/4747)과 안 겹치게 donoti의 D(4)·O(6)를 딴 4646/4647 사용
const PORT: u16 = 4646;
pub const HOST: &str = "0.0.0.0";
const VAPID_SUBJECT: &str = "mailto:sudosoon@gmail.com";

pub struct Config {
    pub port: u16,
    pub data_dir: String,
    pub session_secret: String,
    pub initial_username: Option<String>,
    pub initial_password: Option<String>,
    pub vapid: Vapid,
}

pub static CONFIG: LazyLock<Config> = LazyLock::new(|| {
    // 데이터 디렉토리 (SQLite) - 도커에서만 /data로 주입
    let data_dir = c::data_dir();
    std::fs::create_dir_all(&data_dir).expect("data dir");
    Config {
        port: c::port(PORT),
        // 세션 시크릿은 최초 기동 시 자동 생성해 데이터 디렉토리에 보관 (env 불필요)
        session_secret: c::load_secret_file(&data_dir, ".session-secret", 48),
        initial_username: c::env_nonempty("INITIAL_USERNAME"),
        initial_password: c::env_nonempty("INITIAL_PASSWORD"),
        // VAPID 키쌍(ECDSA P-256)도 최초 기동 시 자동 생성해 보관.
        // 재생성되면 기존 푸시 구독이 전부 무효가 되므로 구독 DB와 같은 디렉토리에 함께 유지한다.
        vapid: webpush::load_or_create(&data_dir, VAPID_SUBJECT),
        data_dir,
    }
});
