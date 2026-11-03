//! Mail backends. Each one syncs into the local database, pushes queued
//! mutations back to the server, sends mail and can re-download raw messages.

pub mod gmail;
pub mod graph;
pub mod imap;

use std::time::Duration;

use anyhow::{Result, bail};
use base64::{Engine as _, engine::general_purpose};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

use crate::{
    config,
    db::{Db, Flags, MessageRecord, MsgRef, ThreadAction, local_id},
    error::AuthExpired,
    mime,
    models::{Account, AccountKind, Draft, SyncStatus},
    oauth::{self, OAuthClient, Tokens},
    secrets::Secrets,
};

/// Each synced folder also gets its newest messages this many, however old,
/// so a mailbox that's been quiet for longer than the sync window isn't empty.
pub const LATEST_PER_FOLDER: usize = 50;

/// A remote mutation waiting in the ops queue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Op {
    pub action: ThreadAction,
    /// Local message ids.
    pub ids: Vec<String>,
}

/// Everything a provider needs to do its job.
#[derive(Clone)]
pub struct Ctx {
    pub app: AppHandle,
    pub db: Db,
    pub secrets: Secrets,
    pub http: reqwest::Client,
    pub account: Account,
}

impl Ctx {
    pub fn emit_changed(&self) {
        let _ = self.app.emit("mail:changed", &self.account.id);
    }

    pub fn emit_progress(&self, done: u32, total: u32) {
        let _ = self.app.emit(
            "sync:status",
            SyncStatus {
                account_id: self.account.id.clone(),
                state: "syncing".into(),
                message: None,
                progress: Some((done, total)),
            },
        );
    }

    /// How far back the first sync reaches.
    pub fn sync_days(&self) -> i64 {
        self.db
            .get_setting("sync_days")
            .ok()
            .flatten()
            .and_then(|v| v.parse().ok())
            .unwrap_or(60)
    }

    pub fn token_key(&self) -> String {
        format!("oauth:{}", self.account.id)
    }

    /// Stores a freshly parsed message. `thread_key` is the provider's thread
    /// id; `None` means "thread it via References" (IMAP).
    #[allow(clippy::too_many_arguments)]
    pub fn ingest(
        &self,
        remote_id: &str,
        thread_key: Option<&str>,
        raw: &[u8],
        flags: Flags,
        labels: Vec<String>,
        folder: Option<String>,
        imap_uid: Option<u32>,
        fallback_date: Option<i64>,
        category_hint: Option<&str>,
    ) -> Result<()> {
        let p = mime::parse(raw)?;
        let acc = &self.account.id;
        let thread_id = match thread_key {
            Some(k) => local_id(acc, k),
            None => {
                let mut lookup: Vec<String> = p.refs.clone();
                if let Some(irt) = &p.in_reply_to {
                    lookup.push(irt.clone());
                }
                if let Some(mid) = &p.message_id {
                    lookup.push(mid.clone());
                }
                match self.db.find_thread_by_message_ids(acc, &lookup)? {
                    Some(t) => t,
                    None => {
                        let root = p
                            .refs
                            .first()
                            .or(p.in_reply_to.as_ref())
                            .or(p.message_id.as_ref())
                            .cloned()
                            .unwrap_or_else(|| remote_id.to_string());
                        local_id(acc, &format!("t:{root}"))
                    }
                }
            }
        };
        let mut category =
            mime::classify(&p.from.email, &labels, p.bulk, p.list_unsubscribe.is_some()).to_string();
        if category == "primary" {
            if let Some(h) = category_hint {
                category = h.to_string();
            }
        }
        let mut flags = flags;
        let own_domain = self.account.email.rsplit('@').next().unwrap_or("");
        let from_own_domain = p.from.email.rsplit('@').next().is_some_and(|d| d.eq_ignore_ascii_case(own_domain));
        // Gmail and Outlook filter spam themselves; plain IMAP servers often
        // only tag it in headers and leave it in INBOX.
        if self.account.kind == crate::models::AccountKind::Imap
            && (p.spam_flagged || (p.auth_failed && from_own_domain))
        {
            flags.spam = true;
        }
        // A forged "From: you" isn't something you sent.
        if p.from.email.eq_ignore_ascii_case(&self.account.email) && !flags.drafts && !flags.spam && !p.auth_failed {
            flags.sent = true;
            flags.read = true;
        }
        self.db.upsert_message(&MessageRecord {
            account_id: acc.clone(),
            remote_id: remote_id.to_string(),
            thread_id,
            message_id: p.message_id,
            in_reply_to: p.in_reply_to,
            refs: p.refs,
            subject: p.subject,
            from: p.from,
            to: p.to,
            cc: p.cc,
            bcc: p.bcc,
            reply_to: p.reply_to,
            date: p.date.or(fallback_date).unwrap_or_else(crate::db::now),
            snippet: p.snippet,
            body_text: p.body_text,
            body_html: p.body_html,
            flags,
            labels,
            folder,
            imap_uid,
            category,
            attachments: p.attachments,
            list_unsubscribe: p.list_unsubscribe,
        })
    }
}

static REFRESH_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Returns a valid access token for an OAuth account, refreshing it if needed.
pub async fn oauth_token(ctx: &Ctx, client: &OAuthClient, force_refresh: bool) -> Result<String> {
    let _guard = REFRESH_LOCK.lock().await;
    let key = ctx.token_key();
    let tokens: Tokens = ctx.secrets.get_json(&key)?.ok_or(AuthExpired)?;
    if tokens.is_fresh() && !force_refresh {
        return Ok(tokens.access_token);
    }
    let rt = tokens.refresh_token.ok_or(AuthExpired)?;
    let fresh = oauth::refresh(&ctx.http, client, &rt).await?;
    ctx.secrets.set_json(&key, &fresh)?;
    Ok(fresh.access_token)
}

#[derive(Debug, thiserror::Error)]
#[error("HTTP {status}: {body}")]
pub struct HttpError {
    pub status: u16,
    pub body: String,
}

pub fn http_status(e: &anyhow::Error) -> Option<u16> {
    e.downcast_ref::<HttpError>().map(|h| h.status)
}

/// Sends an authenticated API request with token refresh and retries for
/// throttling / transient server errors.
pub async fn api(
    ctx: &Ctx,
    client: &OAuthClient,
    make: impl Fn(&reqwest::Client, &str) -> reqwest::RequestBuilder,
) -> Result<reqwest::Response> {
    let mut force = false;
    let mut attempt = 0u32;
    loop {
        attempt += 1;
        let token = oauth_token(ctx, client, force).await?;
        let resp = match make(&ctx.http, &token).send().await {
            Ok(r) => r,
            Err(e) if attempt < 4 => {
                tracing::debug!("request failed, retrying: {e}");
                tokio::time::sleep(Duration::from_millis(500 * 2u64.pow(attempt))).await;
                continue;
            }
            Err(e) => return Err(e.into()),
        };
        let status = resp.status();
        if status.is_success() {
            return Ok(resp);
        }
        if status.as_u16() == 401 {
            if force {
                return Err(AuthExpired.into());
            }
            force = true;
            continue;
        }
        if (status.as_u16() == 429 || status.is_server_error()) && attempt < 5 {
            let wait = resp
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(2u64.pow(attempt));
            tokio::time::sleep(Duration::from_secs(wait.min(60))).await;
            continue;
        }
        let body = resp.text().await.unwrap_or_default();
        bail!(HttpError { status: status.as_u16(), body: body.chars().take(500).collect() });
    }
}

/// Decodes base64url with or without padding (Gmail is inconsistent).
pub fn b64url_decode(s: &str) -> Result<Vec<u8>> {
    let t = s.trim().trim_end_matches('=');
    Ok(general_purpose::URL_SAFE_NO_PAD
        .decode(t)
        .or_else(|_| general_purpose::STANDARD_NO_PAD.decode(t))?)
}

pub enum Provider {
    Gmail(gmail::Gmail),
    Graph(graph::Graph),
    Imap(Box<imap::Imap>),
}

impl Provider {
    pub fn new(ctx: &Ctx) -> Result<Self> {
        Ok(match ctx.account.kind {
            AccountKind::Gmail => Provider::Gmail(gmail::Gmail::new(config::google(&ctx.db)?)),
            AccountKind::Outlook => Provider::Graph(graph::Graph::new(config::microsoft_graph(&ctx.db)?)),
            AccountKind::Imap => Provider::Imap(Box::new(imap::Imap::new(ctx)?)),
        })
    }

    pub async fn sync(&mut self, ctx: &Ctx) -> Result<()> {
        match self {
            Provider::Gmail(p) => p.sync(ctx).await,
            Provider::Graph(p) => p.sync(ctx).await,
            Provider::Imap(p) => p.sync(ctx).await,
        }
    }

    pub async fn apply(&mut self, ctx: &Ctx, op: &Op) -> Result<()> {
        let refs = ctx.db.msg_refs(&op.ids)?;
        if refs.is_empty() {
            return Ok(());
        }
        match self {
            Provider::Gmail(p) => p.apply(ctx, op.action, &refs).await,
            Provider::Graph(p) => p.apply(ctx, op.action, &refs).await,
            Provider::Imap(p) => p.apply(ctx, op.action, &refs).await,
        }
    }

    pub async fn send(&mut self, ctx: &Ctx, draft: &Draft) -> Result<()> {
        match self {
            Provider::Gmail(p) => p.send(ctx, draft).await,
            Provider::Graph(p) => p.send(ctx, draft).await,
            Provider::Imap(p) => p.send(ctx, draft).await,
        }
    }

    pub async fn fetch_raw(&mut self, ctx: &Ctx, r: &MsgRef) -> Result<Vec<u8>> {
        match self {
            Provider::Gmail(p) => p.fetch_raw(ctx, &r.remote_id).await,
            Provider::Graph(p) => p.fetch_raw(ctx, &r.remote_id).await,
            Provider::Imap(p) => p.fetch_raw(ctx, r).await,
        }
    }
}
