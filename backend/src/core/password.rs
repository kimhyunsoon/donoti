// 4개 앱 공통 사본 - 고칠 때 네 곳을 같이 고친다.
//
// node `argon2` 0.41 기본값과 같은 argon2id (m=65536, t=3, p=4, 출력 32바이트, salt 16바이트).
// 기존 해시는 PHC 문자열의 파라미터대로 검증된다. CPU 를 쓰므로 blocking 스레드에서.
use argon2::password_hash::{PasswordHasher, PasswordVerifier};
use argon2::{Algorithm, Argon2, Params, Version};

fn hasher() -> Argon2<'static> {
    Argon2::new(
        Algorithm::Argon2id,
        Version::V0x13,
        Params::new(65536, 3, 4, Some(32)).expect("argon2 params"),
    )
}

pub fn hash_sync(plain: &str) -> String {
    let mut salt = [0u8; 16];
    rand::fill(&mut salt);
    hasher()
        .hash_password_with_salt(plain.as_bytes(), &salt)
        .expect("argon2 hash")
        .to_string()
}

pub fn verify_sync(hash: &str, plain: &str) -> bool {
    // 해시 문자열의 m/t/p 를 따른다 (Argon2::default 로 검증해도 파라미터는 해시에서 읽음)
    Argon2::default()
        .verify_password(plain.as_bytes(), hash)
        .is_ok()
}

pub async fn hash(plain: String) -> String {
    tokio::task::spawn_blocking(move || hash_sync(&plain))
        .await
        .expect("argon2 task")
}

pub async fn verify(hash: String, plain: String) -> bool {
    tokio::task::spawn_blocking(move || verify_sync(&hash, &plain))
        .await
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_format() {
        let h = hash_sync("pw1234");
        assert!(h.starts_with("$argon2id$v=19$m=65536,t=3,p=4$"));
        assert!(verify_sync(&h, "pw1234"));
        assert!(!verify_sync(&h, "nope"));
        assert!(!verify_sync("garbage", "pw1234"));
    }
}
