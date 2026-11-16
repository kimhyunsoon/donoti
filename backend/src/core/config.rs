// 4개 앱 공통 사본 - 고칠 때 네 곳을 같이 고친다.
use std::fs;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

/// DATA_DIR (기본 ./data)
pub fn data_dir() -> String {
    std::env::var("DATA_DIR").unwrap_or_else(|_| "./data".into())
}

/// 고정 포트, 단 PORT 환경 변수가 있으면 덮어쓴다 (동등성 비교용)
pub fn port(default: u16) -> u16 {
    std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(default)
}

/// TS: secure: process.env.NODE_ENV === 'production'
pub fn production() -> bool {
    std::env::var("NODE_ENV").as_deref() == Ok("production")
}

pub fn env_nonempty(k: &str) -> Option<String> {
    std::env::var(k).ok().filter(|v| !v.is_empty())
}

/// 파일 쓰기 (mode 0600, writeFileSync(file, s, { mode: 0o600 }) 와 같이 새 파일일 때만 모드 적용)
pub fn write_secret(path: &Path, content: &str) -> std::io::Result<()> {
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    f.write_all(content.as_bytes())
}

/// 비밀 파일 읽기, 없으면 hex 난수(bytes 바이트)로 만들어 보관
pub fn load_secret_file(dir: &str, name: &str, bytes: usize) -> String {
    let path = Path::new(dir).join(name);
    if path.exists() {
        return fs::read_to_string(&path)
            .expect("read secret")
            .trim()
            .to_string();
    }
    let secret = random_hex(bytes);
    write_secret(&path, &secret).expect("write secret");
    secret
}

pub fn random_hex(bytes: usize) -> String {
    let mut b = vec![0u8; bytes];
    rand::fill(&mut b[..]);
    hex::encode(b)
}

/// 로그: stdout 한 줄 (docker logs 로 본다)
pub fn init_log() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with_target(false)
        .init();
}
