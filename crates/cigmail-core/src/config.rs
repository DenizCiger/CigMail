//! OAuth client definitions. Client IDs come from Settings first, then from
//! build-time environment variables (see SETUP.md).

use anyhow::{Result, bail};

use crate::{
    db::Db,
    oauth::{OAuthClient, Redirect},
};

/// The public client used by OpenAI's Codex CLI (and openai-oauth).
pub const CODEX_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";

fn setting_or_env(db: &Db, key: &str, env: Option<&'static str>) -> Option<String> {
    db.get_setting(key)
        .ok()
        .flatten()
        .filter(|v| !v.trim().is_empty())
        .map(|v| v.trim().to_string())
        .or_else(|| env.map(String::from).filter(|v| !v.is_empty()))
}

pub fn google(db: &Db) -> Result<OAuthClient> {
    let Some(client_id) = setting_or_env(db, "google_client_id", option_env!("CIGMAIL_GOOGLE_CLIENT_ID")) else {
        bail!("Google OAuth client is not configured. Add its client ID under Settings → Connections.");
    };
    Ok(OAuthClient {
        auth_url: "https://accounts.google.com/o/oauth2/v2/auth".into(),
        token_url: "https://oauth2.googleapis.com/token".into(),
        client_id,
        // Google's "Desktop app" clients have a secret that isn't actually secret.
        client_secret: setting_or_env(db, "google_client_secret", option_env!("CIGMAIL_GOOGLE_CLIENT_SECRET")),
        scopes: "openid email profile https://www.googleapis.com/auth/gmail.modify".into(),
        extra_params: vec![
            ("access_type".into(), "offline".into()),
            ("prompt".into(), "consent select_account".into()),
        ],
        redirect: Redirect::Ip,
        refresh_as_json: false,
    })
}

fn microsoft(db: &Db, scopes: &str) -> Result<OAuthClient> {
    let Some(client_id) = setting_or_env(db, "microsoft_client_id", option_env!("CIGMAIL_MICROSOFT_CLIENT_ID")) else {
        bail!("Microsoft OAuth client is not configured. Add its client ID under Settings → Connections.");
    };
    let tenant = setting_or_env(db, "microsoft_tenant", None).unwrap_or_else(|| "common".into());
    Ok(OAuthClient {
        auth_url: format!("https://login.microsoftonline.com/{tenant}/oauth2/v2.0/authorize"),
        token_url: format!("https://login.microsoftonline.com/{tenant}/oauth2/v2.0/token"),
        client_id,
        client_secret: None,
        scopes: scopes.into(),
        extra_params: vec![("prompt".into(), "select_account".into())],
        redirect: Redirect::Localhost,
        refresh_as_json: false,
    })
}

pub fn microsoft_graph(db: &Db) -> Result<OAuthClient> {
    microsoft(
        db,
        "offline_access openid email profile https://graph.microsoft.com/User.Read https://graph.microsoft.com/Mail.ReadWrite https://graph.microsoft.com/Mail.Send",
    )
}

/// IMAP/SMTP over XOAUTH2, for Microsoft 365 tenants that don't allow Graph consent.
pub fn microsoft_imap(db: &Db) -> Result<OAuthClient> {
    microsoft(
        db,
        "offline_access openid email https://outlook.office.com/IMAP.AccessAsUser.All https://outlook.office.com/SMTP.Send",
    )
}

pub fn codex() -> OAuthClient {
    OAuthClient {
        auth_url: "https://auth.openai.com/oauth/authorize".into(),
        token_url: "https://auth.openai.com/oauth/token".into(),
        client_id: CODEX_CLIENT_ID.into(),
        client_secret: None,
        scopes: "openid profile email offline_access".into(),
        extra_params: vec![
            ("id_token_add_organizations".into(), "true".into()),
            ("codex_cli_simplified_flow".into(), "true".into()),
        ],
        redirect: Redirect::Fixed { port: 1455, path: "/auth/callback" },
        refresh_as_json: true,
    }
}
