//! The link to a `cigmail-server`: the user's own server that keeps their mail in reach of AI
//! assistants (see `crates/cigmail-server`).
//!
//! The app pairs with it once, trading the server's password for a token, and from then on hands
//! over its accounts with their logins whenever they change. The server syncs mail on its own;
//! nothing flows back but who is connected.

use anyhow::{Context, anyhow, bail};
use serde::Serialize;
use serde_json::{Value, json};
use tauri::State;

use crate::{commands::AppState, config, db::Db, error::CmdResult, secrets::Secrets};

const URL_SETTING: &str = "assistant_server_url";
const TOKEN_KEY: &str = "server:token";
/// Settings the server's providers need besides the OAuth apps.
const PASSED_SETTINGS: &[&str] = &["microsoft_tenant", "sync_days"];

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ServerState {
    /// The server's address; `None` when the app isn't connected to one.
    url: Option<String>,
    /// What to give an assistant as the connector address.
    mcp_url: Option<String>,
    /// The server's accounts and what is connected to it, as it reports them.
    status: Option<Value>,
    /// Why the server couldn't be reached just now.
    error: Option<String>,
}

/// `calendar.example.org/mcp/` → `https://calendar.example.org`.
fn normalize_url(input: &str) -> anyhow::Result<String> {
    let mut url = input.trim().trim_end_matches('/').to_string();
    if let Some(base) = url.strip_suffix("/mcp") {
        url = base.to_string();
    }
    if url.is_empty() {
        bail!("enter the server's address, like https://mail.example.org");
    }
    if !url.contains("://") {
        url = format!("https://{url}");
    }
    let parsed = url::Url::parse(&url).map_err(|_| anyhow!("\"{input}\" is not an address like https://mail.example.org"))?;
    let host = parsed.host_str().unwrap_or("");
    let local = matches!(host, "localhost" | "127.0.0.1" | "[::1]" | "10.0.2.2");
    match parsed.scheme() {
        "https" => {}
        // The password and the mail logins travel over this connection.
        "http" if local => {}
        "http" => bail!("the server has to be reached over https://, or your mail logins would cross the network unprotected"),
        other => bail!("{other}:// is not a web address"),
    }
    Ok(url.trim_end_matches('/').to_string())
}

fn link(db: &Db, secrets: &Secrets) -> Option<(String, String)> {
    let url = db.get_setting(URL_SETTING).ok().flatten().filter(|u| !u.is_empty())?;
    let token = secrets.get(TOKEN_KEY).ok().flatten()?;
    Some((url, token))
}

fn forget(db: &Db, secrets: &Secrets) {
    let _ = db.set_setting(URL_SETTING, "");
    let _ = secrets.delete(TOKEN_KEY);
}

/// The server's answer as JSON, or what it complained about.
async fn answer(resp: reqwest::Result<reqwest::Response>, url: &str) -> anyhow::Result<Value> {
    let resp = resp.map_err(|e| anyhow!("could not reach {url}: {}", e.without_url()))?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    let body: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    if status.is_success() {
        return Ok(body);
    }
    match body["error"].as_str() {
        Some(message) => bail!("{message}"),
        None if status.as_u16() == 404 => bail!("{url} doesn't look like a CigMail server"),
        None => bail!("the server answered {status}"),
    }
}

/// Everything the server needs to sync the accounts itself.
fn bundle(db: &Db, secrets: &Secrets) -> anyhow::Result<Value> {
    let mut settings = serde_json::Map::new();
    if let Ok(google) = config::google(db) {
        settings.insert("google_client_id".into(), json!(google.client_id));
        if let Some(secret) = google.client_secret {
            settings.insert("google_client_secret".into(), json!(secret));
        }
    }
    if let Ok(microsoft) = config::microsoft_graph(db) {
        settings.insert("microsoft_client_id".into(), json!(microsoft.client_id));
    }
    for key in PASSED_SETTINGS {
        if let Some(value) = db.get_setting(key)?.filter(|v| !v.trim().is_empty()) {
            settings.insert((*key).into(), json!(value));
        }
    }
    let mut accounts = Vec::new();
    for account in db.accounts()? {
        let mut logins = serde_json::Map::new();
        for kind in ["oauth", "password"] {
            if let Some(value) = secrets.get(&format!("{kind}:{}", account.id))? {
                logins.insert(kind.into(), json!(value));
            }
        }
        // An account that lost its login here has nothing to hand over; the server keeps syncing
        // it with what it has until the user reconnects it.
        if logins.is_empty() {
            continue;
        }
        let config: Value = db.account_config(&account.id).unwrap_or_else(|_| json!({}));
        accounts.push(json!({ "account": account, "config": config, "secrets": logins }));
    }
    Ok(json!({ "replace": false, "settings": settings, "accounts": accounts }))
}

/// Hands the accounts over. `Ok(false)`: the app isn't connected to a server.
pub async fn push(db: &Db, secrets: &Secrets, http: &reqwest::Client) -> anyhow::Result<bool> {
    let Some((url, token)) = link(db, secrets) else { return Ok(false) };
    let body = bundle(db, secrets)?;
    answer(http.put(format!("{url}/v1/accounts")).bearer_auth(&token).json(&body).send().await, &url).await.context("handing the accounts to the server")?;
    Ok(true)
}

/// After an account was added, reconnected, changed or removed: tell the server, without making
/// the user wait for it.
pub fn push_soon(state: &AppState) {
    let (db, secrets, http) = (state.db.clone(), state.secrets.clone(), state.http.clone());
    if link(&db, &secrets).is_none() {
        return;
    }
    tauri::async_runtime::spawn(async move {
        if let Err(e) = push(&db, &secrets, &http).await {
            tracing::warn!("assistant server: {e:#}");
        }
    });
}

/// An account removed here is removed there too, with its mail and login.
pub fn forget_account_soon(state: &AppState, account_id: &str) {
    let (db, secrets, http) = (state.db.clone(), state.secrets.clone(), state.http.clone());
    let Some((url, token)) = link(&db, &secrets) else { return };
    let id = account_id.to_string();
    tauri::async_runtime::spawn(async move {
        let sent = http.delete(format!("{url}/v1/accounts/{id}")).bearer_auth(&token).send().await;
        if let Err(e) = answer(sent, &url).await {
            tracing::warn!("assistant server: removing an account: {e:#}");
        }
    });
}

async fn state_of(db: &Db, secrets: &Secrets, http: &reqwest::Client) -> ServerState {
    let Some((url, token)) = link(db, secrets) else { return ServerState::default() };
    let mcp_url = Some(format!("{url}/mcp"));
    match answer(http.get(format!("{url}/v1/status")).bearer_auth(&token).send().await, &url).await {
        Ok(status) => ServerState { url: Some(url), mcp_url, status: Some(status), error: None },
        Err(e) => ServerState { url: Some(url), mcp_url, status: None, error: Some(format!("{e:#}")) },
    }
}

#[tauri::command]
pub async fn server_status(state: State<'_, AppState>) -> CmdResult<ServerState> {
    Ok(state_of(&state.db, &state.secrets, &state.http).await)
}

/// Pairs with the server and hands over the accounts.
#[tauri::command]
pub async fn server_connect(state: State<'_, AppState>, url: String, password: String) -> CmdResult<ServerState> {
    let url = normalize_url(&url)?;
    let device_name = format!("CigMail ({})", std::env::consts::OS);
    let paired = answer(state.http.post(format!("{url}/v1/pair")).json(&json!({ "password": password, "deviceName": device_name })).send().await, &url).await?;
    let token = paired["token"].as_str().context("the server sent no token")?;
    // Leave a server the app was connected to before.
    if let Some((old_url, old_token)) = link(&state.db, &state.secrets)
        && old_url != url
    {
        let _ = state.http.post(format!("{old_url}/v1/unpair")).bearer_auth(old_token).send().await;
    }
    state.secrets.set(TOKEN_KEY, token)?;
    state.db.set_setting(URL_SETTING, &url)?;
    push(&state.db, &state.secrets, &state.http).await?;
    Ok(state_of(&state.db, &state.secrets, &state.http).await)
}

/// Hands the accounts over again, e.g. after the server lost a sign-in.
#[tauri::command]
pub async fn server_push(state: State<'_, AppState>) -> CmdResult<ServerState> {
    push(&state.db, &state.secrets, &state.http).await?;
    Ok(state_of(&state.db, &state.secrets, &state.http).await)
}

/// Disconnects this app. With `wipe`, the server first forgets every account, its mail and logins.
#[tauri::command]
pub async fn server_disconnect(state: State<'_, AppState>, wipe: bool) -> CmdResult<()> {
    if let Some((url, token)) = link(&state.db, &state.secrets) {
        if wipe {
            let cleared = state.http.put(format!("{url}/v1/accounts")).bearer_auth(&token).json(&json!({ "replace": true, "accounts": [] })).send().await;
            answer(cleared, &url).await.context("clearing the server")?;
        }
        let _ = state.http.post(format!("{url}/v1/unpair")).bearer_auth(&token).send().await;
    }
    forget(&state.db, &state.secrets);
    Ok(())
}

/// Removes an assistant or another paired app from the server.
#[tauri::command]
pub async fn server_remove_device(state: State<'_, AppState>, id: String) -> CmdResult<ServerState> {
    let (url, token) = link(&state.db, &state.secrets).context("not connected to a server")?;
    answer(state.http.delete(format!("{url}/v1/devices/{id}")).bearer_auth(&token).send().await, &url).await?;
    Ok(state_of(&state.db, &state.secrets, &state.http).await)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Account, AccountKind};

    /// The app's side against a real server: what `push` sends is what the server takes.
    #[tokio::test(flavor = "multi_thread")]
    async fn accounts_reach_the_server() {
        let dir = tempfile::tempdir().unwrap();
        let server = cigmail_server::start(cigmail_server::Config {
            bind: "127.0.0.1:0".parse().unwrap(),
            data_dir: dir.path().join("server"),
            password: Some("correct horse battery".into()),
            ..Default::default()
        })
        .await
        .unwrap();
        let db = Db::open(&dir.path().join("app.db")).unwrap();
        let secrets = Secrets::new(db.clone());
        let http = reqwest::Client::new();
        assert!(!push(&db, &secrets, &http).await.unwrap(), "not connected: nothing to do");

        let account = |id: &str, email: &str| Account {
            id: id.into(),
            kind: AccountKind::Imap,
            email: email.into(),
            name: "Anna".into(),
            label: Some("Personal".into()),
            color: "#5e6ad2".into(),
            created_at: 1_794_000_000,
            last_sync: None,
            last_error: None,
            signature: Some("Anna\nexample.org".into()),
        };
        // Nothing listens on port 9: the server's sync fails quietly, which is fine here.
        let config = json!({ "imapHost": "127.0.0.1", "imapPort": 9, "imapSecurity": "none", "smtpHost": "127.0.0.1", "smtpPort": 9,
                             "smtpSecurity": "none", "username": "anna@example.org", "auth": "password" });
        db.insert_account(&account("acc1", "anna@example.org"), &config).unwrap();
        db.insert_account(&account("acc2", "nologin@example.org"), &config).unwrap();
        // A new account has no signature; the user sets it afterwards.
        db.update_account_profile("acc1", "Anna", Some("Personal"), "#5e6ad2", Some("Anna\nexample.org")).unwrap();
        // The keyring isn't there on a build machine, so say where the secret goes.
        db.set_fallback_secret("password:acc1", Some("mailpass")).unwrap();
        db.set_setting("google_client_id", "abc.apps.googleusercontent.com").unwrap();
        db.set_setting("sync_days", "30").unwrap();

        // Pair as `server_connect` does.
        let url = server.url();
        let paired = answer(http.post(format!("{url}/v1/pair")).json(&json!({ "password": "correct horse battery", "deviceName": "test" })).send().await, &url).await.unwrap();
        db.set_fallback_secret(TOKEN_KEY, paired["token"].as_str()).unwrap();
        db.set_setting(URL_SETTING, &url).unwrap();
        let wrong = answer(http.post(format!("{url}/v1/pair")).json(&json!({ "password": "nope" })).send().await, &url).await.unwrap_err();
        assert_eq!(wrong.to_string(), "wrong password");

        assert!(push(&db, &secrets, &http).await.unwrap());
        let there = server.state.mail.accounts().unwrap();
        assert_eq!(there.len(), 1, "an account without a login here isn't handed over");
        assert_eq!((there[0].id.as_str(), there[0].email.as_str(), there[0].signature.as_deref()), ("acc1", "anna@example.org", Some("Anna\nexample.org")));
        assert_eq!(server.state.secrets.get("password:acc1").unwrap().as_deref(), Some("mailpass"));
        assert_eq!(server.state.mail.get_setting("google_client_id").unwrap().as_deref(), Some("abc.apps.googleusercontent.com"));
        assert_eq!(server.state.mail.get_setting("sync_days").unwrap().as_deref(), Some("30"));
        let imap: crate::models::ImapConfig = server.state.mail.account_config("acc1").unwrap();
        assert_eq!((imap.imap_port, imap.username.as_str()), (9, "anna@example.org"));

        let state = state_of(&db, &secrets, &http).await;
        assert_eq!(state.mcp_url.as_deref(), Some(format!("{url}/mcp").as_str()));
        assert_eq!(state.status.unwrap()["accounts"][0]["email"], "anna@example.org");
        server.stop().await;
        let state = state_of(&db, &secrets, &http).await;
        assert!(state.status.is_none() && state.error.unwrap().contains("could not reach"));
    }

    #[test]
    fn addresses() {
        assert_eq!(normalize_url(" mail.example.org/mcp/ ").unwrap(), "https://mail.example.org");
        assert_eq!(normalize_url("https://mail.example.org/").unwrap(), "https://mail.example.org");
        assert_eq!(normalize_url("http://localhost:8788").unwrap(), "http://localhost:8788");
        assert!(normalize_url("http://mail.example.org").is_err(), "logins don't go over plain http");
        assert!(normalize_url("ftp://mail.example.org").is_err());
        assert!(normalize_url("  ").is_err());
    }
}
