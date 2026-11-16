// 크롤링 스케줄러 - 매분 0초(KST)에 스케줄을 평가해 일치하는 크롤러를 실행
use std::collections::HashSet;
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

use crate::core::db;
use crate::crawlers::{WatchRow, get_crawler, log};
use crate::jsx;
use crate::notify::now_ms;
use crate::params;
use crate::watch_schedule::{kst_now, matches_schedule};

// 동시 실행 방지 - 이전 배치가 아직 안 끝난 provider는 이번 틱을 건너뛴다
static RUNNING: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(Default::default);

fn tick() -> Result<(), String> {
    let now = kst_now();

    // 종료 시점이 지난 알림은 목록에서 제거 (soft delete - 종료된 알림 페이지에서 복구 가능)
    let (expired, watches) = {
        let c = db::conn();
        let expired = db::run(
            &c,
            "UPDATE watches SET deleted_at = datetime('now'), updated_at = datetime('now')
       WHERE deleted_at IS NULL AND ends_at IS NOT NULL AND ends_at <= ?",
            params![now.stamp.clone()],
        )
        .map_err(|e| e.0)?;
        // 임시 중지(enabled=0)는 크롤링 대상에서 제외
        let watches = db::all(
            &c,
            "SELECT * FROM watches WHERE deleted_at IS NULL AND enabled = 1",
            params![],
        )
        .map_err(|e| e.0)?;
        (expired.changes, watches)
    };
    if expired > 0 {
        log(&format!("종료 시점이 지난 알림 {expired}건 정리"));
    }

    // 같은 시각에 매칭된 watch는 provider별로 묶어 배치 1회로 처리한다 (Map 삽입 순서)
    let mut matched: Vec<(String, Vec<WatchRow>)> = vec![];
    for r in &watches {
        let watch = WatchRow::from_row(r);
        let Ok(rules) = jsx::json_parse(&watch.schedule) else {
            continue;
        };
        if !matches_schedule(&rules, now.day, &now.hhmm)? {
            continue;
        }
        match matched.iter_mut().find(|(p, _)| *p == watch.provider) {
            Some((_, g)) => g.push(watch),
            None => matched.push((watch.provider.clone(), vec![watch])),
        }
    }

    for (provider, group) in matched {
        let Some(crawler) = get_crawler(&provider) else {
            log(&format!(
                "크롤러 미구현: {provider} (알림 {}건) - 건너뜀",
                group.len()
            ));
            continue;
        };
        if !RUNNING.lock().unwrap().insert(provider.clone()) {
            log(&format!(
                "이전 크롤링 진행 중: {provider} - 이번 회차 건너뜀"
            ));
            continue;
        }
        let n = group.len();
        tokio::spawn(async move {
            if let Err(message) = crawler.run(group).await {
                log(&format!("크롤링 실패 ({provider}, 알림 {n}건): {message}"));
            }
            RUNNING.lock().unwrap().remove(&provider);
        });
    }
    Ok(())
}

/// 크롤링 스케줄러를 시작한다. 매분 0초(KST)에 스케줄을 평가해 일치하는 크롤러를 실행
pub fn start() {
    tokio::spawn(async {
        loop {
            // 매 틱 다음 분 0초에 재정렬 (setInterval 드리프트 누적 방지)
            let wait = 60_000 - now_ms().rem_euclid(60_000);
            tokio::time::sleep(Duration::from_millis(wait as u64)).await;
            if let Err(e) = tick() {
                log(&format!("스케줄러 틱 오류: {e}"));
            }
        }
    });
    log("크롤링 스케줄러 시작 (매분 평가, KST)");
}
