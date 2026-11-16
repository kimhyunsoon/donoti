// dogroo·donoti 공통 사본 - 고칠 때 두 곳을 같이 고친다.
//
// node web-push 3.6.7 과 같은 발송: RFC 8291 aes128gcm 암호화 + RFC 8292 VAPID(ES256 JWT).
// - 헤더: TTL 2419200(4주), Content-Encoding aes128gcm, Content-Type application/octet-stream, Urgency normal,
//   Authorization `vapid t=<jwt>, k=<공개키>`
// - JWT: {"typ":"JWT","alg":"ES256"} . {aud: 엔드포인트 origin, exp: 지금+12시간, sub}
// - 2xx 가 아니면 오류 `Received unexpected response code` + statusCode (404/410 은 호출 쪽이 구독 정리)
// - .vapid.json({publicKey, privateKey} base64url)은 있으면 그대로 쓰고, 없을 때만 만든다 (바뀌면 기존 구독 전부 무효)
use std::path::Path;
use std::time::Duration;

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes128Gcm, Nonce};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use hkdf::Hkdf;
use p256::ecdsa::signature::Signer;
use p256::ecdsa::{Signature, SigningKey};
use p256::elliptic_curve::sec1::ToSec1Point;
use p256::{PublicKey, SecretKey};
use sha2::Sha256;

const DEFAULT_TTL: u32 = 2_419_200;
const JWT_EXPIRATION_SECONDS: u64 = 12 * 60 * 60;
const RECORD_SIZE: u32 = 4096;

#[derive(Clone)]
pub struct Vapid {
    pub public_key: String,
    pub private_key: String,
    pub subject: String,
}

/// 데이터 폴더의 .vapid.json 읽기, 없으면 만든다 (webpush.generateVAPIDKeys 형식)
pub fn load_or_create(dir: &str, subject: &str) -> Vapid {
    let file = Path::new(dir).join(".vapid.json");
    if file.exists() {
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&file).expect("read .vapid.json"))
                .expect(".vapid.json");
        return Vapid {
            public_key: v["publicKey"].as_str().unwrap_or_default().to_string(),
            private_key: v["privateKey"].as_str().unwrap_or_default().to_string(),
            subject: subject.into(),
        };
    }
    let mut raw = [0u8; 32];
    let secret = loop {
        rand::fill(&mut raw);
        if let Ok(s) = SecretKey::from_slice(&raw) {
            break s;
        }
    };
    let public = secret.public_key().to_sec1_point(false);
    let v = Vapid {
        public_key: B64.encode(public.as_bytes()),
        private_key: B64.encode(raw),
        subject: subject.into(),
    };
    let json = format!(
        r#"{{"publicKey":"{}","privateKey":"{}"}}"#,
        v.public_key, v.private_key
    );
    super::config::write_secret(&file, &json).expect("write .vapid.json");
    v
}

pub struct Subscription<'a> {
    pub endpoint: &'a str,
    pub p256dh: &'a str,
    pub auth: &'a str,
}

#[derive(Debug)]
pub struct PushError {
    pub status: Option<u16>,
    pub message: String,
}

impl std::fmt::Display for PushError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

fn hkdf_expand(salt: &[u8], ikm: &[u8], info: &[u8], len: usize) -> Vec<u8> {
    let hk = Hkdf::<Sha256>::new(Some(salt), ikm);
    let mut out = vec![0u8; len];
    hk.expand(info, &mut out).expect("hkdf");
    out
}

/// RFC 8291 aes128gcm 본문 (as_secret·salt 를 받아 시험 벡터로 검증할 수 있게)
pub fn encrypt_with(
    as_secret: &SecretKey,
    salt: &[u8; 16],
    ua_public: &[u8],
    auth: &[u8],
    plaintext: &[u8],
) -> Result<Vec<u8>, String> {
    let ua = PublicKey::from_sec1_bytes(ua_public).map_err(|_| "invalid p256dh".to_string())?;
    let as_public = as_secret.public_key().to_sec1_point(false);
    let shared = p256::ecdh::diffie_hellman(as_secret.to_nonzero_scalar(), ua.as_affine());
    let mut key_info = b"WebPush: info\0".to_vec();
    key_info.extend_from_slice(ua_public);
    key_info.extend_from_slice(as_public.as_bytes());
    let ikm = hkdf_expand(auth, shared.raw_secret_bytes(), &key_info, 32);
    let cek = hkdf_expand(salt, &ikm, b"Content-Encoding: aes128gcm\0", 16);
    let nonce = hkdf_expand(salt, &ikm, b"Content-Encoding: nonce\0", 12);
    let mut record = plaintext.to_vec();
    record.push(0x02); // 마지막 레코드 구분자, 패딩 없음
    let cipher = Aes128Gcm::new_from_slice(&cek).map_err(|e| e.to_string())?;
    let ct = cipher
        .encrypt(
            &Nonce::try_from(nonce.as_slice()).map_err(|e| e.to_string())?,
            record.as_slice(),
        )
        .map_err(|e| e.to_string())?;
    let mut out = salt.to_vec();
    out.extend_from_slice(&RECORD_SIZE.to_be_bytes());
    out.push(as_public.as_bytes().len() as u8);
    out.extend_from_slice(as_public.as_bytes());
    out.extend_from_slice(&ct);
    Ok(out)
}

fn encrypt(p256dh: &str, auth: &str, payload: &[u8]) -> Result<Vec<u8>, String> {
    let ua = B64
        .decode(p256dh.trim_end_matches('='))
        .map_err(|e| e.to_string())?;
    let auth = B64
        .decode(auth.trim_end_matches('='))
        .map_err(|e| e.to_string())?;
    let mut raw = [0u8; 32];
    let secret = loop {
        rand::fill(&mut raw);
        if let Ok(s) = SecretKey::from_slice(&raw) {
            break s;
        }
    };
    let mut salt = [0u8; 16];
    rand::fill(&mut salt);
    encrypt_with(&secret, &salt, &ua, &auth, payload)
}

/// VAPID JWT (ES256)
pub fn vapid_jwt(vapid: &Vapid, audience: &str, exp: u64) -> Result<String, String> {
    let key = B64.decode(&vapid.private_key).map_err(|e| e.to_string())?;
    let signing = SigningKey::from_slice(&key).map_err(|e| e.to_string())?;
    let header = B64.encode(r#"{"typ":"JWT","alg":"ES256"}"#);
    let payload = B64.encode(
        serde_json::json!({ "aud": audience, "exp": exp, "sub": vapid.subject }).to_string(),
    );
    let input = format!("{header}.{payload}");
    let sig: Signature = signing.sign(input.as_bytes());
    Ok(format!("{input}.{}", B64.encode(sig.to_bytes())))
}

fn client() -> &'static reqwest::Client {
    static C: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    C.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .expect("http client")
    })
}

/// webpush.sendNotification(subscription, payload)
pub async fn send(vapid: &Vapid, sub: &Subscription<'_>, payload: &str) -> Result<u16, PushError> {
    let err = |m: String| PushError {
        status: None,
        message: m,
    };
    let url = reqwest::Url::parse(sub.endpoint).map_err(|e| err(e.to_string()))?;
    let audience = format!("{}://{}", url.scheme(), url.host_str().unwrap_or(""))
        + &url.port().map(|p| format!(":{p}")).unwrap_or_default();
    let exp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + JWT_EXPIRATION_SECONDS;
    let jwt = vapid_jwt(vapid, &audience, exp).map_err(err)?;
    let body = encrypt(sub.p256dh, sub.auth, payload.as_bytes()).map_err(err)?;
    let res = client()
        .post(url)
        .header("TTL", DEFAULT_TTL.to_string())
        .header("Content-Length", body.len().to_string())
        .header("Content-Type", "application/octet-stream")
        .header("Content-Encoding", "aes128gcm")
        .header(
            "Authorization",
            format!("vapid t={jwt}, k={}", vapid.public_key),
        )
        .header("Urgency", "normal")
        .body(body)
        .send()
        .await
        .map_err(|e| err(e.to_string()))?;
    let status = res.status().as_u16();
    let _ = res.bytes().await;
    if !(200..=299).contains(&status) {
        return Err(PushError {
            status: Some(status),
            message: "Received unexpected response code".into(),
        });
    }
    Ok(status)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 8291 부록 A 예시
    #[test]
    fn rfc8291_vector() {
        let as_priv = B64
            .decode("yfWPiYE-n46HLnH0KqZOF1fJJU3MYrct3AELtAQ-oRw")
            .unwrap();
        let ua_pub = B64.decode("BCVxsr7N_eNgVRqvHtD0zTZsEc6-VV-JvLexhqUzORcxaOzi6-AYWXvTBHm4bjyPjs7Vd8pZGH6SRpkNtoIAiw4").unwrap();
        let auth = B64.decode("BTBZMqHH6r4Tts7J_aSIgg").unwrap();
        let salt: [u8; 16] = B64
            .decode("DGv6ra1nlYgDCS1FRnbzlw")
            .unwrap()
            .try_into()
            .unwrap();
        let secret = SecretKey::from_slice(&as_priv).unwrap();
        let out = encrypt_with(
            &secret,
            &salt,
            &ua_pub,
            &auth,
            b"When I grow up, I want to be a watermelon",
        )
        .unwrap();
        assert_eq!(
            B64.encode(out),
            "DGv6ra1nlYgDCS1FRnbzlwAAEABBBP4z9KsN6nGRTbVYI_c7VJSPQTBtkgcy27mlmlMoZIIgDll6e3vCYLocInmYWAmS6TlzAC8wEqKK6PBru3jl7A_yl95bQpu6cVPTpK4Mqgkf1CXztLVBSt2Ks3oZwbuwXPXLWyouBWLVWGNWQexSgSxsj_Qulcy4a-fN"
        );
    }
}
