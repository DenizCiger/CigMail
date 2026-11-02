//! Background workers: one sync loop per account, plus the outbox (undo send)
//! and snooze loops.

use std::{collections::HashMap, sync::Arc, time::Duration};

use anyhow::Result;
use parking_lot::Mutex;
use tauri::{AppHandle, Emitter};
use tauri_plugin_notification::NotificationExt;
use tokio::sync::Notify;

use crate::{
    db::{Db, ThreadAction},
    error::AuthExpired,
    models::{AccountKind, SyncStatus},
    providers::{Ctx, Op, Provider},
    secrets::Secrets,
};

struct Worker {
    wake: Arc<Notify>,
    handle: tauri::async_runtime::JoinHandle<()>,
}

pub struct Engine {
    pub app: AppHandle,
    pub db: Db,
    pub secrets: Secrets,
    pub http: reqwest::Client,
    workers: Mutex<HashMap<String, Worker>>,
}

impl Engine {
    pub fn new(app: AppHandle, db: Db, secrets: Secrets, http: reqwest::Client) -> Arc<Self> {
        Arc::new(Engine { app, db, secrets, http, workers: Mutex::new(HashMap::new()) })
    }

    pub fn ctx(&self, account_id: &str) -> Result<Ctx> {
        Ok(Ctx {
            app: self.app.clone(),
            db: self.db.clone(),
            secrets: self.secrets.clone(),
            http: self.http.clone(),
            account: self.db.account(account_id)?,
        })
    }

    pub fn start_all(self: &Arc<Self>) -> Result<()> {
        let _ = self.db.reset_stuck_outgoing();
        for a in self.db.accounts()? {
            self.start(&a.id);
        }
        let me = self.clone();
        tauri::async_runtime::spawn(async move { me.outbox_loop().await });
        let me = self.clone();
        tauri::async_runtime::spawn(async move { me.snooze_loop().await });
        Ok(())
    }

    pub fn start(self: &Arc<Self>, account_id: &str) {
        self.stop(account_id);
        let wake = Arc::new(Notify::new());
        let me = self.clone();
        let id = account_id.to_string();
        let w = wake.clone();
        let handle = tauri::async_runtime::spawn(async move { me.worker(id, w).await });
        self.workers
            .lock()
            .insert(account_id.to_string(), Worker { wake, handle });
    }

    pub fn stop(&self, account_id: &str) {
        if let Some(w) = self.workers.lock().remove(account_id) {
            w.handle.abort();
        }
    }

    pub fn wake(&self, account_id: &str) {
        if let Some(w) = self.workers.lock().get(account_id) {
            w.wake.notify_one();
        }
    }

    pub fn wake_all(&self) {
        for w in self.workers.lock().values() {
            w.wake.notify_one();
        }
    }

    fn status(&self, account_id: &str, state: &str, message: Option<String>) {
        let _ = self.app.emit(
            "sync:status",
            SyncStatus { account_id: account_id.into(), state: state.into(), message, progress: None },
        );
    }

    /// Applies a user action locally, queues the remote mutation and kicks the
    /// affected account workers.
    pub fn thread_action(&self, thread_ids: &[String], action: ThreadAction) -> Result<()> {
        let groups = self.db.apply_thread_action(thread_ids, action)?;
        for (account_id, _) in &groups {
            self.wake(account_id);
        }
        let _ = self.app.emit("mail:changed", "local");
        Ok(())
    }

    async fn worker(self: Arc<Self>, account_id: String, wake: Arc<Notify>) {
        let mut provider: Option<Provider> = None;
        let mut backoff = 0u32;
        loop {
            let interval = match self.run_once(&account_id, &mut provider).await {
                Ok(()) => {
                    backoff = 0;
                    let _ = self.db.set_sync_result(&account_id, None);
                    self.status(&account_id, "idle", None);
                    let is_imap = self.db.account(&account_id).map(|a| a.kind == AccountKind::Imap).unwrap_or(false);
                    Duration::from_secs(if is_imap { 60 } else { 30 })
                }
                Err(e) => {
                    let auth = e.is::<AuthExpired>() || e.chain().any(|c| c.is::<AuthExpired>());
                    let msg = if auth {
                        format!("Sign-in expired. Reconnect this account. ({e:#})")
                    } else {
                        format!("{e:#}")
                    };
                    tracing::warn!("sync {account_id} failed: {msg}");
                    let _ = self.db.set_sync_result(&account_id, Some(&msg));
                    self.status(&account_id, if auth { "auth" } else { "error" }, Some(msg));
                    // Drop connections so the next attempt starts clean.
                    provider = None;
                    backoff = (backoff + 1).min(6);
                    Duration::from_secs(if auth { 600 } else { 15 * 2u64.pow(backoff) })
                }
            };
            tokio::select! {
                _ = tokio::time::sleep(interval) => {}
                _ = wake.notified() => {}
            }
        }
    }

    async fn run_once(&self, account_id: &str, provider: &mut Option<Provider>) -> Result<()> {
        let ctx = self.ctx(account_id)?;
        if provider.is_none() {
            *provider = Some(Provider::new(&ctx)?);
        }
        let p = provider.as_mut().unwrap();

        // Push local changes first so the sync that follows doesn't undo them.
        for (op_id, op, attempts) in self.db.pending_ops::<Op>(account_id)? {
            match p.apply(&ctx, &op).await {
                Ok(()) => self.db.finish_op(op_id)?,
                Err(e) if e.is::<AuthExpired>() => return Err(e),
                Err(e) => {
                    tracing::warn!("op {op_id} failed: {e:#}");
                    if attempts >= 5 {
                        // Give up, and tell the user: the next sync will show
                        // the server's state again, which looks like an undo.
                        self.db.finish_op(op_id)?;
                        let _ = self.app.emit(
                            "ops:failed",
                            serde_json::json!({ "account": ctx.account.email, "action": op.action, "error": format!("{e:#}") }),
                        );
                    } else {
                        self.db.fail_op(op_id, &format!("{e:#}"))?;
                    }
                }
            }
        }

        self.status(account_id, "syncing", None);
        let since = crate::db::now();
        let first_sync = ctx.account.last_sync.is_none();
        p.sync(&ctx).await?;
        if !first_sync {
            self.notify_new_mail(account_id, since - 120);
        }
        Ok(())
    }

    fn notify_new_mail(&self, account_id: &str, since: i64) {
        let enabled = self.db.get_setting("notifications").ok().flatten().is_none_or(|v| v != "false");
        if !enabled {
            return;
        }
        let Ok(fresh) = self.db.list_threads(&crate::db::ThreadFilter {
            view: "inbox".into(),
            account_id: Some(account_id.into()),
            query: Some("is:unread category:primary".into()),
            limit: 5,
            offset: 0,
        }) else {
            return;
        };
        let key = format!("notified:{account_id}");
        let last_notified: i64 = self.db.get_setting(&key).ok().flatten().and_then(|v| v.parse().ok()).unwrap_or(since);
        let new: Vec<_> = fresh.into_iter().filter(|t| t.last_date > last_notified.max(since)).collect();
        if new.is_empty() {
            return;
        }
        let _ = self.db.set_setting(&key, &new[0].last_date.to_string());
        let (title, body) = if new.len() == 1 {
            let t = &new[0];
            let who = t
                .participants
                .last()
                .map(|p| p.name.clone().unwrap_or_else(|| p.email.clone()))
                .unwrap_or_default();
            (who, t.subject.clone())
        } else {
            (format!("{} new emails", new.len()), new.iter().map(|t| t.subject.clone()).collect::<Vec<_>>().join(" · "))
        };
        let _ = self.app.notification().builder().title(title).body(body).show();
    }

    async fn outbox_loop(self: Arc<Self>) {
        loop {
            tokio::time::sleep(Duration::from_millis(1000)).await;
            let due = match self.db.claim_due_outgoing() {
                Ok(d) => d,
                Err(e) => {
                    tracing::warn!("outbox: {e:#}");
                    continue;
                }
            };
            for (id, draft) in due {
                let me = self.clone();
                tauri::async_runtime::spawn(async move {
                    let result = async {
                        let ctx = me.ctx(&draft.account_id)?;
                        let mut p = Provider::new(&ctx)?;
                        p.send(&ctx, &draft).await
                    }
                    .await;
                    match result {
                        Ok(()) => {
                            let _ = me.db.finish_outgoing(&id, None);
                            if let Some(did) = &draft.id {
                                let _ = me.db.delete_draft(did);
                            }
                            let _ = me.app.emit("outbox:sent", &id);
                            me.wake(&draft.account_id);
                        }
                        Err(e) => {
                            let msg = format!("{e:#}");
                            tracing::warn!("send failed: {msg}");
                            let _ = me.db.finish_outgoing(&id, Some(&msg));
                            let _ = me.app.emit("outbox:failed", serde_json::json!({ "id": id, "error": msg }));
                        }
                    }
                });
            }
        }
    }

    async fn snooze_loop(self: Arc<Self>) {
        loop {
            match self.db.take_due_snoozes() {
                Ok(ids) if !ids.is_empty() => {
                    let _ = self.thread_action(&ids, ThreadAction::Inbox);
                    let _ = self.thread_action(&ids, ThreadAction::Unread);
                    let n = ids.len();
                    let _ = self
                        .app
                        .notification()
                        .builder()
                        .title("Snoozed")
                        .body(if n == 1 { "A snoozed conversation is back in your inbox".to_string() } else { format!("{n} snoozed conversations are back") })
                        .show();
                }
                Ok(_) => {}
                Err(e) => tracing::warn!("snooze: {e:#}"),
            }
            tokio::time::sleep(Duration::from_secs(20)).await;
        }
    }
}
