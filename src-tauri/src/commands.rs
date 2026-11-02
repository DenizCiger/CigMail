//! Tauri commands: the API surface the frontend talks to.

use std::{collections::HashMap, sync::Arc};

use anyhow::{Context, anyhow, bail};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State, ipc::Channel};
use tauri_plugin_opener::OpenerExt;
use tokio::task::AbortHandle;

use crate::{
    ai::{self, Ai, AiStatus, ModelInfo},
    config,
    db::{Db, ThreadAction, ThreadFilter, now},
    error::CmdResult,
    mime,
    models::*,
    oauth::{self, Tokens},
    providers::{Provider, imap::Imap},
    secrets::Secrets,
    sync::Engine,
};

pub struct AppState {
    pub db: Db,
    pub secrets: Secrets,
    pub http: reqwest::Client,
    pub engine: Arc<Engine>,
    pub ai: Arc<Ai>,
    pub ai_tasks: Mutex<HashMap<String, AbortHandle>>,
}

const COLORS: [&str; 8] = [
    "#5e6ad2", "#26b5ce", "#4cb782", "#f2c94c", "#f2994a", "#eb5757", "#bb87fc", "#e86fb4",
];

fn pick_color(db: &Db) -> String {
    let used = db.accounts().map(|a| a.len()).unwrap_or(0);
    COLORS[used % COLORS.len()].to_string()
}

fn open_browser(app: &AppHandle) -> impl FnOnce(&str) -> anyhow::Result<()> + use<> {
    let app = app.clone();
    move |url: &str| {
        app.opener().open_url(url, None::<&str>).map_err(|e| anyhow!("could not open browser: {e}"))
    }
}

// ------------------------------------------------------------------ accounts

#[tauri::command]
pub fn list_accounts(state: State<'_, AppState>) -> CmdResult<Vec<Account>> {
    Ok(state.db.accounts()?)
}

async fn identity(http: &reqwest::Client, kind: AccountKind, tokens: &Tokens) -> anyhow::Result<(String, String)> {
    match kind {
        AccountKind::Gmail => {
            let v: serde_json::Value = http
                .get("https://openidconnect.googleapis.com/v1/userinfo")
                .bearer_auth(&tokens.access_token)
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?;
            let email = v["email"].as_str().context("Google did not return an email")?.to_string();
            let name = v["name"].as_str().unwrap_or(&email).to_string();
            Ok((email, name))
        }
        _ => {
            let v: serde_json::Value = http
                .get("https://graph.microsoft.com/v1.0/me?$select=displayName,mail,userPrincipalName")
                .bearer_auth(&tokens.access_token)
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?;
            let email = v["mail"]
                .as_str()
                .or(v["userPrincipalName"].as_str())
                .context("Microsoft did not return an email")?
                .to_string();
            let name = v["displayName"].as_str().unwrap_or(&email).to_string();
            Ok((email, name))
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewOAuthAccount {
    kind: AccountKind,
    label: Option<String>,
}

#[tauri::command]
pub async fn add_oauth_account(app: AppHandle, state: State<'_, AppState>, req: NewOAuthAccount) -> CmdResult<Account> {
    let client = match req.kind {
        AccountKind::Gmail => config::google(&state.db)?,
        AccountKind::Outlook => config::microsoft_graph(&state.db)?,
        AccountKind::Imap => return Err(anyhow!("use add_imap_account").into()),
    };
    let tokens = oauth::authorize(&state.http, &client, open_browser(&app)).await?;
    let (email, name) = identity(&state.http, req.kind, &tokens).await?;

    // Reconnecting an existing account just refreshes its tokens.
    if let Some(existing) = state
        .db
        .accounts()?
        .into_iter()
        .find(|a| a.kind == req.kind && a.email.eq_ignore_ascii_case(&email))
    {
        state.secrets.set_json(&format!("oauth:{}", existing.id), &tokens)?;
        state.db.set_sync_result(&existing.id, None)?;
        state.engine.start(&existing.id);
        return Ok(state.db.account(&existing.id)?);
    }

    let account = Account {
        id: uuid::Uuid::new_v4().simple().to_string()[..12].to_string(),
        kind: req.kind,
        email,
        name,
        label: req.label.filter(|l| !l.trim().is_empty()),
        color: pick_color(&state.db),
        created_at: now(),
        last_sync: None,
        last_error: None,
        signature: None,
    };
    state.secrets.set_json(&format!("oauth:{}", account.id), &tokens)?;
    state.db.insert_account(&account, &serde_json::json!({}))?;
    state.engine.start(&account.id);
    Ok(account)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewImapAccount {
    email: String,
    name: String,
    label: Option<String>,
    config: ImapConfig,
    password: Option<String>,
}

#[tauri::command]
pub async fn test_imap(config: ImapConfig, password: String) -> CmdResult<()> {
    Imap::test(&config, &password).await?;
    Ok(())
}

#[tauri::command]
pub async fn add_imap_account(app: AppHandle, state: State<'_, AppState>, req: NewImapAccount) -> CmdResult<Account> {
    let id = uuid::Uuid::new_v4().simple().to_string()[..12].to_string();
    let mut email = req.email.trim().to_string();
    let mut name = req.name.trim().to_string();
    match req.config.auth {
        ImapAuth::Password => {
            let pw = req.password.clone().unwrap_or_default();
            Imap::test(&req.config, &pw).await?;
            state.secrets.set(&format!("password:{id}"), &pw)?;
        }
        ImapAuth::MicrosoftOAuth => {
            let tokens =
                oauth::authorize(&state.http, &config::microsoft_imap(&state.db)?, open_browser(&app)).await?;
            if let Some(c) = tokens.id_token.as_deref().and_then(oauth::decode_jwt_claims) {
                if email.is_empty() {
                    email = c["preferred_username"].as_str().or(c["email"].as_str()).unwrap_or("").to_string();
                }
                if name.is_empty() {
                    name = c["name"].as_str().unwrap_or("").to_string();
                }
            }
            let mut cfg = req.config.clone();
            if cfg.username.is_empty() {
                cfg.username = email.clone();
            }
            Imap::connect_with(&cfg, &tokens.access_token).await?;
            state.secrets.set_json(&format!("oauth:{id}"), &tokens)?;
        }
    }
    if email.is_empty() {
        return Err(anyhow!("email address is required").into());
    }
    let mut cfg = req.config;
    if cfg.username.is_empty() {
        cfg.username = email.clone();
    }
    let account = Account {
        id,
        kind: AccountKind::Imap,
        name: if name.is_empty() { email.clone() } else { name },
        email,
        label: req.label.filter(|l| !l.trim().is_empty()),
        color: pick_color(&state.db),
        created_at: now(),
        last_sync: None,
        last_error: None,
        signature: None,
    };
    state.db.insert_account(&account, &serde_json::to_value(&cfg)?)?;
    state.engine.start(&account.id);
    Ok(account)
}

#[tauri::command]
pub async fn reconnect_account(app: AppHandle, state: State<'_, AppState>, id: String, password: Option<String>) -> CmdResult<()> {
    let account = state.db.account(&id)?;
    match account.kind {
        AccountKind::Gmail | AccountKind::Outlook => {
            let client = if account.kind == AccountKind::Gmail {
                config::google(&state.db)?
            } else {
                config::microsoft_graph(&state.db)?
            };
            let tokens = oauth::authorize(&state.http, &client, open_browser(&app)).await?;
            let (email, _) = identity(&state.http, account.kind, &tokens).await?;
            if !email.eq_ignore_ascii_case(&account.email) {
                return Err(anyhow!("you signed in as {email}, but this account is {}", account.email).into());
            }
            state.secrets.set_json(&format!("oauth:{id}"), &tokens)?;
        }
        AccountKind::Imap => {
            let cfg: ImapConfig = state.db.account_config(&id)?;
            match cfg.auth {
                ImapAuth::Password => {
                    let pw = password.context("password required")?;
                    Imap::test(&cfg, &pw).await?;
                    state.secrets.set(&format!("password:{id}"), &pw)?;
                }
                ImapAuth::MicrosoftOAuth => {
                    let tokens =
                        oauth::authorize(&state.http, &config::microsoft_imap(&state.db)?, open_browser(&app)).await?;
                    state.secrets.set_json(&format!("oauth:{id}"), &tokens)?;
                }
            }
        }
    }
    state.db.set_sync_result(&id, None)?;
    state.engine.start(&id);
    Ok(())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountUpdate {
    id: String,
    name: String,
    label: Option<String>,
    color: String,
    signature: Option<String>,
}

#[tauri::command]
pub fn update_account(state: State<'_, AppState>, update: AccountUpdate) -> CmdResult<Account> {
    state.db.update_account_profile(
        &update.id,
        update.name.trim(),
        update.label.as_deref().map(str::trim).filter(|l| !l.is_empty()),
        &update.color,
        update.signature.as_deref().filter(|s| !s.trim().is_empty()),
    )?;
    Ok(state.db.account(&update.id)?)
}

#[tauri::command]
pub fn remove_account(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    state.engine.stop(&id);
    let _ = state.secrets.delete(&format!("oauth:{id}"));
    let _ = state.secrets.delete(&format!("password:{id}"));
    state.db.delete_account(&id)?;
    Ok(())
}

#[tauri::command]
pub fn cancel_oauth() {
    oauth::cancel_pending();
}

#[tauri::command]
pub fn sync_now(state: State<'_, AppState>, account_id: Option<String>) {
    match account_id {
        Some(id) => state.engine.wake(&id),
        None => state.engine.wake_all(),
    }
}

/// Wipes the local cache for an account and downloads everything again.
#[tauri::command]
pub fn resync_account(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    state.engine.stop(&id);
    state.db.clear_account_mail(&id)?;
    state.db.set_sync_state(&id, &serde_json::json!({}))?;
    state.engine.start(&id);
    Ok(())
}

// ------------------------------------------------------------------ threads

#[tauri::command]
pub fn list_threads(state: State<'_, AppState>, filter: ThreadFilter) -> CmdResult<Vec<ThreadSummary>> {
    Ok(state.db.list_threads(&filter)?)
}

#[tauri::command]
pub fn count_unread(state: State<'_, AppState>, filters: HashMap<String, ThreadFilter>) -> CmdResult<HashMap<String, i64>> {
    let mut out = HashMap::new();
    for (k, f) in filters {
        out.insert(k, state.db.count_threads(&f, true)?);
    }
    Ok(out)
}

#[tauri::command]
pub fn get_thread(state: State<'_, AppState>, id: String) -> CmdResult<ThreadDetail> {
    Ok(state.db.thread(&id)?)
}

#[tauri::command]
pub fn thread_action(state: State<'_, AppState>, thread_ids: Vec<String>, action: ThreadAction) -> CmdResult<()> {
    state.engine.thread_action(&thread_ids, action)?;
    Ok(())
}

#[tauri::command]
pub fn snooze(state: State<'_, AppState>, thread_ids: Vec<String>, until: Option<i64>) -> CmdResult<()> {
    for t in &thread_ids {
        state.db.set_snooze(t, until)?;
    }
    let action = if until.is_some() { ThreadAction::Archive } else { ThreadAction::Inbox };
    state.engine.thread_action(&thread_ids, action)?;
    Ok(())
}

// ------------------------------------------------------------------ compose

#[tauri::command]
pub fn reply_draft(state: State<'_, AppState>, message_id: String, mode: String) -> CmdResult<Draft> {
    let msg = state.db.message(&message_id)?;
    let account = state.db.account(&msg.account_id)?;
    Ok(mime::reply_draft(&msg, &account, &mode))
}

#[tauri::command]
pub fn send_message(state: State<'_, AppState>, draft: Draft) -> CmdResult<SendResult> {
    if draft.to.is_empty() && draft.cc.is_empty() && draft.bcc.is_empty() {
        return Err(anyhow!("add at least one recipient").into());
    }
    for a in draft.to.iter().chain(&draft.cc).chain(&draft.bcc) {
        if a.email.parse::<lettre::Address>().is_err() {
            return Err(anyhow!("invalid address: {}", a.email).into());
        }
    }
    let delay: i64 = state
        .db
        .get_setting("undo_send_secs")?
        .and_then(|v| v.parse().ok())
        .unwrap_or(10);
    let id = uuid::Uuid::new_v4().to_string();
    let send_at = now() + delay.clamp(0, 60);
    state.db.queue_outgoing(&id, &draft, send_at)?;
    if let Some(did) = &draft.id {
        let _ = state.db.delete_draft(did);
    }
    Ok(SendResult { id, send_at })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SendResult {
    id: String,
    send_at: i64,
}

#[tauri::command]
pub fn cancel_send(state: State<'_, AppState>, id: String) -> CmdResult<Option<Draft>> {
    Ok(state.db.cancel_outgoing(&id)?)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FailedSend {
    id: String,
    draft: Draft,
    error: String,
}

#[tauri::command]
pub fn failed_sends(state: State<'_, AppState>) -> CmdResult<Vec<FailedSend>> {
    Ok(state
        .db
        .failed_outgoing()?
        .into_iter()
        .map(|(id, draft, error)| FailedSend { id, draft, error })
        .collect())
}

#[tauri::command]
pub fn save_draft(state: State<'_, AppState>, draft: Draft) -> CmdResult<String> {
    Ok(state.db.save_draft(&draft)?)
}

#[tauri::command]
pub fn list_drafts(state: State<'_, AppState>) -> CmdResult<Vec<Draft>> {
    Ok(state.db.drafts()?)
}

#[tauri::command]
pub fn delete_draft(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    Ok(state.db.delete_draft(&id)?)
}

#[tauri::command]
pub fn suggest_contacts(state: State<'_, AppState>, query: String) -> CmdResult<Vec<Contact>> {
    Ok(state.db.suggest_contacts(query.trim(), 8)?)
}

/// Downloads an attachment (re-fetching the raw message) and either opens it
/// or saves it to the Downloads folder. Returns the file path.
#[tauri::command]
pub async fn open_attachment(
    app: AppHandle,
    state: State<'_, AppState>,
    message_id: String,
    index: usize,
    save: bool,
) -> CmdResult<String> {
    let (account_id, r) = state.db.message_locator(&message_id)?;
    let ctx = state.engine.ctx(&account_id)?;
    let mut provider = Provider::new(&ctx)?;
    let raw = provider.fetch_raw(&ctx, &r).await?;
    let (name, _mime, bytes) = mime::extract_attachment(&raw, index)?;
    let safe: String = name
        .chars()
        .map(|c| if matches!(c, '/' | '\\' | ':' | '\0') { '_' } else { c })
        .collect();
    let dir = if save {
        app.path().download_dir().map_err(|e| anyhow!("{e}"))?
    } else {
        let d = std::env::temp_dir().join("cigmail");
        std::fs::create_dir_all(&d)?;
        d
    };
    let mut path = dir.join(&safe);
    if save {
        let (stem, ext) = match safe.rsplit_once('.') {
            Some((s, e)) => (s.to_string(), format!(".{e}")),
            None => (safe.clone(), String::new()),
        };
        let mut n = 1;
        while path.exists() {
            path = dir.join(format!("{stem} ({n}){ext}"));
            n += 1;
        }
    }
    std::fs::write(&path, bytes)?;
    let p = path.to_string_lossy().to_string();
    if !save {
        app.opener().open_path(&p, None::<&str>).map_err(|e| anyhow!("could not open file: {e}"))?;
    }
    Ok(p)
}

// ------------------------------------------------------------------ settings

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> CmdResult<HashMap<String, String>> {
    let mut s = state.db.all_settings()?;
    // Never echo secrets back to the UI.
    s.remove("google_client_secret").map(|_| s.insert("google_client_secret".into(), "••••••••".into()));
    s.retain(|k, _| !k.starts_with("notified:"));
    Ok(s)
}

#[tauri::command]
pub fn set_setting(state: State<'_, AppState>, key: String, value: String) -> CmdResult<()> {
    if key == "google_client_secret" && value.starts_with('•') {
        return Ok(());
    }
    state.db.set_setting(&key, &value)?;
    if matches!(key.as_str(), "google_client_id" | "google_client_secret" | "microsoft_client_id" | "microsoft_tenant") {
        // Providers cache their OAuth client; restart workers to pick up changes.
        for a in state.db.accounts()? {
            state.engine.start(&a.id);
        }
    }
    Ok(())
}

pub fn default_splits() -> Vec<Split> {
    vec![
        Split { id: "important".into(), name: "Important".into(), query: "category:primary".into() },
        Split { id: "news".into(), name: "News".into(), query: "category:newsletter".into() },
        Split { id: "updates".into(), name: "Updates".into(), query: "category:notification".into() },
    ]
}

#[tauri::command]
pub fn get_splits(state: State<'_, AppState>) -> CmdResult<Vec<Split>> {
    Ok(state.db.get_setting_json("splits")?.unwrap_or_else(default_splits))
}

#[tauri::command]
pub fn set_splits(state: State<'_, AppState>, splits: Vec<Split>) -> CmdResult<()> {
    state.db.set_setting("splits", &serde_json::to_string(&splits)?)?;
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthConfigStatus {
    google: bool,
    microsoft: bool,
}

#[tauri::command]
pub fn oauth_config_status(state: State<'_, AppState>) -> OAuthConfigStatus {
    OAuthConfigStatus {
        google: config::google(&state.db).is_ok(),
        microsoft: config::microsoft_graph(&state.db).is_ok(),
    }
}

// ------------------------------------------------------------------ AI

#[derive(Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum AiEvent {
    Delta { text: String },
    Done { text: String },
    Error { message: String },
}

#[tauri::command]
pub fn ai_status(state: State<'_, AppState>) -> CmdResult<AiStatus> {
    Ok(state.ai.status()?)
}

#[tauri::command]
pub async fn ai_login(app: AppHandle, state: State<'_, AppState>) -> CmdResult<AiStatus> {
    Ok(state.ai.login(open_browser(&app)).await?)
}

#[tauri::command]
pub fn ai_logout(state: State<'_, AppState>) -> CmdResult<()> {
    Ok(state.ai.logout()?)
}

#[tauri::command]
pub async fn ai_models(state: State<'_, AppState>) -> CmdResult<Vec<ModelInfo>> {
    Ok(state.ai.models().await?)
}

/// Runs an AI job in the background, streaming events to `channel`, and
/// registers it under `request_id` so it can be cancelled.
fn spawn_ai<F>(state: &AppState, request_id: String, channel: Channel<AiEvent>, job: F)
where
    F: std::future::Future<Output = anyhow::Result<String>> + Send + 'static,
{
    let ch = channel.clone();
    let handle = tauri::async_runtime::spawn(async move {
        match job.await {
            Ok(text) => {
                let _ = ch.send(AiEvent::Done { text });
            }
            Err(e) => {
                let _ = ch.send(AiEvent::Error { message: format!("{e:#}") });
            }
        }
    });
    let mut tasks = state.ai_tasks.lock();
    if let Some(prev) = tasks.insert(request_id, handle.inner().abort_handle()) {
        prev.abort();
    }
}

#[tauri::command]
pub fn ai_cancel(state: State<'_, AppState>, request_id: String) {
    if let Some(h) = state.ai_tasks.lock().remove(&request_id) {
        h.abort();
    }
}

#[tauri::command]
pub fn ai_summarize(
    state: State<'_, AppState>,
    request_id: String,
    thread_id: String,
    force: bool,
    channel: Channel<AiEvent>,
) -> CmdResult<()> {
    let detail = state.db.thread(&thread_id)?;
    let key = format!(
        "{}:{}",
        detail.messages.len(),
        detail.messages.last().map(|m| m.id.as_str()).unwrap_or("")
    );
    if !force {
        if let Some((summary, k)) = state.db.ai_summary(&thread_id)? {
            if k == key {
                let _ = channel.send(AiEvent::Done { text: summary });
                return Ok(());
            }
        }
    }
    let account = state.db.account(&detail.thread.account_id)?;
    let ai = state.ai.clone();
    let db = state.db.clone();
    let ch = channel.clone();
    spawn_ai(&state, request_id, channel, async move {
        let system = ai::summary_prompt(&account);
        let text = ai
            .stream(&system, &ai::transcript(&detail, 24_000), |d| {
                let _ = ch.send(AiEvent::Delta { text: d.to_string() });
            })
            .await?;
        let text = text.trim().to_string();
        db.set_ai_summary(&thread_id, &text, &key)?;
        Ok(text)
    });
    Ok(())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComposeRequest {
    account_id: String,
    instruction: String,
    #[serde(default)]
    draft_text: String,
    #[serde(default)]
    thread_id: Option<String>,
    #[serde(default)]
    recipients: Vec<Address>,
}

#[tauri::command]
pub fn ai_compose(
    state: State<'_, AppState>,
    request_id: String,
    req: ComposeRequest,
    channel: Channel<AiEvent>,
) -> CmdResult<()> {
    let account = state.db.account(&req.account_id)?;
    let samples = state.db.recent_sent_texts(&req.account_id, 8)?;
    let thread = req.thread_id.as_deref().and_then(|t| state.db.thread(t).ok());
    let ai = state.ai.clone();
    let ch = channel.clone();
    spawn_ai(&state, request_id, channel, async move {
        let system = ai::compose_prompt(&account, &samples);
        let mut user = format!("Instruction: {}\n", req.instruction.trim());
        if !req.recipients.is_empty() {
            let r: Vec<String> = req.recipients.iter().map(Address::display).collect();
            user.push_str(&format!("Recipients: {}\n", r.join(", ")));
        }
        if !req.draft_text.trim().is_empty() {
            user.push_str(&format!("\nCurrent draft (improve/continue it):\n{}\n", req.draft_text.trim()));
        }
        if let Some(t) = &thread {
            user.push_str(&format!("\nConversation being replied to:\n{}\n", ai::transcript(t, 16_000)));
        }
        let text = ai
            .stream(&system, &user, |d| {
                let _ = ch.send(AiEvent::Delta { text: d.to_string() });
            })
            .await?;
        Ok(text.trim().to_string())
    });
    Ok(())
}

#[tauri::command]
pub fn ai_rewrite(
    state: State<'_, AppState>,
    request_id: String,
    instruction: String,
    text: String,
    channel: Channel<AiEvent>,
) -> CmdResult<()> {
    if text.trim().is_empty() {
        bail_cmd("nothing to rewrite")?;
    }
    let ai = state.ai.clone();
    let ch = channel.clone();
    spawn_ai(&state, request_id, channel, async move {
        let user = format!("Instruction: {instruction}\n\nText:\n{text}");
        let out = ai
            .stream(ai::REWRITE_PROMPT, &user, |d| {
                let _ = ch.send(AiEvent::Delta { text: d.to_string() });
            })
            .await?;
        Ok(out.trim().to_string())
    });
    Ok(())
}

fn bail_cmd(msg: &str) -> anyhow::Result<()> {
    bail!("{msg}")
}
