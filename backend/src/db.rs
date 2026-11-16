use crate::config::CONFIG;
use crate::core::db::{self, Result};
use crate::crawlers::stock_symbols::seed_stock_symbols;

const SCHEMA: &str = include_str!("schema.sql");

pub fn open() -> Result<()> {
    db::open(&format!("{}/donoti.db", CONFIG.data_dir))
}

// 스키마 적용 (IF NOT EXISTS 기반이라 기동 시마다 실행해도 안전)
pub fn migrate() -> Result<()> {
    let c = db::conn();
    db::exec(&c, SCHEMA)?;
    // 기존 DB에 없는 컬럼 추가
    let noti_cols = db::columns(&c, "notifications")?;
    if !noti_cols.iter().any(|n| n == "read_at") {
        db::exec(&c, "ALTER TABLE notifications ADD COLUMN read_at TEXT")?;
    }
    if !noti_cols.iter().any(|n| n == "watch_id") {
        db::exec(&c, "ALTER TABLE notifications ADD COLUMN watch_id INTEGER")?;
    }
    let watch_cols = db::columns(&c, "watches")?;
    if !watch_cols.iter().any(|n| n == "enabled") {
        db::exec(
            &c,
            "ALTER TABLE watches ADD COLUMN enabled INTEGER NOT NULL DEFAULT 1",
        )?;
    }
    if !watch_cols.iter().any(|n| n == "state") {
        db::exec(&c, "ALTER TABLE watches ADD COLUMN state TEXT")?;
    }
    seed_stock_symbols(&c)
}
