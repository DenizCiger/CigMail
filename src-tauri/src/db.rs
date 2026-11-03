//! SQLite storage: the local cache every view reads from. Providers write into
//! it during sync; the UI only ever talks to this database, which is what makes
//! the app feel instant and work offline.

use std::{collections::HashMap, path::Path, sync::Arc};

use anyhow::{Context, Result};
use parking_lot::Mutex;
use rusqlite::{Connection, OptionalExtension, Row, params, params_from_iter, types::Value};
use serde::{Serialize, de::DeserializeOwned};

use crate::models::*;
use crate::query;

const SCHEMA: &str = r#"
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;
PRAGMA synchronous = NORMAL;

CREATE TABLE IF NOT EXISTS accounts (
    id          TEXT PRIMARY KEY,
    kind        TEXT NOT NULL,
    email       TEXT NOT NULL,
    name        TEXT NOT NULL,
    label       TEXT,
    color       TEXT NOT NULL,
    config      TEXT NOT NULL DEFAULT '{}',
    sync_state  TEXT NOT NULL DEFAULT '{}',
    signature   TEXT,
    sort        INTEGER NOT NULL DEFAULT 0,
    created_at  INTEGER NOT NULL,
    last_sync   INTEGER,
    last_error  TEXT
);

CREATE TABLE IF NOT EXISTS messages (
    id               TEXT PRIMARY KEY,
    account_id       TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    remote_id        TEXT NOT NULL,
    thread_id        TEXT NOT NULL,
    message_id       TEXT,
    in_reply_to      TEXT,
    refs             TEXT NOT NULL DEFAULT '[]',
    subject          TEXT NOT NULL DEFAULT '',
    from_name        TEXT,
    from_email       TEXT NOT NULL DEFAULT '',
    to_json          TEXT NOT NULL DEFAULT '[]',
    cc_json          TEXT NOT NULL DEFAULT '[]',
    bcc_json         TEXT NOT NULL DEFAULT '[]',
    reply_to_json    TEXT NOT NULL DEFAULT '[]',
    to_text          TEXT NOT NULL DEFAULT '',
    date             INTEGER NOT NULL,
    snippet          TEXT NOT NULL DEFAULT '',
    body_text        TEXT,
    body_html        TEXT,
    is_read          INTEGER NOT NULL DEFAULT 0,
    is_starred       INTEGER NOT NULL DEFAULT 0,
    in_inbox         INTEGER NOT NULL DEFAULT 0,
    in_sent          INTEGER NOT NULL DEFAULT 0,
    in_drafts        INTEGER NOT NULL DEFAULT 0,
    in_trash         INTEGER NOT NULL DEFAULT 0,
    in_spam          INTEGER NOT NULL DEFAULT 0,
    labels           TEXT NOT NULL DEFAULT '[]',
    folder           TEXT,
    imap_uid         INTEGER,
    category         TEXT NOT NULL DEFAULT 'primary',
    has_attachments  INTEGER NOT NULL DEFAULT 0,
    attachments_json TEXT NOT NULL DEFAULT '[]',
    list_unsubscribe TEXT
);
CREATE INDEX IF NOT EXISTS messages_thread ON messages(thread_id, date);
CREATE INDEX IF NOT EXISTS messages_msgid ON messages(account_id, message_id);
CREATE INDEX IF NOT EXISTS messages_folder ON messages(account_id, folder, imap_uid);
CREATE INDEX IF NOT EXISTS messages_remote ON messages(account_id, remote_id);

CREATE TABLE IF NOT EXISTS threads (
    id              TEXT PRIMARY KEY,
    account_id      TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    subject         TEXT NOT NULL DEFAULT '',
    snippet         TEXT NOT NULL DEFAULT '',
    last_date       INTEGER NOT NULL,
    participants    TEXT NOT NULL DEFAULT '[]',
    message_count   INTEGER NOT NULL DEFAULT 0,
    unread          INTEGER NOT NULL DEFAULT 0,
    starred         INTEGER NOT NULL DEFAULT 0,
    in_inbox        INTEGER NOT NULL DEFAULT 0,
    in_sent         INTEGER NOT NULL DEFAULT 0,
    in_drafts       INTEGER NOT NULL DEFAULT 0,
    in_trash        INTEGER NOT NULL DEFAULT 0,
    in_spam         INTEGER NOT NULL DEFAULT 0,
    has_attachments INTEGER NOT NULL DEFAULT 0,
    category        TEXT NOT NULL DEFAULT 'primary'
);
CREATE INDEX IF NOT EXISTS threads_date ON threads(last_date DESC);
CREATE INDEX IF NOT EXISTS threads_inbox ON threads(in_inbox, last_date DESC);

-- Local-only thread state that must survive resyncs.
CREATE TABLE IF NOT EXISTS thread_meta (
    thread_id      TEXT PRIMARY KEY,
    snoozed_until  INTEGER,
    bumped_at      INTEGER,
    ai_summary     TEXT,
    ai_summary_key TEXT
);

CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts USING fts5(
    subject, from_name, from_email, to_text, body_text,
    content = 'messages', content_rowid = 'rowid',
    tokenize = 'unicode61 remove_diacritics 2'
);
CREATE TRIGGER IF NOT EXISTS messages_ai AFTER INSERT ON messages BEGIN
    INSERT INTO messages_fts(rowid, subject, from_name, from_email, to_text, body_text)
    VALUES (new.rowid, new.subject, new.from_name, new.from_email, new.to_text, new.body_text);
END;
CREATE TRIGGER IF NOT EXISTS messages_ad AFTER DELETE ON messages BEGIN
    INSERT INTO messages_fts(messages_fts, rowid, subject, from_name, from_email, to_text, body_text)
    VALUES ('delete', old.rowid, old.subject, old.from_name, old.from_email, old.to_text, old.body_text);
END;
CREATE TRIGGER IF NOT EXISTS messages_au AFTER UPDATE OF subject, from_name, from_email, to_text, body_text ON messages BEGIN
    INSERT INTO messages_fts(messages_fts, rowid, subject, from_name, from_email, to_text, body_text)
    VALUES ('delete', old.rowid, old.subject, old.from_name, old.from_email, old.to_text, old.body_text);
    INSERT INTO messages_fts(rowid, subject, from_name, from_email, to_text, body_text)
    VALUES (new.rowid, new.subject, new.from_name, new.from_email, new.to_text, new.body_text);
END;

CREATE TABLE IF NOT EXISTS contacts (
    email     TEXT PRIMARY KEY,
    name      TEXT,
    score     INTEGER NOT NULL DEFAULT 0,
    last_seen INTEGER NOT NULL DEFAULT 0
);

-- Remote mutations waiting to be pushed to the provider.
CREATE TABLE IF NOT EXISTS ops (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    payload    TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    attempts   INTEGER NOT NULL DEFAULT 0,
    last_error TEXT
);

-- Messages waiting for their undo-send window to close.
CREATE TABLE IF NOT EXISTS outbox (
    id         TEXT PRIMARY KEY,
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    draft      TEXT NOT NULL,
    send_at    INTEGER NOT NULL,
    status     TEXT NOT NULL DEFAULT 'queued',
    attempts   INTEGER NOT NULL DEFAULT 0,
    last_error TEXT
);

CREATE TABLE IF NOT EXISTS drafts (
    id         TEXT PRIMARY KEY,
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    data       TEXT NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS secrets_fallback (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
"#;

#[derive(Clone)]
pub struct Db(Arc<Mutex<Connection>>);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Flags {
    pub read: bool,
    pub starred: bool,
    pub inbox: bool,
    pub sent: bool,
    pub drafts: bool,
    pub trash: bool,
    pub spam: bool,
}

/// A fully parsed message ready to be stored.
#[derive(Debug, Clone)]
pub struct MessageRecord {
    pub account_id: String,
    pub remote_id: String,
    pub thread_id: String,
    pub message_id: Option<String>,
    pub in_reply_to: Option<String>,
    pub refs: Vec<String>,
    pub subject: String,
    pub from: Address,
    pub to: Vec<Address>,
    pub cc: Vec<Address>,
    pub bcc: Vec<Address>,
    pub reply_to: Vec<Address>,
    pub date: i64,
    pub snippet: String,
    pub body_text: Option<String>,
    pub body_html: Option<String>,
    pub flags: Flags,
    pub labels: Vec<String>,
    pub folder: Option<String>,
    pub imap_uid: Option<u32>,
    pub category: String,
    pub attachments: Vec<AttachmentMeta>,
    pub list_unsubscribe: Option<String>,
}

/// The bits of a stored message a provider needs to mutate it remotely.
#[derive(Debug, Clone)]
pub struct MsgRef {
    #[allow(dead_code)]
    pub id: String,
    pub remote_id: String,
    pub folder: Option<String>,
    pub imap_uid: Option<u32>,
    pub flags: Flags,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ThreadAction {
    Archive,
    Inbox,
    Trash,
    Read,
    Unread,
    Star,
    Unstar,
    Spam,
}

pub fn local_id(account_id: &str, remote_id: &str) -> String {
    format!("{account_id}:{remote_id}")
}

pub fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

fn to_json<T: Serialize>(v: &T) -> String {
    serde_json::to_string(v).unwrap_or_else(|_| "null".into())
}

fn from_json<T: DeserializeOwned + Default>(s: &str) -> T {
    serde_json::from_str(s).unwrap_or_default()
}

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path).with_context(|| format!("opening {}", path.display()))?;
        conn.execute_batch(SCHEMA).context("creating schema")?;
        let mut conn = conn;
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version < 1 {
            // Thread rows from before copies of the same message were counted once.
            let tx = conn.transaction()?;
            let ids: Vec<String> =
                tx.prepare("SELECT id FROM threads")?.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
            for id in ids {
                recompute_thread(&tx, &id)?;
            }
            tx.execute_batch("PRAGMA user_version = 1")?;
            tx.commit()?;
        }
        Ok(Db(Arc::new(Mutex::new(conn))))
    }

    /// Run `f` with the connection locked. Keep closures short: this lock is
    /// shared by every command and sync task.
    pub fn with<R>(&self, f: impl FnOnce(&Connection) -> Result<R>) -> Result<R> {
        let conn = self.0.lock();
        f(&conn)
    }

    pub fn tx<R>(&self, f: impl FnOnce(&rusqlite::Transaction) -> Result<R>) -> Result<R> {
        let mut conn = self.0.lock();
        let tx = conn.transaction()?;
        let r = f(&tx)?;
        tx.commit()?;
        Ok(r)
    }

    // ---------------------------------------------------------------- settings

    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        self.with(|c| {
            Ok(c.query_row("SELECT value FROM settings WHERE key = ?", [key], |r| r.get(0))
                .optional()?)
        })
    }

    pub fn get_setting_json<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
        Ok(self
            .get_setting(key)?
            .and_then(|s| serde_json::from_str(&s).ok()))
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.with(|c| {
            c.execute(
                "INSERT INTO settings(key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                [key, value],
            )?;
            Ok(())
        })
    }

    pub fn all_settings(&self) -> Result<HashMap<String, String>> {
        self.with(|c| {
            let mut st = c.prepare("SELECT key, value FROM settings")?;
            let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
            Ok(rows.collect::<Result<_, _>>()?)
        })
    }

    // ---------------------------------------------------------------- accounts

    fn account_from_row(r: &Row) -> rusqlite::Result<Account> {
        Ok(Account {
            id: r.get("id")?,
            kind: AccountKind::parse(&r.get::<_, String>("kind")?),
            email: r.get("email")?,
            name: r.get("name")?,
            label: r.get("label")?,
            color: r.get("color")?,
            created_at: r.get("created_at")?,
            last_sync: r.get("last_sync")?,
            last_error: r.get("last_error")?,
            signature: r.get("signature")?,
        })
    }

    pub fn accounts(&self) -> Result<Vec<Account>> {
        self.with(|c| {
            let mut st = c.prepare("SELECT * FROM accounts ORDER BY sort, created_at")?;
            let rows = st.query_map([], Self::account_from_row)?;
            Ok(rows.collect::<Result<_, _>>()?)
        })
    }

    pub fn account(&self, id: &str) -> Result<Account> {
        self.with(|c| {
            c.query_row("SELECT * FROM accounts WHERE id = ?", [id], Self::account_from_row)
                .optional()?
                .with_context(|| format!("unknown account {id}"))
        })
    }

    pub fn insert_account(&self, a: &Account, config: &serde_json::Value) -> Result<()> {
        self.with(|c| {
            let sort: i64 =
                c.query_row("SELECT COALESCE(MAX(sort), 0) + 1 FROM accounts", [], |r| r.get(0))?;
            c.execute(
                "INSERT INTO accounts(id, kind, email, name, label, color, config, sort, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    a.id,
                    a.kind.as_str(),
                    a.email,
                    a.name,
                    a.label,
                    a.color,
                    config.to_string(),
                    sort,
                    a.created_at
                ],
            )?;
            Ok(())
        })
    }

    pub fn update_account_profile(
        &self,
        id: &str,
        name: &str,
        label: Option<&str>,
        color: &str,
        signature: Option<&str>,
    ) -> Result<()> {
        self.with(|c| {
            c.execute(
                "UPDATE accounts SET name = ?2, label = ?3, color = ?4, signature = ?5 WHERE id = ?1",
                params![id, name, label, color, signature],
            )?;
            Ok(())
        })
    }

    pub fn delete_account(&self, id: &str) -> Result<()> {
        self.with(|c| {
            c.execute(
                "DELETE FROM thread_meta WHERE thread_id IN (SELECT id FROM threads WHERE account_id = ?)",
                [id],
            )?;
            c.execute("DELETE FROM accounts WHERE id = ?", [id])?;
            // External-content FTS tables need an explicit rebuild after the
            // cascade deleted rows behind their back.
            c.execute("INSERT INTO messages_fts(messages_fts) VALUES ('rebuild')", [])?;
            Ok(())
        })
    }

    pub fn account_config<T: DeserializeOwned>(&self, id: &str) -> Result<T> {
        let s: String = self.with(|c| {
            Ok(c.query_row("SELECT config FROM accounts WHERE id = ?", [id], |r| r.get(0))?)
        })?;
        Ok(serde_json::from_str(&s)?)
    }

    pub fn sync_state<T: DeserializeOwned + Default>(&self, id: &str) -> Result<T> {
        let s: String = self.with(|c| {
            Ok(c.query_row("SELECT sync_state FROM accounts WHERE id = ?", [id], |r| r.get(0))?)
        })?;
        Ok(from_json(&s))
    }

    pub fn set_sync_state<T: Serialize>(&self, id: &str, state: &T) -> Result<()> {
        self.with(|c| {
            c.execute(
                "UPDATE accounts SET sync_state = ?2 WHERE id = ?1",
                params![id, to_json(state)],
            )?;
            Ok(())
        })
    }

    pub fn set_sync_result(&self, id: &str, error: Option<&str>) -> Result<()> {
        self.with(|c| {
            if error.is_none() {
                c.execute(
                    "UPDATE accounts SET last_sync = ?2, last_error = NULL WHERE id = ?1",
                    params![id, now()],
                )?;
            } else {
                c.execute(
                    "UPDATE accounts SET last_error = ?2 WHERE id = ?1",
                    params![id, error],
                )?;
            }
            Ok(())
        })
    }

    /// Drops all cached mail for an account, e.g. when a provider tells us our
    /// sync cursor is no longer valid.
    pub fn clear_account_mail(&self, id: &str) -> Result<()> {
        self.with(|c| {
            c.execute("DELETE FROM messages WHERE account_id = ?", [id])?;
            c.execute("DELETE FROM threads WHERE account_id = ?", [id])?;
            c.execute("INSERT INTO messages_fts(messages_fts) VALUES ('rebuild')", [])?;
            Ok(())
        })
    }

    // ---------------------------------------------------------------- messages

    pub fn message_exists(&self, account_id: &str, remote_id: &str) -> Result<bool> {
        let id = local_id(account_id, remote_id);
        self.with(|c| {
            Ok(c.query_row("SELECT 1 FROM messages WHERE id = ?", [id], |_| Ok(()))
                .optional()?
                .is_some())
        })
    }

    /// Finds the thread an IMAP message belongs to by looking up any of the
    /// Message-IDs it references.
    pub fn find_thread_by_message_ids(&self, account_id: &str, ids: &[String]) -> Result<Option<String>> {
        if ids.is_empty() {
            return Ok(None);
        }
        self.with(|c| {
            let placeholders = vec!["?"; ids.len()].join(",");
            let sql = format!(
                "SELECT thread_id FROM messages WHERE account_id = ? AND message_id IN ({placeholders}) LIMIT 1"
            );
            let mut vals: Vec<Value> = vec![account_id.to_string().into()];
            vals.extend(ids.iter().map(|s| Value::from(s.clone())));
            Ok(c.query_row(&sql, params_from_iter(vals), |r| r.get(0)).optional()?)
        })
    }

    pub fn upsert_message(&self, m: &MessageRecord) -> Result<()> {
        let id = local_id(&m.account_id, &m.remote_id);
        let to_text = m
            .to
            .iter()
            .chain(&m.cc)
            .map(Address::display)
            .collect::<Vec<_>>()
            .join(", ");
        self.tx(|c| {
            // A message can move between threads (e.g. IMAP threading improves
            // as more context arrives), so remember the old one to recompute it.
            let old_thread: Option<String> = c
                .query_row("SELECT thread_id FROM messages WHERE id = ?", [&id], |r| r.get(0))
                .optional()?;
            let is_new = old_thread.is_none();
            c.execute(
                "INSERT INTO messages(
                    id, account_id, remote_id, thread_id, message_id, in_reply_to, refs, subject,
                    from_name, from_email, to_json, cc_json, bcc_json, reply_to_json, to_text, date,
                    snippet, body_text, body_html, is_read, is_starred, in_inbox, in_sent, in_drafts,
                    in_trash, in_spam, labels, folder, imap_uid, category, has_attachments,
                    attachments_json, list_unsubscribe)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,
                         ?22,?23,?24,?25,?26,?27,?28,?29,?30,?31,?32,?33)
                 ON CONFLICT(id) DO UPDATE SET
                    thread_id = excluded.thread_id, message_id = excluded.message_id,
                    in_reply_to = excluded.in_reply_to, refs = excluded.refs, subject = excluded.subject,
                    from_name = excluded.from_name, from_email = excluded.from_email,
                    to_json = excluded.to_json, cc_json = excluded.cc_json, bcc_json = excluded.bcc_json,
                    reply_to_json = excluded.reply_to_json, to_text = excluded.to_text, date = excluded.date,
                    snippet = excluded.snippet, body_text = excluded.body_text, body_html = excluded.body_html,
                    is_read = excluded.is_read, is_starred = excluded.is_starred, in_inbox = excluded.in_inbox,
                    in_sent = excluded.in_sent, in_drafts = excluded.in_drafts, in_trash = excluded.in_trash,
                    in_spam = excluded.in_spam, labels = excluded.labels, folder = excluded.folder,
                    imap_uid = excluded.imap_uid, category = excluded.category,
                    has_attachments = excluded.has_attachments, attachments_json = excluded.attachments_json,
                    list_unsubscribe = excluded.list_unsubscribe",
                params![
                    id,
                    m.account_id,
                    m.remote_id,
                    m.thread_id,
                    m.message_id,
                    m.in_reply_to,
                    to_json(&m.refs),
                    m.subject,
                    m.from.name,
                    m.from.email,
                    to_json(&m.to),
                    to_json(&m.cc),
                    to_json(&m.bcc),
                    to_json(&m.reply_to),
                    to_text,
                    m.date,
                    m.snippet,
                    m.body_text,
                    m.body_html,
                    m.flags.read,
                    m.flags.starred,
                    m.flags.inbox,
                    m.flags.sent,
                    m.flags.drafts,
                    m.flags.trash,
                    m.flags.spam,
                    to_json(&m.labels),
                    m.folder,
                    m.imap_uid,
                    m.category,
                    !m.attachments.iter().all(|a| a.inline),
                    to_json(&m.attachments),
                    m.list_unsubscribe,
                ],
            )?;
            if is_new {
                bump_contacts(c, m)?;
            }
            recompute_thread(c, &m.thread_id)?;
            if let Some(old) = old_thread.filter(|t| *t != m.thread_id) {
                recompute_thread(c, &old)?;
            }
            Ok(())
        })
    }

    pub fn update_flags(
        &self,
        account_id: &str,
        remote_id: &str,
        flags: Flags,
        labels: Option<&[String]>,
        folder: Option<&str>,
    ) -> Result<bool> {
        let id = local_id(account_id, remote_id);
        self.tx(|c| {
            let row: Option<(String, bool)> = c
                .query_row("SELECT thread_id, in_inbox FROM messages WHERE id = ?", [&id], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })
                .optional()?;
            let Some((thread, was_inbox)) = row else { return Ok(false) };
            if flags.inbox && !was_inbox {
                tracing::info!("server moved {id} back into the inbox");
            }
            c.execute(
                "UPDATE messages SET is_read = ?2, is_starred = ?3, in_inbox = ?4, in_sent = ?5,
                    in_drafts = ?6, in_trash = ?7, in_spam = ?8,
                    labels = COALESCE(?9, labels), folder = COALESCE(?10, folder)
                 WHERE id = ?1",
                params![
                    id,
                    flags.read,
                    flags.starred,
                    flags.inbox,
                    flags.sent,
                    flags.drafts,
                    flags.trash,
                    flags.spam,
                    labels.map(|l| to_json(&l)),
                    folder
                ],
            )?;
            recompute_thread(c, &thread)?;
            Ok(true)
        })
    }

    /// Like `update_flags`, but for state read from the server during sync:
    /// messages with a queued local change keep their local flags until that
    /// change has been pushed, so a sync can't briefly undo what the user did.
    pub fn sync_flags(
        &self,
        account_id: &str,
        remote_id: &str,
        flags: Flags,
        labels: Option<&[String]>,
        folder: Option<&str>,
    ) -> Result<bool> {
        let id = local_id(account_id, remote_id);
        let pending: bool = self.with(|c| {
            Ok(c.query_row(
                "SELECT EXISTS(SELECT 1 FROM ops WHERE account_id = ?1 AND instr(payload, ?2) > 0)",
                params![account_id, format!("\"{id}\"")],
                |r| r.get(0),
            )?)
        })?;
        if pending {
            return self.message_exists(account_id, remote_id);
        }
        self.update_flags(account_id, remote_id, flags, labels, folder)
    }

    pub fn delete_message(&self, account_id: &str, remote_id: &str) -> Result<()> {
        let id = local_id(account_id, remote_id);
        self.tx(|c| {
            let thread: Option<String> = c
                .query_row("SELECT thread_id FROM messages WHERE id = ?", [&id], |r| r.get(0))
                .optional()?;
            c.execute("DELETE FROM messages WHERE id = ?", [&id])?;
            if let Some(t) = thread {
                recompute_thread(c, &t)?;
            }
            Ok(())
        })
    }

    pub fn msg_refs(&self, ids: &[String]) -> Result<Vec<MsgRef>> {
        self.with(|c| {
            let mut out = Vec::with_capacity(ids.len());
            let mut st = c.prepare(
                "SELECT id, remote_id, folder, imap_uid, is_read, is_starred, in_inbox, in_sent,
                        in_drafts, in_trash, in_spam
                 FROM messages WHERE id = ?",
            )?;
            for id in ids {
                if let Some(r) = st.query_row([id], msg_ref_from_row).optional()? {
                    out.push(r);
                }
            }
            Ok(out)
        })
    }

    /// Message refs for one IMAP folder, keyed by UID.
    pub fn folder_uids(&self, account_id: &str, folder: &str) -> Result<HashMap<u32, MsgRef>> {
        self.with(|c| {
            let mut st = c.prepare(
                "SELECT id, remote_id, folder, imap_uid, is_read, is_starred, in_inbox, in_sent,
                        in_drafts, in_trash, in_spam
                 FROM messages WHERE account_id = ? AND folder = ? AND imap_uid IS NOT NULL",
            )?;
            let rows = st.query_map([account_id, folder], msg_ref_from_row)?;
            let mut map = HashMap::new();
            for r in rows {
                let r = r?;
                if let Some(uid) = r.imap_uid {
                    map.insert(uid, r);
                }
            }
            Ok(map)
        })
    }

    /// Raw-ish lookup used for attachments and reply composition.
    pub fn message_locator(&self, id: &str) -> Result<(String, MsgRef)> {
        self.with(|c| {
            c.query_row(
                "SELECT id, remote_id, folder, imap_uid, is_read, is_starred, in_inbox, in_sent,
                        in_drafts, in_trash, in_spam, account_id
                 FROM messages WHERE id = ?",
                [id],
                |r| Ok((r.get::<_, String>(11)?, msg_ref_from_row(r)?)),
            )
            .optional()?
            .context("message not found")
        })
    }

    // ---------------------------------------------------------------- threads

    pub fn list_threads(&self, filter: &ThreadFilter) -> Result<Vec<ThreadSummary>> {
        let (where_sql, vals) = filter.to_sql()?;
        let order = if filter.view == "snoozed" {
            "tm.snoozed_until ASC"
        } else {
            "MAX(t.last_date, COALESCE(tm.bumped_at, 0)) DESC"
        };
        let sql = format!(
            "SELECT t.*, tm.snoozed_until, tm.ai_summary FROM threads t
             LEFT JOIN thread_meta tm ON tm.thread_id = t.id
             WHERE {where_sql} ORDER BY {order} LIMIT {} OFFSET {}",
            filter.limit.clamp(1, 2000),
            filter.offset.max(0)
        );
        self.with(|c| {
            let mut st = c.prepare(&sql)?;
            let rows = st.query_map(params_from_iter(vals), thread_from_row)?;
            Ok(rows.collect::<Result<_, _>>()?)
        })
    }

    pub fn count_threads(&self, filter: &ThreadFilter, unread_only: bool) -> Result<i64> {
        let (mut where_sql, vals) = filter.to_sql()?;
        if unread_only {
            where_sql.push_str(" AND t.unread > 0");
        }
        let sql = format!(
            "SELECT COUNT(*) FROM threads t LEFT JOIN thread_meta tm ON tm.thread_id = t.id WHERE {where_sql}"
        );
        self.with(|c| Ok(c.query_row(&sql, params_from_iter(vals), |r| r.get(0))?))
    }

    pub fn thread(&self, id: &str) -> Result<ThreadDetail> {
        self.with(|c| {
            let thread = c
                .query_row(
                    "SELECT t.*, tm.snoozed_until, tm.ai_summary FROM threads t
                     LEFT JOIN thread_meta tm ON tm.thread_id = t.id WHERE t.id = ?",
                    [id],
                    thread_from_row,
                )
                .optional()?
                .context("thread not found")?;
            let mut st = c.prepare("SELECT * FROM messages WHERE thread_id = ? ORDER BY date ASC")?;
            let mut messages: Vec<Message> = Vec::new();
            for m in st.query_map([id], message_from_row)? {
                let m = m?;
                // Mail you send to yourself is stored twice, as the delivered copy
                // and the one filed in Sent. Show it once.
                let dup = m
                    .message_id
                    .as_ref()
                    .and_then(|mid| messages.iter_mut().find(|p| p.message_id.as_ref() == Some(mid)));
                match dup {
                    Some(prev) => {
                        prev.is_read &= m.is_read;
                        prev.is_starred |= m.is_starred;
                        prev.in_sent |= m.in_sent;
                    }
                    None => messages.push(m),
                }
            }
            Ok(ThreadDetail { thread, messages })
        })
    }

    pub fn message(&self, id: &str) -> Result<Message> {
        self.with(|c| {
            c.query_row("SELECT * FROM messages WHERE id = ?", [id], message_from_row)
                .optional()?
                .context("message not found")
        })
    }

    /// Applies a user action to the local cache immediately and returns, per
    /// account, the message ids the provider has to change remotely.
    pub fn apply_thread_action(
        &self,
        thread_ids: &[String],
        action: ThreadAction,
    ) -> Result<Vec<(String, Vec<String>)>> {
        self.tx(|c| {
            let mut by_account: HashMap<String, Vec<String>> = HashMap::new();
            for tid in thread_ids {
                let (cond, set) = match action {
                    ThreadAction::Archive => ("in_inbox = 1", "in_inbox = 0"),
                    ThreadAction::Inbox => (
                        "(in_inbox = 0 OR in_trash = 1 OR in_spam = 1) AND in_sent = 0 AND in_drafts = 0",
                        "in_inbox = 1, in_trash = 0, in_spam = 0",
                    ),
                    ThreadAction::Trash => ("in_trash = 0 AND in_drafts = 0", "in_trash = 1, in_inbox = 0"),
                    ThreadAction::Read => ("is_read = 0", "is_read = 1"),
                    // Unread / star only touch the latest message, like Gmail does.
                    ThreadAction::Unread => (
                        "is_read = 1 AND in_sent = 0 AND id = (SELECT id FROM messages WHERE thread_id = ?1 AND in_sent = 0 ORDER BY date DESC LIMIT 1)",
                        "is_read = 0",
                    ),
                    ThreadAction::Star => (
                        "is_starred = 0 AND id = (SELECT id FROM messages WHERE thread_id = ?1 ORDER BY date DESC LIMIT 1)",
                        "is_starred = 1",
                    ),
                    ThreadAction::Unstar => ("is_starred = 1", "is_starred = 0"),
                    ThreadAction::Spam => ("in_spam = 0 AND in_sent = 0 AND in_drafts = 0", "in_spam = 1, in_inbox = 0"),
                };
                let mut st = c.prepare(&format!(
                    "SELECT id, account_id FROM messages WHERE thread_id = ?1 AND {cond}"
                ))?;
                let rows: Vec<(String, String)> = st
                    .query_map([tid], |r| Ok((r.get(0)?, r.get(1)?)))?
                    .collect::<Result<_, _>>()?;
                if rows.is_empty() {
                    continue;
                }
                c.execute(
                    &format!("UPDATE messages SET {set} WHERE thread_id = ?1 AND {cond}"),
                    [tid],
                )?;
                for (id, acc) in rows {
                    by_account.entry(acc).or_default().push(id);
                }
                if matches!(action, ThreadAction::Archive | ThreadAction::Trash | ThreadAction::Inbox | ThreadAction::Spam) {
                    c.execute("UPDATE thread_meta SET bumped_at = NULL WHERE thread_id = ?", [tid])?;
                }
                recompute_thread(c, tid)?;
            }
            // Queue the remote change in the same transaction, so a sync can
            // never see the local change without its pending op.
            for (acc, ids) in &by_account {
                c.execute(
                    "INSERT INTO ops(account_id, payload, created_at) VALUES (?1, ?2, ?3)",
                    params![acc, serde_json::json!({ "action": action, "ids": ids }).to_string(), now()],
                )?;
            }
            Ok(by_account.into_iter().collect())
        })
    }

    pub fn set_snooze(&self, thread_id: &str, until: Option<i64>) -> Result<()> {
        self.with(|c| {
            c.execute(
                "INSERT INTO thread_meta(thread_id, snoozed_until) VALUES (?1, ?2)
                 ON CONFLICT(thread_id) DO UPDATE SET snoozed_until = excluded.snoozed_until",
                params![thread_id, until],
            )?;
            Ok(())
        })
    }

    /// Snoozes that are due: clears them and bumps the thread to the top.
    pub fn take_due_snoozes(&self) -> Result<Vec<String>> {
        let t = now();
        self.with(|c| {
            let mut st = c.prepare(
                "SELECT thread_id FROM thread_meta WHERE snoozed_until IS NOT NULL AND snoozed_until <= ?",
            )?;
            let ids: Vec<String> = st.query_map([t], |r| r.get(0))?.collect::<Result<_, _>>()?;
            c.execute(
                "UPDATE thread_meta SET snoozed_until = NULL, bumped_at = ?1
                 WHERE snoozed_until IS NOT NULL AND snoozed_until <= ?1",
                [t],
            )?;
            Ok(ids)
        })
    }

    pub fn set_ai_summary(&self, thread_id: &str, summary: &str, key: &str) -> Result<()> {
        self.with(|c| {
            c.execute(
                "INSERT INTO thread_meta(thread_id, ai_summary, ai_summary_key) VALUES (?1, ?2, ?3)
                 ON CONFLICT(thread_id) DO UPDATE SET ai_summary = excluded.ai_summary,
                    ai_summary_key = excluded.ai_summary_key",
                params![thread_id, summary, key],
            )?;
            Ok(())
        })
    }

    pub fn ai_summary(&self, thread_id: &str) -> Result<Option<(String, String)>> {
        self.with(|c| {
            Ok(c.query_row(
                "SELECT ai_summary, ai_summary_key FROM thread_meta
                 WHERE thread_id = ? AND ai_summary IS NOT NULL",
                [thread_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?)
        })
    }

    /// The user's recent sent mail, used as style examples for AI writing.
    pub fn recent_sent_texts(&self, account_id: &str, n: usize) -> Result<Vec<String>> {
        self.with(|c| {
            let mut st = c.prepare(
                "SELECT body_text FROM messages WHERE account_id = ? AND in_sent = 1
                 AND body_text IS NOT NULL ORDER BY date DESC LIMIT ?",
            )?;
            let rows = st.query_map(params![account_id, n as i64], |r| r.get::<_, String>(0))?;
            Ok(rows.collect::<Result<_, _>>()?)
        })
    }

    // ---------------------------------------------------------------- contacts

    pub fn suggest_contacts(&self, q: &str, limit: usize) -> Result<Vec<Contact>> {
        let like = format!("%{}%", q.replace('%', "").replace('_', ""));
        self.with(|c| {
            let mut st = c.prepare(
                "SELECT name, email, score FROM contacts
                 WHERE email LIKE ?1 OR name LIKE ?1
                 ORDER BY score DESC, last_seen DESC LIMIT ?2",
            )?;
            let rows = st.query_map(params![like, limit as i64], |r| {
                Ok(Contact { name: r.get(0)?, email: r.get(1)?, count: r.get(2)? })
            })?;
            Ok(rows.collect::<Result<_, _>>()?)
        })
    }

    // ---------------------------------------------------------------- ops queue

    #[cfg(test)]
    pub fn push_op<T: Serialize>(&self, account_id: &str, op: &T) -> Result<()> {
        self.with(|c| {
            c.execute(
                "INSERT INTO ops(account_id, payload, created_at) VALUES (?1, ?2, ?3)",
                params![account_id, to_json(op), now()],
            )?;
            Ok(())
        })
    }

    pub fn pending_ops<T: DeserializeOwned>(&self, account_id: &str) -> Result<Vec<(i64, T, i64)>> {
        self.with(|c| {
            let mut st = c.prepare(
                "SELECT id, payload, attempts FROM ops WHERE account_id = ? ORDER BY id LIMIT 200",
            )?;
            let rows = st.query_map([account_id], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?))
            })?;
            let mut out = Vec::new();
            for row in rows {
                let (id, payload, attempts) = row?;
                match serde_json::from_str(&payload) {
                    Ok(op) => out.push((id, op, attempts)),
                    Err(_) => {
                        c.execute("DELETE FROM ops WHERE id = ?", [id])?;
                    }
                }
            }
            Ok(out)
        })
    }

    pub fn finish_op(&self, id: i64) -> Result<()> {
        self.with(|c| {
            c.execute("DELETE FROM ops WHERE id = ?", [id])?;
            Ok(())
        })
    }

    pub fn fail_op(&self, id: i64, err: &str) -> Result<()> {
        self.with(|c| {
            c.execute(
                "UPDATE ops SET attempts = attempts + 1, last_error = ?2 WHERE id = ?1",
                params![id, err],
            )?;
            Ok(())
        })
    }

    // ---------------------------------------------------------------- outbox

    pub fn queue_outgoing(&self, id: &str, draft: &Draft, send_at: i64) -> Result<()> {
        self.with(|c| {
            c.execute(
                "INSERT INTO outbox(id, account_id, draft, send_at) VALUES (?1, ?2, ?3, ?4)",
                params![id, draft.account_id, to_json(draft), send_at],
            )?;
            Ok(())
        })
    }

    /// Atomically claims due outbox entries so they can't be cancelled while sending.
    pub fn claim_due_outgoing(&self) -> Result<Vec<(String, Draft)>> {
        self.tx(|c| {
            let mut st = c.prepare(
                "SELECT id, draft FROM outbox WHERE status = 'queued' AND send_at <= ?",
            )?;
            let rows: Vec<(String, String)> = st
                .query_map([now()], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<Result<_, _>>()?;
            let mut out = Vec::new();
            for (id, d) in rows {
                c.execute("UPDATE outbox SET status = 'sending' WHERE id = ?", [&id])?;
                if let Ok(d) = serde_json::from_str(&d) {
                    out.push((id, d));
                }
            }
            Ok(out)
        })
    }

    pub fn cancel_outgoing(&self, id: &str) -> Result<Option<Draft>> {
        self.tx(|c| {
            let d: Option<String> = c
                .query_row(
                    "SELECT draft FROM outbox WHERE id = ? AND status IN ('queued', 'failed')",
                    [id],
                    |r| r.get(0),
                )
                .optional()?;
            if d.is_some() {
                c.execute("DELETE FROM outbox WHERE id = ?", [id])?;
            }
            Ok(d.and_then(|d| serde_json::from_str(&d).ok()))
        })
    }

    pub fn finish_outgoing(&self, id: &str, error: Option<&str>) -> Result<()> {
        self.with(|c| {
            match error {
                None => c.execute("DELETE FROM outbox WHERE id = ?", [id])?,
                Some(e) => c.execute(
                    "UPDATE outbox SET status = 'failed', attempts = attempts + 1, last_error = ?2 WHERE id = ?1",
                    params![id, e],
                )?,
            };
            Ok(())
        })
    }

    pub fn reset_stuck_outgoing(&self) -> Result<()> {
        self.with(|c| {
            c.execute("UPDATE outbox SET status = 'queued' WHERE status = 'sending'", [])?;
            Ok(())
        })
    }

    pub fn failed_outgoing(&self) -> Result<Vec<(String, Draft, String)>> {
        self.with(|c| {
            let mut st = c.prepare(
                "SELECT id, draft, COALESCE(last_error, '') FROM outbox WHERE status = 'failed'",
            )?;
            let rows = st.query_map([], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
            })?;
            let mut out = Vec::new();
            for row in rows {
                let (id, d, e) = row?;
                if let Ok(d) = serde_json::from_str(&d) {
                    out.push((id, d, e));
                }
            }
            Ok(out)
        })
    }

    // ---------------------------------------------------------------- drafts

    pub fn save_draft(&self, d: &Draft) -> Result<String> {
        let id = d.id.clone().unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let mut d = d.clone();
        d.id = Some(id.clone());
        self.with(|c| {
            c.execute(
                "INSERT INTO drafts(id, account_id, data, updated_at) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(id) DO UPDATE SET account_id = excluded.account_id,
                    data = excluded.data, updated_at = excluded.updated_at",
                params![id, d.account_id, to_json(&d), now()],
            )?;
            Ok(())
        })?;
        Ok(id)
    }

    pub fn drafts(&self) -> Result<Vec<Draft>> {
        self.with(|c| {
            let mut st = c.prepare("SELECT data FROM drafts ORDER BY updated_at DESC")?;
            let rows = st.query_map([], |r| r.get::<_, String>(0))?;
            Ok(rows
                .filter_map(|r| r.ok().and_then(|s| serde_json::from_str(&s).ok()))
                .collect())
        })
    }

    pub fn delete_draft(&self, id: &str) -> Result<()> {
        self.with(|c| {
            c.execute("DELETE FROM drafts WHERE id = ?", [id])?;
            Ok(())
        })
    }

    // ---------------------------------------------------------------- secrets

    pub fn fallback_secret(&self, key: &str) -> Result<Option<String>> {
        self.with(|c| {
            Ok(c.query_row("SELECT value FROM secrets_fallback WHERE key = ?", [key], |r| r.get(0))
                .optional()?)
        })
    }

    pub fn set_fallback_secret(&self, key: &str, value: Option<&str>) -> Result<()> {
        self.with(|c| {
            match value {
                Some(v) => c.execute(
                    "INSERT INTO secrets_fallback(key, value) VALUES (?1, ?2)
                     ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                    [key, v],
                )?,
                None => c.execute("DELETE FROM secrets_fallback WHERE key = ?", [key])?,
            };
            Ok(())
        })
    }
}

/// Which threads a list shows: a view ("inbox", "starred", ...), an optional
/// account and an optional search/split query.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadFilter {
    pub view: String,
    #[serde(default)]
    pub account_id: Option<String>,
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
}

fn default_limit() -> i64 {
    300
}

impl ThreadFilter {
    fn to_sql(&self) -> Result<(String, Vec<Value>)> {
        let now = now();
        let mut conds: Vec<String> = Vec::new();
        let mut vals: Vec<Value> = Vec::new();
        let not_snoozed = format!("(tm.snoozed_until IS NULL OR tm.snoozed_until <= {now})");
        match self.view.as_str() {
            "inbox" => conds.push(format!("t.in_inbox = 1 AND t.in_trash = 0 AND t.in_spam = 0 AND {not_snoozed}")),
            "starred" => conds.push("t.starred = 1 AND t.in_trash = 0".into()),
            "sent" => conds.push("t.in_sent = 1 AND t.in_trash = 0".into()),
            "drafts" => conds.push("t.in_drafts = 1 AND t.in_trash = 0".into()),
            "done" => conds.push(format!(
                "t.in_inbox = 0 AND t.in_trash = 0 AND t.in_spam = 0 AND {not_snoozed}"
            )),
            "snoozed" => conds.push(format!("tm.snoozed_until > {now}")),
            "trash" => conds.push("t.in_trash = 1".into()),
            "spam" => conds.push("t.in_spam = 1".into()),
            // "all" / "search": everything except trash and spam
            _ => conds.push("t.in_trash = 0 AND t.in_spam = 0".into()),
        }
        if let Some(acc) = &self.account_id {
            conds.push("t.account_id = ?".into());
            vals.push(acc.clone().into());
        }
        if let Some(q) = self.query.as_deref().filter(|q| !q.trim().is_empty()) {
            let (sql, qvals) = query::to_sql(&query::parse(q));
            if !sql.is_empty() {
                conds.push(sql);
                vals.extend(qvals);
            }
        }
        Ok((conds.join(" AND "), vals))
    }
}

fn msg_ref_from_row(r: &Row) -> rusqlite::Result<MsgRef> {
    Ok(MsgRef {
        id: r.get(0)?,
        remote_id: r.get(1)?,
        folder: r.get(2)?,
        imap_uid: r.get(3)?,
        flags: Flags {
            read: r.get(4)?,
            starred: r.get(5)?,
            inbox: r.get(6)?,
            sent: r.get(7)?,
            drafts: r.get(8)?,
            trash: r.get(9)?,
            spam: r.get(10)?,
        },
    })
}

fn thread_from_row(r: &Row) -> rusqlite::Result<ThreadSummary> {
    Ok(ThreadSummary {
        id: r.get("id")?,
        account_id: r.get("account_id")?,
        subject: r.get("subject")?,
        snippet: crate::mime::clean_snippet(&r.get::<_, String>("snippet")?),
        last_date: r.get("last_date")?,
        participants: from_json(&r.get::<_, String>("participants")?),
        message_count: r.get("message_count")?,
        unread: r.get::<_, i64>("unread")? > 0,
        starred: r.get("starred")?,
        in_inbox: r.get("in_inbox")?,
        has_attachments: r.get("has_attachments")?,
        category: r.get("category")?,
        snoozed_until: r.get("snoozed_until")?,
        ai_summary: r.get("ai_summary")?,
    })
}

fn message_from_row(r: &Row) -> rusqlite::Result<Message> {
    Ok(Message {
        id: r.get("id")?,
        account_id: r.get("account_id")?,
        thread_id: r.get("thread_id")?,
        message_id: r.get("message_id")?,
        references: from_json(&r.get::<_, String>("refs")?),
        subject: r.get("subject")?,
        from: Address { name: r.get("from_name")?, email: r.get("from_email")? },
        to: from_json(&r.get::<_, String>("to_json")?),
        cc: from_json(&r.get::<_, String>("cc_json")?),
        bcc: from_json(&r.get::<_, String>("bcc_json")?),
        reply_to: from_json(&r.get::<_, String>("reply_to_json")?),
        date: r.get("date")?,
        snippet: crate::mime::clean_snippet(&r.get::<_, String>("snippet")?),
        body_html: r.get("body_html")?,
        body_text: r.get("body_text")?,
        is_read: r.get("is_read")?,
        is_starred: r.get("is_starred")?,
        is_draft: r.get("in_drafts")?,
        in_sent: r.get("in_sent")?,
        attachments: from_json(&r.get::<_, String>("attachments_json")?),
        list_unsubscribe: r.get("list_unsubscribe")?,
    })
}

fn bump_contacts(c: &Connection, m: &MessageRecord) -> Result<()> {
    // People you write to matter more than people who write to you.
    let mut entries: Vec<(&Address, i64)> = Vec::new();
    if m.flags.sent {
        entries.extend(m.to.iter().chain(&m.cc).chain(&m.bcc).map(|a| (a, 5)));
    } else if m.category == "primary" {
        entries.push((&m.from, 1));
    }
    let mut st = c.prepare_cached(
        "INSERT INTO contacts(email, name, score, last_seen) VALUES (lower(?1), ?2, ?3, ?4)
         ON CONFLICT(email) DO UPDATE SET score = score + excluded.score,
            name = COALESCE(excluded.name, name), last_seen = MAX(last_seen, excluded.last_seen)",
    )?;
    for (a, w) in entries {
        if a.email.is_empty() {
            continue;
        }
        st.execute(params![a.email, a.name.as_deref().filter(|n| !n.is_empty()), w, m.date])?;
    }
    Ok(())
}

/// Rebuilds the denormalized `threads` row from its messages.
fn recompute_thread(c: &Connection, thread_id: &str) -> Result<()> {
    struct M {
        account_id: String,
        message_id: Option<String>,
        subject: String,
        snippet: String,
        date: i64,
        from: Address,
        read: bool,
        starred: bool,
        inbox: bool,
        sent: bool,
        drafts: bool,
        trash: bool,
        spam: bool,
        attachments: bool,
        category: String,
    }
    let mut st = c.prepare_cached(
        "SELECT account_id, subject, snippet, date, from_name, from_email, is_read, is_starred,
                in_inbox, in_sent, in_drafts, in_trash, in_spam, has_attachments, category, message_id
         FROM messages WHERE thread_id = ? ORDER BY date ASC",
    )?;
    let msgs: Vec<M> = st
        .query_map([thread_id], |r| {
            Ok(M {
                account_id: r.get(0)?,
                subject: r.get(1)?,
                snippet: r.get(2)?,
                date: r.get(3)?,
                from: Address { name: r.get(4)?, email: r.get(5)? },
                read: r.get(6)?,
                starred: r.get(7)?,
                inbox: r.get(8)?,
                sent: r.get(9)?,
                drafts: r.get(10)?,
                trash: r.get(11)?,
                spam: r.get(12)?,
                attachments: r.get(13)?,
                category: r.get(14)?,
                message_id: r.get(15)?,
            })
        })?
        .collect::<Result<_, _>>()?;

    if msgs.is_empty() {
        c.execute("DELETE FROM threads WHERE id = ?", [thread_id])?;
        return Ok(());
    }

    let visible: Vec<&M> = {
        let v: Vec<&M> = msgs.iter().filter(|m| !m.trash).collect();
        if v.is_empty() { msgs.iter().collect() } else { v }
    };
    let last = visible.last().unwrap();
    let subject = msgs
        .iter()
        .map(|m| m.subject.as_str())
        .find(|s| !s.is_empty())
        .unwrap_or("")
        .to_string();
    let mut participants: Vec<Address> = Vec::new();
    for m in &visible {
        if !participants.iter().any(|p| p.email.eq_ignore_ascii_case(&m.from.email)) {
            participants.push(m.from.clone());
        }
    }
    // Most recent senders are the most relevant ones to show.
    if participants.len() > 4 {
        participants.drain(..participants.len() - 4);
    }
    // Copies of the same message (sent to yourself) count once.
    let mut ids = std::collections::HashSet::new();
    let message_count = visible
        .iter()
        .filter(|m| m.message_id.as_ref().is_none_or(|id| ids.insert(id.as_str())))
        .count() as i64;
    let unread = visible.iter().filter(|m| !m.read && !m.sent).count() as i64;
    // If you've replied, the conversation is important regardless of what the
    // first message looked like.
    let category = if msgs.iter().any(|m| m.sent) {
        "primary".to_string()
    } else {
        visible
            .iter()
            .rev()
            .find(|m| !m.sent)
            .map(|m| m.category.clone())
            .unwrap_or_else(|| "primary".into())
    };

    c.prepare_cached(
        "INSERT INTO threads(id, account_id, subject, snippet, last_date, participants, message_count,
            unread, starred, in_inbox, in_sent, in_drafts, in_trash, in_spam, has_attachments, category)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)
         ON CONFLICT(id) DO UPDATE SET subject = excluded.subject, snippet = excluded.snippet,
            last_date = excluded.last_date, participants = excluded.participants,
            message_count = excluded.message_count, unread = excluded.unread,
            starred = excluded.starred, in_inbox = excluded.in_inbox, in_sent = excluded.in_sent,
            in_drafts = excluded.in_drafts, in_trash = excluded.in_trash, in_spam = excluded.in_spam,
            has_attachments = excluded.has_attachments, category = excluded.category",
    )?
    .execute(params![
        thread_id,
        last.account_id,
        subject,
        last.snippet,
        last.date,
        to_json(&participants),
        message_count,
        unread,
        msgs.iter().any(|m| m.starred && !m.trash),
        msgs.iter().any(|m| m.inbox && !m.trash),
        msgs.iter().any(|m| m.sent),
        msgs.iter().any(|m| m.drafts),
        msgs.iter().all(|m| m.trash),
        msgs.iter().all(|m| m.spam),
        msgs.iter().any(|m| m.attachments),
        category,
    ])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(remote: &str, thread: &str, from: &str, subject: &str, body: &str, date: i64, category: &str) -> MessageRecord {
        MessageRecord {
            account_id: "a1".into(),
            remote_id: remote.into(),
            thread_id: local_id("a1", thread),
            message_id: Some(format!("{remote}@x")),
            in_reply_to: None,
            refs: vec![],
            subject: subject.into(),
            from: Address { name: Some("Ann".into()), email: from.into() },
            to: vec![Address { name: None, email: "me@x.com".into() }],
            cc: vec![],
            bcc: vec![],
            reply_to: vec![],
            date,
            snippet: body.into(),
            body_text: Some(body.into()),
            body_html: None,
            flags: Flags { inbox: true, ..Default::default() },
            labels: vec![],
            folder: None,
            imap_uid: None,
            category: category.into(),
            attachments: vec![],
            list_unsubscribe: None,
        }
    }

    fn filter(view: &str, query: Option<&str>) -> ThreadFilter {
        ThreadFilter { view: view.into(), account_id: None, query: query.map(String::from), limit: 100, offset: 0 }
    }

    #[test]
    fn ingest_thread_search_and_actions() {
        let dir = std::env::temp_dir().join(format!("cigmail-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = Db::open(&dir.join("t.db")).unwrap();
        db.insert_account(
            &Account {
                id: "a1".into(),
                kind: AccountKind::Imap,
                email: "me@x.com".into(),
                name: "Me".into(),
                label: Some("School".into()),
                color: "#fff".into(),
                created_at: 0,
                last_sync: None,
                last_error: None,
                signature: None,
            },
            &serde_json::json!({}),
        )
        .unwrap();

        db.upsert_message(&record("m1", "t1", "ann@x.com", "Exam dates", "The exam is on Friday", 100, "primary")).unwrap();
        db.upsert_message(&record("m2", "t1", "bob@x.com", "Re: Exam dates", "Thanks!", 200, "primary")).unwrap();
        db.upsert_message(&record("m3", "t2", "news@shop.com", "Big sale", "Everything 50% off", 150, "newsletter")).unwrap();

        let inbox = db.list_threads(&filter("inbox", None)).unwrap();
        assert_eq!(inbox.len(), 2);
        assert_eq!(inbox[0].id, "a1:t1", "newest thread first");
        assert_eq!(inbox[0].message_count, 2);
        assert_eq!(inbox[0].subject, "Exam dates");
        assert!(inbox[0].unread);

        assert_eq!(db.list_threads(&filter("inbox", Some("category:primary"))).unwrap().len(), 1);
        assert_eq!(db.list_threads(&filter("inbox", Some("-category:primary"))).unwrap().len(), 1);
        assert_eq!(db.list_threads(&filter("search", Some("exa"))).unwrap()[0].id, "a1:t1");
        assert_eq!(db.list_threads(&filter("search", Some("from:shop"))).unwrap()[0].id, "a1:t2");
        assert_eq!(db.list_threads(&filter("search", Some("account:school sale"))).unwrap().len(), 1);

        let ops = db.apply_thread_action(&["a1:t1".into()], ThreadAction::Archive).unwrap();
        assert_eq!(ops[0].1.len(), 2);
        assert_eq!(db.list_threads(&filter("inbox", None)).unwrap().len(), 1);
        assert_eq!(db.list_threads(&filter("done", None)).unwrap().len(), 1);

        db.apply_thread_action(&["a1:t2".into()], ThreadAction::Read).unwrap();
        assert_eq!(db.count_threads(&filter("inbox", None), true).unwrap(), 0);

        db.set_snooze("a1:t2", Some(now() + 3600)).unwrap();
        assert_eq!(db.list_threads(&filter("inbox", None)).unwrap().len(), 0);
        assert_eq!(db.list_threads(&filter("snoozed", None)).unwrap().len(), 1);

        // A sync must not undo a change that's still queued for the server.
        db.push_op("a1", &serde_json::json!({ "action": "archive", "ids": ["a1:m1"] })).unwrap();
        let mut server = Flags { inbox: true, read: true, ..Default::default() };
        assert!(db.sync_flags("a1", "m1", server, None, None).unwrap());
        assert_eq!(db.list_threads(&filter("inbox", None)).unwrap().iter().filter(|t| t.id == "a1:t1").count(), 0);
        let (op_id, _, _) = db.pending_ops::<serde_json::Value>("a1").unwrap()[0].clone();
        db.finish_op(op_id).unwrap();
        server.inbox = false;
        db.sync_flags("a1", "m1", server, None, None).unwrap();

        db.delete_message("a1", "m3").unwrap();
        assert!(db.list_threads(&filter("snoozed", None)).unwrap().is_empty());
        assert_eq!(db.suggest_contacts("ann@", 5).unwrap()[0].email, "ann@x.com");

        // Mail to yourself: the delivered copy and the Sent copy show as one.
        let mut delivered = record("INBOX/1", "t3", "me@x.com", "Note", "hi", 300, "primary");
        delivered.message_id = Some("self@x".into());
        let mut filed = record("Sent/1", "t3", "me@x.com", "Note", "hi", 300, "primary");
        filed.message_id = Some("self@x".into());
        filed.flags = Flags { sent: true, read: true, ..Default::default() };
        db.upsert_message(&delivered).unwrap();
        db.upsert_message(&filed).unwrap();
        let t = db.thread("a1:t3").unwrap();
        assert_eq!(t.messages.len(), 1);
        assert!(t.messages[0].in_sent);
        assert_eq!(t.thread.message_count, 1);
        std::fs::remove_dir_all(dir).ok();
    }
}
