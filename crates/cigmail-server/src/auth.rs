//! Who may talk to the server, in `server.db`.
//!
//! One password protects everything. It is stored as an Argon2id hash. The app trades it for a
//! token when it pairs, an assistant gets tokens when the user signs it in (`oauth`); both are
//! rows in `devices`, and only the SHA-256 of a token is kept.

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use argon2::Argon2;
use argon2::password_hash::rand_core::{OsRng, RngCore};
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use sha2::{Digest, Sha256};

pub const MIN_PASSWORD_LEN: usize = 10;

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
-- Paired apps ('app') and connected assistants ('assistant').
CREATE TABLE IF NOT EXISTS devices (
    id           TEXT PRIMARY KEY,
    kind         TEXT NOT NULL,
    name         TEXT NOT NULL,
    -- An assistant's is never handed out: assistants only hold OAuth tokens.
    token_hash   TEXT NOT NULL UNIQUE,
    created_at   INTEGER NOT NULL,
    last_seen_at INTEGER,
    revoked_at   INTEGER
);
"#;

pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

pub fn open(path: &std::path::Path) -> rusqlite::Result<Connection> {
    let conn = Connection::open(path)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.execute_batch(SCHEMA)?;
    Ok(conn)
}

pub fn hash_password(password: &str) -> String {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default().hash_password(password.as_bytes(), &salt).expect("argon2 with default parameters").to_string()
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    PasswordHash::new(hash).is_ok_and(|h| Argon2::default().verify_password(password.as_bytes(), &h).is_ok())
}

pub fn password_hash(conn: &Connection) -> rusqlite::Result<Option<String>> {
    conn.query_row("SELECT value FROM meta WHERE key = 'password_hash'", [], |r| r.get(0)).optional()
}

pub fn set_password(conn: &Connection, password: &str) -> anyhow::Result<()> {
    if password.chars().count() < MIN_PASSWORD_LEN {
        anyhow::bail!("the password needs at least {MIN_PASSWORD_LEN} characters: it is all that stands between the internet and your mail");
    }
    // Starting again with the same CIGMAIL_PASSWORD shouldn't rewrite the hash every time.
    if password_hash(conn)?.is_some_and(|h| verify_password(password, &h)) {
        return Ok(());
    }
    conn.execute(
        "INSERT INTO meta (key, value) VALUES ('password_hash', ?1) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        [hash_password(password)],
    )?;
    Ok(())
}

/// Whether `password` is the server's. False while none is set.
pub fn check_password(conn: &Connection, password: &str) -> rusqlite::Result<bool> {
    Ok(match password_hash(conn)? {
        Some(hash) => verify_password(password, &hash),
        None => false,
    })
}

pub fn new_token() -> String {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

pub fn token_hash(token: &str) -> String {
    Sha256::digest(token.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

/// Registers an app or an assistant and returns (device id, token).
pub fn add_device(conn: &Connection, kind: &str, name: &str) -> rusqlite::Result<(String, String)> {
    let id = uuid::Uuid::new_v4().simple().to_string();
    let token = new_token();
    let name: String = name.trim().chars().filter(|c| !c.is_control()).take(80).collect();
    let name = if name.is_empty() { "Unnamed".to_string() } else { name };
    conn.execute(
        "INSERT INTO devices (id, kind, name, token_hash, created_at, last_seen_at) VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
        params![id, kind, name, token_hash(&token), now_ms()],
    )?;
    Ok((id, token))
}

/// The paired app a token belongs to, if it's still valid. Marks it as seen.
pub fn app_by_token(conn: &Connection, token: &str) -> rusqlite::Result<Option<String>> {
    let found: Option<(String, Option<i64>)> = conn
        .query_row(
            "SELECT id, last_seen_at FROM devices WHERE token_hash = ?1 AND kind = 'app' AND revoked_at IS NULL",
            [token_hash(token)],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let Some((id, seen)) = found else { return Ok(None) };
    let now = now_ms();
    if seen.is_none_or(|s| now - s > 60_000) {
        conn.execute("UPDATE devices SET last_seen_at = ?2 WHERE id = ?1", params![id, now])?;
    }
    Ok(Some(id))
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    pub id: String,
    /// "app" or "assistant".
    pub kind: String,
    pub name: String,
    pub created_at: i64,
    pub last_seen_at: Option<i64>,
    /// Only for assistants: whether it may change things or only read.
    pub can_write: Option<bool>,
}

/// Revokes an app or assistant; false if there's no such one.
pub fn revoke_device(conn: &Connection, id: &str) -> rusqlite::Result<bool> {
    let n = conn.execute("UPDATE devices SET revoked_at = ?2 WHERE id = ?1 AND revoked_at IS NULL", params![id, now_ms()])?;
    Ok(n > 0)
}

/// Sliding-window counters for what anyone can reach.
#[derive(Default)]
pub struct Limiter {
    hits: HashMap<String, VecDeque<Instant>>,
}

impl Limiter {
    /// Whether `key` had fewer than `max` hits within `window`.
    pub fn allowed(&mut self, key: &str, max: usize, window: Duration) -> bool {
        let now = Instant::now();
        let hits = self.hits.entry(key.to_string()).or_default();
        while hits.front().is_some_and(|t| now.duration_since(*t) > window) {
            hits.pop_front();
        }
        hits.len() < max
    }

    pub fn hit(&mut self, key: &str) {
        self.hits.entry(key.to_string()).or_default().push_back(Instant::now());
        // Keep memory bounded under a flood of distinct keys.
        if self.hits.len() > 50_000 {
            self.hits.retain(|_, v| v.back().is_some_and(|t| t.elapsed() < Duration::from_secs(3600)));
        }
    }
}

pub const LOGIN_WINDOW: Duration = Duration::from_secs(15 * 60);
const MAX_FAILED_PER_IP: usize = 10;
/// One password for everything means guesses from many addresses add up.
const MAX_FAILED_OVERALL: usize = 40;

/// Checks the password for a caller at `ip`, counting failures. `Err` = too many failures lately.
pub fn sign_in(conn: &Connection, limiter: &parking_lot::Mutex<Limiter>, ip: &str, password: &str) -> rusqlite::Result<Result<bool, ()>> {
    let ip_key = format!("login-ip:{ip}");
    {
        let mut l = limiter.lock();
        if !l.allowed(&ip_key, MAX_FAILED_PER_IP, LOGIN_WINDOW) || !l.allowed("login", MAX_FAILED_OVERALL, LOGIN_WINDOW) {
            return Ok(Err(()));
        }
    }
    let ok = check_password(conn, password)?;
    if !ok {
        let mut l = limiter.lock();
        l.hit(&ip_key);
        l.hit("login");
    }
    Ok(Ok(ok))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_and_limits() {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(SCHEMA).unwrap();
        let limiter = parking_lot::Mutex::new(Limiter::default());
        assert_eq!(sign_in(&db, &limiter, "1.2.3.4", "anything").unwrap(), Ok(false), "no password set: nobody gets in");
        assert!(set_password(&db, "short").is_err());
        set_password(&db, "correct horse battery").unwrap();
        let hash = password_hash(&db).unwrap().unwrap();
        assert!(hash.starts_with("$argon2id$"));
        set_password(&db, "correct horse battery").unwrap();
        assert_eq!(password_hash(&db).unwrap().unwrap(), hash, "unchanged");
        assert_eq!(sign_in(&db, &limiter, "1.2.3.4", "correct horse battery").unwrap(), Ok(true));
        for _ in 0..MAX_FAILED_PER_IP - 1 {
            assert_eq!(sign_in(&db, &limiter, "1.2.3.4", "wrong").unwrap(), Ok(false));
        }
        assert_eq!(sign_in(&db, &limiter, "1.2.3.4", "correct horse battery").unwrap(), Err(()), "locked out, right or wrong");
        assert_eq!(sign_in(&db, &limiter, "5.6.7.8", "correct horse battery").unwrap(), Ok(true));
    }

    #[test]
    fn devices() {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(SCHEMA).unwrap();
        let (id, token) = add_device(&db, "app", " Laptop ").unwrap();
        let (_, assistant) = add_device(&db, "assistant", "Claude").unwrap();
        assert_eq!(app_by_token(&db, &token).unwrap().as_deref(), Some(id.as_str()));
        assert_eq!(app_by_token(&db, &assistant).unwrap(), None, "an assistant is no app");
        assert!(revoke_device(&db, &id).unwrap());
        assert!(!revoke_device(&db, &id).unwrap());
        assert_eq!(app_by_token(&db, &token).unwrap(), None);
    }
}
