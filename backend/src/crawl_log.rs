use std::io::Write;
use std::path::PathBuf;
use std::sync::LazyLock;

use crate::config::CONFIG;

static DIR: LazyLock<PathBuf> = LazyLock::new(|| {
    let dir = PathBuf::from(&CONFIG.data_dir).join("logs");
    let _ = std::fs::create_dir_all(&dir);
    dir
});

/// 기동 시 logs 폴더 생성 (TS 는 모듈 로드 때 mkdirSync)
pub fn init() {
    LazyLock::force(&DIR);
}

/// 수집(크롤링) 로그를 일자별(KST) 파일에 남긴다: {DATA_DIR}/logs/crawl-YYYY-MM-DD.log
pub fn crawl_log(message: &str) {
    // KST = UTC+9 고정
    let t = chrono::Utc::now() + chrono::Duration::hours(9);
    let stamp = t.format("%Y-%m-%d %H:%M:%S").to_string();
    let path = DIR.join(format!("crawl-{}.log", &stamp[..10]));
    // 로그 기록 실패는 수집을 막지 않는다
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = f.write_all(format!("[{stamp}] {message}\n").as_bytes());
    }
}
