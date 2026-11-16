// 알림 발송 큐 워커 - 생산자(크롤러·API)는 notifications 행만 넣고, 워커가 Web Push로 브로드캐스트한다
use std::collections::HashMap;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::time::Duration;

use regex::Regex;
use serde_json::json;

use crate::catalog::group_notification_title;
use crate::config::CONFIG;
use crate::core::db::{self, P};
use crate::core::{js, webpush};
use crate::jsx::JS_S;
use crate::params;

const MAX_ATTEMPTS: i64 = 5;
// 재시도 백오프: 10초 → 1분 → 5분 → 15분
const BACKOFF_MS: [i64; 4] = [10_000, 60_000, 300_000, 900_000];
const POLL_MS: u64 = 5_000;
// 같은 회차에 쌓인 같은 종류(source)는 브라우저 알림 하나로 합친다 - 넘치면 나눠서 발송
const GROUP_MAX: usize = 4;
// 합쳐진 본문의 항목당 최대 길이 (넘으면 말줄임)
const GROUP_LINE_MAX: usize = 50;
// 발송 완료 알림 보관 기간 (failed는 수동 확인용이라 자동 삭제하지 않음)
const SENT_RETENTION: &str = "-30 days";

pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// 알림 큐에 등록한다. 발송은 워커가 담당. 값은 TS 가 넘기는 JS 값 그대로 (body ?? '', url ?? null, watchId ?? null)
/// 반환: 생성된 알림 id
pub fn enqueue_raw(source: P, title: P, body: P, url: P, watch_id: P) -> db::Result<i64> {
    let c = db::conn();
    let r = db::run(
        &c,
        "INSERT INTO notifications (source, title, body, url, watch_id, next_attempt_at)
       VALUES (?, ?, ?, ?, ?, ?)",
        &[source, title, body, url, watch_id, P::Num(now_ms() as f64)],
    )?;
    Ok(r.last_insert_rowid)
}

/// enqueueNotification - 이미 펼친 인자 (source, title, body, url, watchId)
pub fn enqueue_args(mut args: Vec<P>) -> db::Result<i64> {
    args.push(P::Num(now_ms() as f64));
    let c = db::conn();
    let r = db::run(
        &c,
        "INSERT INTO notifications (source, title, body, url, watch_id, next_attempt_at)
       VALUES (?, ?, ?, ?, ?, ?)",
        &args,
    )?;
    Ok(r.last_insert_rowid)
}

/// 크롤러용 enqueueNotification({ source, title, body, url, watchId })
pub fn enqueue(
    source: &str,
    title: &str,
    body: &str,
    url: Option<&str>,
    watch_id: i64,
) -> db::Result<i64> {
    enqueue_raw(
        P::from(source),
        P::from(title),
        P::from(body),
        P::from(url),
        P::from(watch_id),
    )
}

#[derive(Clone, Debug)]
pub struct NotificationRow {
    pub id: i64,
    pub source: String,
    pub title: String,
    pub body: String,
    pub url: Option<String>,
    pub attempts: i64,
}

static EMOJI_PREFIX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"^[\p{{Extended_Pictographic}}\u{{FE0F}}\u{{200D}}]+{JS_S}*"
    ))
    .unwrap()
});

// 합쳐진 알림의 항목 한 줄: 개별 제목(이모지 제외) + 본문 첫 줄
pub fn summary_line(row: &NotificationRow) -> String {
    let title = EMOJI_PREFIX.replace(&row.title, "");
    let first_body = js::trim(row.body.split('\n').next().unwrap_or(""));
    let line = if !first_body.is_empty() {
        format!("{title} · {first_body}")
    } else {
        title.into_owned()
    };
    if js::utf16_len(&line) > GROUP_LINE_MAX {
        format!("{}…", js::utf16_slice(&line, 0, GROUP_LINE_MAX - 1))
    } else {
        line
    }
}

// 브라우저 알림 페이로드. 2건 이상이면 제목·본문을 합치고 ids로 일괄 읽음 처리를 지원한다
pub fn build_payload(rows: &[NotificationRow], unread: i64) -> String {
    if rows.len() == 1 {
        let row = &rows[0];
        return js::stringify(&json!({
            // id: OS 알림 클릭 시 서비스워커가 해당 알림을 읽음 처리하는 데 사용
            "id": row.id,
            "title": row.title,
            "body": row.body,
            "url": row.url.as_deref().unwrap_or("/"),
            "unread": unread,
        }));
    }
    js::stringify(&json!({
        "ids": rows.iter().map(|r| r.id).collect::<Vec<_>>(),
        "title": group_notification_title(&rows[0].source, rows.len()),
        "body": rows.iter().map(summary_line).collect::<Vec<_>>().join("\n"),
        "url": "/",
        "unread": unread,
    }))
}

fn count_unread(c: &rusqlite::Connection) -> db::Result<i64> {
    Ok(db::get(
        c,
        "SELECT COUNT(*) AS c FROM notifications WHERE status = 'sent' AND read_at IS NULL",
        params![],
    )?
    .and_then(|r| db::int(&r, "c"))
    .unwrap_or(0))
}

// 모든 구독 기기로 Web Push 발송 (같은 종류 묶음은 알림 1개로 합쳐서). 만료 구독(404/410)은
// 정리하고 전달 수를 반환. 구독이 있는데 전부 실패하면 Err → 큐 재시도로 이어진다
async fn broadcast(rows: &[NotificationRow]) -> Result<i64, String> {
    let (subs, prev) = {
        let c = db::conn();
        let subs = db::all(&c, "SELECT * FROM push_subscriptions", params![]).map_err(|e| e.0)?;
        if subs.is_empty() {
            return Ok(0);
        }
        // unread = 앱 아이콘 배지 카운트 (이번 묶음 포함) - 서비스워커가 setAppBadge에 사용
        (subs, count_unread(&c).map_err(|e| e.0)?)
    };
    let payload = build_payload(rows, prev + rows.len() as i64);
    let mut delivered = 0;
    let mut last_error = String::new();
    for sub in &subs {
        let s = webpush::Subscription {
            endpoint: db::text(sub, "endpoint").unwrap_or(""),
            p256dh: db::text(sub, "keys_p256dh").unwrap_or(""),
            auth: db::text(sub, "keys_auth").unwrap_or(""),
        };
        match webpush::send(&CONFIG.vapid, &s, &payload).await {
            Ok(_) => delivered += 1,
            Err(err) => {
                // 만료된 구독은 정리 (실패로 치지 않음)
                if matches!(err.status, Some(404 | 410)) {
                    let c = db::conn();
                    db::run(
                        &c,
                        "DELETE FROM push_subscriptions WHERE id = ?",
                        params![db::int(sub, "id")],
                    )
                    .map_err(|e| e.0)?;
                } else {
                    last_error = err.message;
                    tracing::info!(
                        "푸시 발송 실패 (구독 {}): {last_error}",
                        db::int(sub, "id").unwrap_or(0)
                    );
                }
            }
        }
    }
    let remaining = {
        let c = db::conn();
        db::get(
            &c,
            "SELECT COUNT(*) AS c FROM push_subscriptions",
            params![],
        )
        .map_err(|e| e.0)?
        .and_then(|r| db::int(&r, "c"))
        .unwrap_or(0)
    };
    if delivered == 0 && remaining > 0 {
        return Err(if last_error.is_empty() {
            "push_failed".into()
        } else {
            last_error
        });
    }
    Ok(delivered)
}

static BUSY: AtomicBool = AtomicBool::new(false);
static LAST_CLEANUP: AtomicI64 = AtomicI64::new(0);

fn row_of(r: &db::Row) -> NotificationRow {
    NotificationRow {
        id: db::int(r, "id").unwrap_or(0),
        source: js::to_js_string(r.get("source")),
        title: js::to_js_string(r.get("title")),
        body: js::to_js_string(r.get("body")),
        url: db::text(r, "url").map(String::from),
        attempts: db::int(r, "attempts").unwrap_or(0),
    }
}

async fn tick() {
    if BUSY.swap(true, Ordering::SeqCst) {
        return;
    }
    if let Err(e) = tick_inner().await {
        tracing::error!("{e}");
    }
    BUSY.store(false, Ordering::SeqCst);
}

async fn tick_inner() -> Result<(), String> {
    let rows: Vec<NotificationRow> = {
        let c = db::conn();
        db::all(
            &c,
            "SELECT id, source, title, body, url, attempts FROM notifications
           WHERE status = 'pending' AND next_attempt_at <= ?
           ORDER BY id LIMIT 10",
            params![now_ms() as f64],
        )
        .map_err(|e| e.0)?
        .iter()
        .map(row_of)
        .collect()
    };

    // 같은 종류(source)끼리 묶는다 - 알림센터 행은 그대로 두고 브라우저 알림만 합쳐진다 (Map 삽입 순서)
    let mut order: Vec<String> = vec![];
    let mut groups: HashMap<String, Vec<NotificationRow>> = HashMap::new();
    for row in rows {
        if !groups.contains_key(&row.source) {
            order.push(row.source.clone());
        }
        groups.entry(row.source.clone()).or_default().push(row);
    }

    for source in order {
        let group = &groups[&source];
        // 너무 길어지지 않게 GROUP_MAX건씩 나눠서 발송
        for chunk in group.chunks(GROUP_MAX) {
            {
                let c = db::conn();
                for row in chunk {
                    db::run(
                        &c,
                        "UPDATE notifications SET status = 'sending' WHERE id = ?",
                        params![row.id],
                    )
                    .map_err(|e| e.0)?;
                }
            }
            let label = if chunk.len() == 1 {
                format!("\"{}\"", chunk[0].title)
            } else {
                format!("{}건 (합쳐서 발송)", chunk.len())
            };
            match broadcast(chunk).await {
                Ok(delivered) => {
                    let c = db::conn();
                    for row in chunk {
                        db::run(
                            &c,
                            "UPDATE notifications
                 SET status = 'sent', delivered = ?, sent_at = datetime('now'), last_error = NULL
                 WHERE id = ?",
                            params![delivered, row.id],
                        )
                        .map_err(|e| e.0)?;
                    }
                    tracing::info!(
                        "알림 발송 완료: {} {label} ({delivered}기기)",
                        chunk[0].source
                    );
                }
                Err(message) => {
                    let c = db::conn();
                    for row in chunk {
                        let attempts = row.attempts + 1;
                        if attempts >= MAX_ATTEMPTS {
                            db::run(
                                &c,
                                "UPDATE notifications SET status = 'failed', attempts = ?, last_error = ? WHERE id = ?",
                                params![attempts, message.clone(), row.id],
                            )
                            .map_err(|e| e.0)?;
                        } else {
                            let backoff = BACKOFF_MS
                                [((attempts - 1).min(BACKOFF_MS.len() as i64 - 1)).max(0) as usize];
                            db::run(
                                &c,
                                "UPDATE notifications
                   SET status = 'pending', attempts = ?, next_attempt_at = ?, last_error = ?
                   WHERE id = ?",
                                params![
                                    attempts,
                                    (now_ms() + backoff) as f64,
                                    message.clone(),
                                    row.id
                                ],
                            )
                            .map_err(|e| e.0)?;
                        }
                    }
                    let finals = chunk
                        .iter()
                        .filter(|r| r.attempts + 1 >= MAX_ATTEMPTS)
                        .count();
                    if finals == chunk.len() {
                        tracing::info!("알림 최종 실패: {} {label} - {message}", chunk[0].source);
                    } else {
                        tracing::info!(
                            "알림 발송 실패: {} {label} - {message} (재시도 예약)",
                            chunk[0].source
                        );
                    }
                }
            }
        }
    }

    // 오래된 발송 완료 알림 정리 (시간당 1회)
    if now_ms() - LAST_CLEANUP.load(Ordering::SeqCst) > 3_600_000 {
        LAST_CLEANUP.store(now_ms(), Ordering::SeqCst);
        let c = db::conn();
        db::run(
            &c,
            "DELETE FROM notifications WHERE status = 'sent' AND sent_at < datetime('now', ?)",
            params![SENT_RETENTION],
        )
        .map_err(|e| e.0)?;
    }
    Ok(())
}

/// 알림 큐 워커를 시작한다. pending을 주기적으로 FIFO 소비해 Web Push로 발송.
/// 첫 회차는 기동 중(listen 전)에 끝까지 돈다 (TS 는 tick() 을 동기 구간에서 시작)
pub async fn start() -> db::Result<()> {
    // 발송 중 크래시 잔재 복구 (재발송될 수 있으나 개인 알림 용도로 허용)
    {
        let c = db::conn();
        db::run(
            &c,
            "UPDATE notifications SET status = 'pending' WHERE status = 'sending'",
            params![],
        )?;
    }
    let first = tokio::spawn(tick());
    tokio::spawn(async {
        let mut timer = tokio::time::interval(Duration::from_millis(POLL_MS));
        timer.tick().await; // setInterval 은 5초 뒤 첫 실행
        loop {
            timer.tick().await;
            tokio::spawn(tick());
        }
    });
    let _ = first.await;
    tracing::info!("알림 워커 시작 (폴링 {}초)", POLL_MS / 1000);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: i64, source: &str, title: &str, body: &str) -> NotificationRow {
        NotificationRow {
            id,
            source: source.into(),
            title: title.into(),
            body: body.into(),
            url: None,
            attempts: 0,
        }
    }

    // 기대값은 TS(notify.ts summaryLine/buildPayload)를 tsx 로 돌린 출력
    #[test]
    fn summary_like_ts() {
        include!("testdata/notify_expected.in");
    }
}
