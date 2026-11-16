// 4개 앱 공통 사본 - 고칠 때 네 곳을 같이 고친다.
//
// better-sqlite3 와 같은 동작:
// - 프로세스당 연결 1개(동기). 여기서는 Mutex 로 감싸 요청 사이 순서를 지킨다
// - 바인딩: JS number → REAL, string → TEXT, null/undefined → NULL (better-sqlite3 11.10 은 숫자를 전부 bind_double)
// - 결과: INTEGER → 정수, REAL → JS 숫자, TEXT → 문자열, NULL → null, 키는 컬럼 순서
// - 오류 문구 = sqlite3_errmsg (TS 에서 err.message 로 응답에 실리는 곳이 있다)
use std::sync::{Mutex, MutexGuard, OnceLock};

use rusqlite::types::{Value as Sql, ValueRef};
use rusqlite::{Connection, Statement};
use serde_json::{Map, Value};

use super::js;

static DB: OnceLock<Mutex<Connection>> = OnceLock::new();

pub type Row = Map<String, Value>;

#[derive(Debug)]
pub struct DbError(pub String, pub Option<&'static str>);

impl DbError {
    pub fn msg(m: impl Into<String>) -> DbError {
        DbError(m.into(), None)
    }
}

/// better-sqlite3 의 SqliteError.code (확장 결과 코드 이름)
fn code_name(extended: i32) -> Option<&'static str> {
    Some(match extended {
        1 => "SQLITE_ERROR",
        5 => "SQLITE_BUSY",
        8 => "SQLITE_READONLY",
        13 => "SQLITE_FULL",
        18 => "SQLITE_TOOBIG",
        19 => "SQLITE_CONSTRAINT",
        20 => "SQLITE_MISMATCH",
        25 => "SQLITE_RANGE",
        275 => "SQLITE_CONSTRAINT_CHECK",
        531 => "SQLITE_CONSTRAINT_COMMITHOOK",
        787 => "SQLITE_CONSTRAINT_FOREIGNKEY",
        1043 => "SQLITE_CONSTRAINT_FUNCTION",
        1299 => "SQLITE_CONSTRAINT_NOTNULL",
        1555 => "SQLITE_CONSTRAINT_PRIMARYKEY",
        1811 => "SQLITE_CONSTRAINT_TRIGGER",
        2067 => "SQLITE_CONSTRAINT_UNIQUE",
        2323 => "SQLITE_CONSTRAINT_VTAB",
        2579 => "SQLITE_CONSTRAINT_ROWID",
        3091 => "SQLITE_CONSTRAINT_DATATYPE",
        _ => return None,
    })
}

impl std::fmt::Display for DbError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for DbError {}

impl From<rusqlite::Error> for DbError {
    fn from(e: rusqlite::Error) -> Self {
        let code = match &e {
            rusqlite::Error::SqliteFailure(err, _) => code_name(err.extended_code),
            // prepare 단계 오류(no such table 등)
            rusqlite::Error::SqlInputError { error, .. } => code_name(error.extended_code),
            _ => None,
        };
        DbError(message(&e), code)
    }
}

/// better-sqlite3 의 err.message 와 같은 문구
pub fn message(e: &rusqlite::Error) -> String {
    match e {
        rusqlite::Error::SqliteFailure(err, Some(msg)) => {
            let _ = err;
            msg.clone()
        }
        rusqlite::Error::SqliteFailure(err, None) => err.to_string(),
        rusqlite::Error::SqlInputError { msg, .. } => msg.clone(),
        other => other.to_string(),
    }
}

pub type Result<T> = std::result::Result<T, DbError>;

/// 기동 시 1회: 파일 열기 + journal_mode=WAL + foreign_keys=ON
pub fn open(path: &str) -> Result<()> {
    let conn = Connection::open(path)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    let _ = DB.set(Mutex::new(conn));
    Ok(())
}

/// 연결 잠금. await 를 건너 들고 있지 않는다(컴파일러가 막아 준다: MutexGuard 는 Send 가 아님)
pub fn conn() -> MutexGuard<'static, Connection> {
    DB.get()
        .expect("db not opened")
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// 파라미터 (TS 에서 넘기는 JS 값)
#[derive(Clone, Debug)]
pub enum P {
    Null,
    Num(f64),
    Text(String),
    /// 바인딩할 수 없는 JS 값(boolean·중첩 배열 등) - 개수 검사 뒤 bind 에서 TypeError
    Bad,
}

impl P {
    fn sql(&self) -> Sql {
        match self {
            P::Null => Sql::Null,
            P::Num(f) => Sql::Real(*f),
            P::Text(s) => Sql::Text(s.clone()),
            P::Bad => Sql::Null,
        }
    }
}

impl From<&str> for P {
    fn from(s: &str) -> Self {
        P::Text(s.to_string())
    }
}
impl From<String> for P {
    fn from(s: String) -> Self {
        P::Text(s)
    }
}
impl From<&String> for P {
    fn from(s: &String) -> Self {
        P::Text(s.clone())
    }
}
impl From<i64> for P {
    fn from(n: i64) -> Self {
        P::Num(n as f64)
    }
}
impl From<f64> for P {
    fn from(n: f64) -> Self {
        P::Num(n)
    }
}
impl<T: Into<P>> From<Option<T>> for P {
    fn from(o: Option<T>) -> Self {
        o.map(Into::into).unwrap_or(P::Null)
    }
}

/// JSON 값 → 파라미터. boolean·객체·배열은 better-sqlite3 가 TypeError 를 낸다
pub fn from_json(v: Option<&Value>) -> Result<P> {
    match v {
        None | Some(Value::Null) => Ok(P::Null),
        Some(Value::Number(n)) => Ok(P::Num(js::as_f64(n))),
        Some(Value::String(s)) => Ok(P::Text(s.clone())),
        Some(_) => Err(DbError::msg(BIND_ERROR)),
    }
}

const BIND_ERROR: &str = "SQLite3 can only bind numbers, strings, bigints, buffers, and null";

/// better-sqlite3 의 인자 펼치기: 배열 인자는 원소가 각각 파라미터가 되고(한 단계만),
/// 일반 객체는 이름 붙은 파라미터 묶음이라 익명(?) 자리를 채우지 않는다. 개수가 틀리면 bind 에서
/// 'Too many/few parameter values', 개수는 맞는데 boolean·중첩 배열이면 BIND_ERROR.
pub fn js_arg(v: Option<&Value>) -> Vec<P> {
    fn one(v: &Value) -> P {
        match v {
            Value::Null => P::Null,
            Value::Number(n) => P::Num(js::as_f64(n)),
            Value::String(s) => P::Text(s.clone()),
            _ => P::Bad,
        }
    }
    match v {
        None => vec![P::Null],
        Some(Value::Array(a)) => a.iter().map(one).collect(),
        Some(Value::Object(_)) => vec![],
        Some(x) => vec![one(x)],
    }
}

#[macro_export]
macro_rules! params {
    () => { &[] as &[$crate::core::db::P] };
    ($($x:expr),+ $(,)?) => { &[$($crate::core::db::P::from($x)),+] as &[$crate::core::db::P] };
}

fn bind(stmt: &mut Statement<'_>, params: &[P]) -> Result<()> {
    let expected = stmt.parameter_count();
    if params.len() != expected {
        return Err(DbError::msg(if params.len() > expected {
            "Too many parameter values were provided"
        } else {
            "Too few parameter values were provided"
        }));
    }
    if params.iter().any(|p| matches!(p, P::Bad)) {
        return Err(DbError::msg(BIND_ERROR));
    }
    for (i, p) in params.iter().enumerate() {
        stmt.raw_bind_parameter(i + 1, p.sql())?;
    }
    Ok(())
}

/// 이름 붙은 파라미터(@name) 바인딩
fn bind_named(stmt: &mut Statement<'_>, params: &[(&str, P)]) -> Result<()> {
    for (name, p) in params {
        if let Some(idx) = stmt.parameter_index(name)? {
            stmt.raw_bind_parameter(idx, p.sql())?;
        } else {
            return Err(DbError::msg(format!(
                "Missing named parameter \"{}\"",
                &name[1..]
            )));
        }
    }
    Ok(())
}

fn to_json(v: ValueRef<'_>) -> Value {
    match v {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(i) => Value::from(i),
        ValueRef::Real(f) => js::num(f),
        ValueRef::Text(t) => Value::String(String::from_utf8_lossy(t).into_owned()),
        // Buffer → JSON.stringify 는 {type:'Buffer', data:[...]}
        ValueRef::Blob(b) => serde_json::json!({ "type": "Buffer", "data": b }),
    }
}

fn collect(stmt: &mut Statement<'_>, limit: Option<usize>) -> Result<Vec<Row>> {
    let names: Vec<String> = stmt.column_names().map(|s| s.to_string()).collect();
    let mut rows = stmt.raw_query();
    let mut out = Vec::new();
    while let Some(r) = rows.next()? {
        let mut m = Map::new();
        for (i, n) in names.iter().enumerate() {
            m.insert(n.clone(), to_json(r.get_ref(i)?));
        }
        out.push(m);
        if limit.is_some_and(|l| out.len() >= l) {
            break;
        }
    }
    Ok(out)
}

/// stmt.all(...)
pub fn all(c: &Connection, sql: &str, params: &[P]) -> Result<Vec<Row>> {
    let mut stmt = c.prepare_cached(sql)?;
    bind(&mut stmt, params)?;
    collect(&mut stmt, None)
}

/// stmt.get(...) (첫 행, 없으면 None)
pub fn get(c: &Connection, sql: &str, params: &[P]) -> Result<Option<Row>> {
    let mut stmt = c.prepare_cached(sql)?;
    bind(&mut stmt, params)?;
    Ok(collect(&mut stmt, Some(1))?.into_iter().next())
}

pub fn all_named(c: &Connection, sql: &str, params: &[(&str, P)]) -> Result<Vec<Row>> {
    let mut stmt = c.prepare_cached(sql)?;
    bind_named(&mut stmt, params)?;
    collect(&mut stmt, None)
}

pub struct RunResult {
    pub changes: i64,
    pub last_insert_rowid: i64,
}

/// stmt.run(...)
pub fn run(c: &Connection, sql: &str, params: &[P]) -> Result<RunResult> {
    let mut stmt = c.prepare_cached(sql)?;
    bind(&mut stmt, params)?;
    stmt.raw_execute()?;
    Ok(RunResult {
        changes: c.changes() as i64,
        last_insert_rowid: c.last_insert_rowid(),
    })
}

pub fn run_named(c: &Connection, sql: &str, params: &[(&str, P)]) -> Result<RunResult> {
    let mut stmt = c.prepare_cached(sql)?;
    bind_named(&mut stmt, params)?;
    stmt.raw_execute()?;
    Ok(RunResult {
        changes: c.changes() as i64,
        last_insert_rowid: c.last_insert_rowid(),
    })
}

/// db.exec(sql) - 여러 문장
pub fn exec(c: &Connection, sql: &str) -> Result<()> {
    c.execute_batch(sql)?;
    Ok(())
}

/// db.transaction(fn)() - BEGIN … COMMIT, 실패하면 ROLLBACK 후 오류 그대로
pub fn transaction<T, E: From<DbError>>(
    c: &Connection,
    f: impl FnOnce(&Connection) -> std::result::Result<T, E>,
) -> std::result::Result<T, E> {
    c.execute_batch("BEGIN").map_err(|e| E::from(e.into()))?;
    match f(c) {
        Ok(v) => {
            c.execute_batch("COMMIT").map_err(|e| E::from(e.into()))?;
            Ok(v)
        }
        Err(e) => {
            let _ = c.execute_batch("ROLLBACK");
            Err(e)
        }
    }
}

/// 숫자 컬럼 읽기 헬퍼 (없거나 null 이면 None)
pub fn int(row: &Row, key: &str) -> Option<i64> {
    row.get(key)
        .and_then(|v| v.as_i64().or_else(|| v.as_f64().map(|f| f as i64)))
}

pub fn text<'a>(row: &'a Row, key: &str) -> Option<&'a str> {
    row.get(key).and_then(|v| v.as_str())
}

/// PRAGMA table_info(table) 컬럼 이름 목록
pub fn columns(c: &Connection, table: &str) -> Result<Vec<String>> {
    Ok(all(c, &format!("PRAGMA table_info({table})"), &[])?
        .iter()
        .filter_map(|r| text(r, "name").map(String::from))
        .collect())
}
