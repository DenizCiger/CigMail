//! Sign-in for AI assistants: the OAuth 2.1 authorization server in front of `/mcp`.
//!
//! An assistant (Claude, Gemini, ...) registers itself (`POST /oauth/register`), sends the user to
//! `/oauth/authorize` where they enter the server's password and choose whether the assistant may
//! change things, and swaps the resulting code for an access token and a refresh token
//! (`POST /oauth/token`). PKCE (S256) is required.
//!
//! Each connected assistant is a row in `devices`, listed and removed from the app. Like app
//! tokens, only hashes of codes and tokens are stored.

// Helpers that can fail hand back the whole response to send instead.
#![allow(clippy::result_large_err)]

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{ConnectInfo, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use axum::{Form, Json};
use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::api::{ApiError, ApiResult, App, blocking, client_ip};
use crate::auth::now_ms;
use crate::{AppState, auth};

pub const SCOPE_READ: &str = "read";
pub const SCOPE_WRITE: &str = "write";

const CODE_TTL_MS: i64 = 5 * 60_000;
const ACCESS_TTL_MS: i64 = 3_600_000;
/// A refresh token that was just replaced still works this long, so a client that lost the answer
/// to its refresh can ask again.
const REFRESH_GRACE_MS: i64 = 2 * 60_000;
/// Clients that registered but never got a sign-in are forgotten after this.
const UNUSED_CLIENT_TTL_MS: i64 = 86_400_000;
const LOG_RETENTION_MS: i64 = 90 * 86_400_000;
const REGISTER_WINDOW: Duration = Duration::from_secs(3600);
const MAX_REGISTRATIONS_PER_IP: usize = 30;

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS oauth_clients (
    id            TEXT PRIMARY KEY,
    name          TEXT NOT NULL,
    redirect_uris TEXT NOT NULL,
    -- Only for clients that asked for a secret; PKCE is required either way.
    secret_hash   TEXT,
    created_at    INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS oauth_codes (
    code_hash    TEXT PRIMARY KEY,
    client_id    TEXT NOT NULL REFERENCES oauth_clients(id) ON DELETE CASCADE,
    redirect_uri TEXT NOT NULL,
    challenge    TEXT NOT NULL,
    scope        TEXT NOT NULL,
    expires_at   INTEGER NOT NULL
);
-- One per connected assistant. `device_id` is its entry in the device list.
CREATE TABLE IF NOT EXISTS oauth_grants (
    id                TEXT PRIMARY KEY,
    device_id         TEXT NOT NULL REFERENCES devices(id) ON DELETE CASCADE,
    client_id         TEXT NOT NULL REFERENCES oauth_clients(id) ON DELETE CASCADE,
    scope             TEXT NOT NULL,
    access_hash       TEXT NOT NULL UNIQUE,
    access_expires_at INTEGER NOT NULL,
    refresh_hash      TEXT NOT NULL UNIQUE,
    prev_refresh_hash TEXT,
    prev_valid_until  INTEGER,
    created_at        INTEGER NOT NULL
);
-- What assistants did: every tool call, with its arguments.
CREATE TABLE IF NOT EXISTS assistant_log (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    device_id  TEXT NOT NULL,
    assistant  TEXT NOT NULL,
    at         INTEGER NOT NULL,
    tool       TEXT NOT NULL,
    arguments  TEXT NOT NULL,
    ok         INTEGER NOT NULL,
    error      TEXT
);
CREATE INDEX IF NOT EXISTS assistant_log_at ON assistant_log(at);
"#;

pub fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(SCHEMA)
}

// ----- where this server is reachable -------------------------------------------------------

/// The server's address as clients see it, without a trailing slash: `public_url` from the config,
/// or else what the request says. Behind a TLS proxy that is `https://<Host>`.
pub fn base_url(state: &AppState, headers: &HeaderMap) -> String {
    if let Some(url) = state.config.public_url.as_deref().map(|u| u.trim().trim_end_matches('/')).filter(|u| !u.is_empty()) {
        return url.to_string();
    }
    let get = |name: &str| headers.get(name).and_then(|v| v.to_str().ok()).map(|v| v.split(',').next().unwrap_or(v).trim().to_string());
    let forwarded = |name: &str| if state.config.trust_proxy { get(name) } else { None };
    let host = forwarded("x-forwarded-host").or_else(|| get("host")).unwrap_or_else(|| state.config.bind.to_string());
    let name = host.rsplit_once(':').map_or(host.as_str(), |(h, port)| if port.chars().all(|c| c.is_ascii_digit()) { h } else { host.as_str() });
    let loopback = matches!(name, "localhost" | "127.0.0.1" | "[::1]");
    // The server only speaks plain HTTP to a proxy or to this machine.
    let scheme = forwarded("x-forwarded-proto").filter(|p| p == "http" || p == "https").unwrap_or_else(|| if loopback { "http" } else { "https" }.into());
    format!("{scheme}://{host}")
}

fn no_store<T: IntoResponse>(body: T) -> Response {
    let mut resp = body.into_response();
    resp.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    resp
}

// ----- discovery ----------------------------------------------------------------------------

/// RFC 9728: tells a client that got a 401 from `/mcp` where to sign in.
pub async fn resource_metadata(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Json<serde_json::Value> {
    let base = base_url(&state, &headers);
    Json(json!({
        "resource": format!("{base}/mcp"),
        "authorization_servers": [base],
        "scopes_supported": [SCOPE_READ, SCOPE_WRITE],
        "bearer_methods_supported": ["header"],
        "resource_name": "CigMail",
    }))
}

/// RFC 8414.
pub async fn server_metadata(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Json<serde_json::Value> {
    let base = base_url(&state, &headers);
    Json(json!({
        "issuer": base,
        "authorization_endpoint": format!("{base}/oauth/authorize"),
        "token_endpoint": format!("{base}/oauth/token"),
        "registration_endpoint": format!("{base}/oauth/register"),
        "revocation_endpoint": format!("{base}/oauth/revoke"),
        "response_types_supported": ["code"],
        "response_modes_supported": ["query"],
        "grant_types_supported": ["authorization_code", "refresh_token"],
        "code_challenge_methods_supported": ["S256"],
        "token_endpoint_auth_methods_supported": ["none", "client_secret_post", "client_secret_basic"],
        "scopes_supported": [SCOPE_READ, SCOPE_WRITE],
    }))
}

// ----- clients ------------------------------------------------------------------------------

struct Client {
    id: String,
    name: String,
    redirect_uris: Vec<String>,
    secret_hash: Option<String>,
}

fn client(conn: &Connection, id: &str) -> rusqlite::Result<Option<Client>> {
    conn.query_row("SELECT id, name, redirect_uris, secret_hash FROM oauth_clients WHERE id = ?1", [id], |r| {
        let uris: String = r.get(2)?;
        Ok(Client { id: r.get(0)?, name: r.get(1)?, redirect_uris: serde_json::from_str(&uris).unwrap_or_default(), secret_hash: r.get(3)? })
    })
    .optional()
}

/// (scheme, host, the rest) of an absolute URI.
fn split_uri(uri: &str) -> Option<(String, String, String)> {
    let (scheme, rest) = uri.split_once(':')?;
    let ok = !scheme.is_empty()
        && scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        && scheme.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    if !ok || uri.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return None;
    }
    let scheme = scheme.to_ascii_lowercase();
    let Some(after) = rest.strip_prefix("//") else { return Some((scheme, String::new(), rest.to_string())) };
    let end = after.find(['/', '?', '#']).unwrap_or(after.len());
    let authority = &after[..end];
    // No user info: `https://claude.ai@evil.example` must not look like claude.ai.
    if authority.contains('@') {
        return None;
    }
    Some((scheme, authority.to_ascii_lowercase(), after[end..].to_string()))
}

fn host_name(authority: &str) -> &str {
    match authority.rsplit_once(':') {
        Some((h, port)) if port.chars().all(|c| c.is_ascii_digit()) => h,
        _ => authority,
    }
}

fn is_loopback(authority: &str) -> bool {
    matches!(host_name(authority), "localhost" | "127.0.0.1" | "[::1]")
}

/// Where a sign-in may send the user back to: an HTTPS page, a port on their own machine, or an
/// app's own scheme. Never something a browser would run or a plain-HTTP site.
fn valid_redirect_uri(uri: &str) -> bool {
    let Some((scheme, authority, _)) = split_uri(uri) else { return false };
    if uri.contains('#') || uri.len() > 2000 {
        return false;
    }
    match scheme.as_str() {
        "https" => !authority.is_empty(),
        "http" => is_loopback(&authority),
        "javascript" | "data" | "file" | "vbscript" | "about" | "blob" | "ws" | "wss" | "ftp" => false,
        _ => true,
    }
}

/// Exact match, except that a loopback address may use any port (RFC 8252: native apps listen on
/// whatever port is free).
fn redirect_registered(registered: &[String], given: &str) -> bool {
    if registered.iter().any(|r| r == given) {
        return true;
    }
    let Some((scheme, authority, rest)) = split_uri(given) else { return false };
    scheme == "http"
        && is_loopback(&authority)
        && registered.iter().filter_map(|r| split_uri(r)).any(|(s, a, r)| s == "http" && is_loopback(&a) && host_name(&a) == host_name(&authority) && r == rest)
}

/// Who the user is sent back to, for the sign-in page.
fn redirect_target(uri: &str) -> String {
    match split_uri(uri) {
        Some((scheme, authority, _)) if scheme == "http" || scheme == "https" => {
            if is_loopback(&authority) { "an app on this computer".into() } else { host_name(&authority).to_string() }
        }
        Some((scheme, ..)) => format!("the app that opens {scheme}:// links"),
        None => "an unknown address".into(),
    }
}

#[derive(Deserialize)]
pub struct Registration {
    #[serde(default)]
    redirect_uris: Vec<String>,
    client_name: Option<String>,
    token_endpoint_auth_method: Option<String>,
}

#[derive(Serialize)]
struct OAuthError {
    error: &'static str,
    error_description: String,
}

fn oauth_error(status: StatusCode, error: &'static str, description: impl Into<String>) -> Response {
    no_store((status, Json(OAuthError { error, error_description: description.into() })))
}

/// RFC 7591: an assistant introduces itself. Anyone may; a client can't do anything until a user
/// signs in for it, and the sign-in page shows where the user will be sent.
pub async fn register(
    State(state): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(req): Json<Registration>,
) -> Response {
    let key = format!("oauth-register:{}", client_ip(&state, &headers, peer));
    {
        let mut limiter = state.limiter.lock();
        if !limiter.allowed(&key, MAX_REGISTRATIONS_PER_IP, REGISTER_WINDOW) {
            return oauth_error(StatusCode::TOO_MANY_REQUESTS, "invalid_request", "too many registrations from here; try again later");
        }
        limiter.hit(&key);
    }
    if req.redirect_uris.is_empty() || req.redirect_uris.len() > 20 || !req.redirect_uris.iter().all(|u| valid_redirect_uri(u)) {
        return oauth_error(
            StatusCode::BAD_REQUEST,
            "invalid_redirect_uri",
            "redirect_uris must be HTTPS addresses, loopback HTTP addresses or an app's own scheme",
        );
    }
    let name: String = req.client_name.as_deref().unwrap_or("").trim().chars().filter(|c| !c.is_control()).take(60).collect();
    let name = if name.is_empty() { "An assistant".to_string() } else { name };
    let method = match req.token_endpoint_auth_method.as_deref() {
        None | Some("none") => "none",
        Some("client_secret_post") => "client_secret_post",
        Some("client_secret_basic") => "client_secret_basic",
        Some(other) => return oauth_error(StatusCode::BAD_REQUEST, "invalid_client_metadata", format!("unsupported token_endpoint_auth_method {other}")),
    };
    let id = uuid::Uuid::new_v4().simple().to_string();
    let secret = (method != "none").then(auth::new_token);
    let now = now_ms();
    let stored = {
        let db = state.db.lock();
        db.execute(
            "DELETE FROM oauth_clients WHERE created_at < ?1 AND NOT EXISTS (SELECT 1 FROM oauth_grants g WHERE g.client_id = oauth_clients.id)",
            [now - UNUSED_CLIENT_TTL_MS],
        )
        .and_then(|_| {
            db.execute(
                "INSERT INTO oauth_clients (id, name, redirect_uris, secret_hash, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![id, name, serde_json::to_string(&req.redirect_uris).unwrap_or_default(), secret.as_deref().map(auth::token_hash), now],
            )
        })
    };
    if let Err(e) = stored {
        eprintln!("cigmail-server: registering an assistant: {e}");
        return oauth_error(StatusCode::INTERNAL_SERVER_ERROR, "server_error", "could not register the client");
    }
    let mut body = json!({
        "client_id": id,
        "client_name": name,
        "redirect_uris": req.redirect_uris,
        "grant_types": ["authorization_code", "refresh_token"],
        "response_types": ["code"],
        "token_endpoint_auth_method": method,
        "client_id_issued_at": now / 1000,
    });
    if let Some(secret) = secret {
        body["client_secret"] = json!(secret);
        body["client_secret_expires_at"] = json!(0);
    }
    no_store((StatusCode::CREATED, Json(body)))
}

// ----- the sign-in page ---------------------------------------------------------------------

#[derive(Deserialize, Default, Clone)]
pub struct AuthorizeParams {
    #[serde(default)]
    response_type: String,
    #[serde(default)]
    client_id: String,
    #[serde(default)]
    redirect_uri: String,
    #[serde(default)]
    code_challenge: String,
    #[serde(default)]
    code_challenge_method: String,
    state: Option<String>,
    scope: Option<String>,
    // Only from the form:
    #[serde(default)]
    password: String,
    /// "1": the assistant may change things too.
    allow_changes: Option<String>,
    /// "deny": the user said no.
    decision: Option<String>,
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&#39;")
}

fn page(status: StatusCode, body: String) -> Response {
    let html = format!(
        r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="robots" content="noindex"><title>CigMail</title>
<style>
:root {{ color-scheme: dark; }}
body {{ margin: 0; min-height: 100vh; display: grid; place-items: center; background: #0e0f12; color: #e6e7ea;
  font: 15px/1.5 system-ui, -apple-system, "Segoe UI", sans-serif; }}
main {{ width: min(380px, calc(100vw - 32px)); padding: 28px; background: #16181d; border: 1px solid #262a33; border-radius: 12px; }}
h1 {{ font-size: 18px; margin: 0 0 6px; }}
p {{ margin: 0 0 14px; color: #a3a8b4; }}
b {{ color: #e6e7ea; }}
label {{ display: block; margin: 12px 0 4px; font-size: 13px; color: #a3a8b4; }}
input[type=password] {{ width: 100%; box-sizing: border-box; padding: 9px 11px; font: inherit; color: inherit;
  background: #0e0f12; border: 1px solid #2c313c; border-radius: 8px; }}
input:focus {{ outline: 2px solid #5e6ad2; outline-offset: 0; border-color: transparent; }}
.check {{ display: flex; gap: 9px; align-items: flex-start; margin: 16px 0 4px; color: #e6e7ea; font-size: 14px; }}
.check small {{ display: block; color: #a3a8b4; }}
.row {{ display: flex; gap: 10px; margin-top: 20px; }}
button {{ flex: 1; padding: 10px; font: inherit; font-weight: 600; border-radius: 8px; border: 1px solid #2c313c; background: transparent; color: #e6e7ea; cursor: pointer; }}
button.primary {{ background: #5e6ad2; border-color: #5e6ad2; color: #fff; }}
.error {{ margin: 0 0 14px; padding: 9px 11px; border-radius: 8px; background: #3a1d22; color: #ffb4bd; }}
ul {{ margin: 0 0 14px; padding-left: 20px; color: #a3a8b4; }}
</style></head><body><main>{body}</main></body></html>"#
    );
    let mut resp = (status, Html(html)).into_response();
    let h = resp.headers_mut();
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    // The page must not be framed by another site asking for a password in our name.
    h.insert("x-frame-options", HeaderValue::from_static("DENY"));
    h.insert("content-security-policy", HeaderValue::from_static("default-src 'none'; style-src 'unsafe-inline'; frame-ancestors 'none'; base-uri 'none'"));
    h.insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    resp
}

fn error_page(status: StatusCode, title: &str, text: &str) -> Response {
    page(status, format!("<h1>{}</h1><p>{}</p>", esc(title), esc(text)))
}

fn sign_in_page(status: StatusCode, p: &AuthorizeParams, client: &Client, error: Option<&str>) -> Response {
    let hidden = [
        ("response_type", p.response_type.as_str()),
        ("client_id", p.client_id.as_str()),
        ("redirect_uri", p.redirect_uri.as_str()),
        ("code_challenge", p.code_challenge.as_str()),
        ("code_challenge_method", p.code_challenge_method.as_str()),
        ("state", p.state.as_deref().unwrap_or("")),
        ("scope", p.scope.as_deref().unwrap_or("")),
    ]
    .iter()
    .map(|(k, v)| format!(r#"<input type="hidden" name="{k}" value="{}">"#, esc(v)))
    .collect::<String>();
    let error = error.map(|e| format!(r#"<div class="error">{}</div>"#, esc(e))).unwrap_or_default();
    // Offered unless the assistant only asked to read.
    let asked_write = p.scope.as_deref().is_none_or(|s| s.trim().is_empty() || s.split_whitespace().any(|x| x == SCOPE_WRITE));
    let changes = if asked_write {
        let checked = if error.is_empty() || p.allow_changes.is_some() { " checked" } else { "" };
        format!(
            r#"<label class="check"><input type="checkbox" name="allow_changes" value="1"{checked}>
<span>Allow changes<small>Mark conversations done, read or starred, snooze them, and save drafts. Without this it can only read. It can never send mail.</small></span></label>"#
        )
    } else {
        String::new()
    };
    page(
        status,
        format!(
            r#"<h1>Connect {name} to CigMail</h1>
<p><b>{name}</b> wants to read your mail. After signing in you are sent back to <b>{target}</b>.</p>
{error}<form method="post" action="/oauth/authorize">{hidden}
<label for="password">Server password</label><input id="password" type="password" name="password" autocomplete="current-password" required autofocus>
{changes}
<div class="row"><button type="submit" name="decision" value="deny" formnovalidate>Cancel</button>
<button class="primary" type="submit" name="decision" value="allow">Connect</button></div></form>"#,
            name = esc(&client.name),
            target = esc(&redirect_target(&p.redirect_uri)),
        ),
    )
}

fn redirect_with(uri: &str, params: &[(&str, &str)]) -> Response {
    let mut url = uri.to_string();
    let mut sep = if url.contains('?') { '&' } else { '?' };
    for (k, v) in params {
        url.push(sep);
        sep = '&';
        url.push_str(k);
        url.push('=');
        url.push_str(&urlencode(v));
    }
    match HeaderValue::from_str(&url) {
        Ok(location) => no_store((StatusCode::SEE_OTHER, [(header::LOCATION, location)])),
        Err(_) => error_page(StatusCode::BAD_REQUEST, "Can't go back to the assistant", "Its return address is not valid."),
    }
}

fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Checks the parts of a request the user can't be sent back for: an unknown client or a return
/// address it didn't register. Everything else is reported to the client by redirect.
fn check_request(state: &AppState, p: &AuthorizeParams) -> Result<Client, Response> {
    let found = client(&state.db.lock(), &p.client_id).map_err(|e| {
        eprintln!("cigmail-server: {e}");
        error_page(StatusCode::INTERNAL_SERVER_ERROR, "Something went wrong", "Try connecting again.")
    })?;
    let Some(client) = found else {
        return Err(error_page(
            StatusCode::BAD_REQUEST,
            "Unknown assistant",
            "This sign-in link is from an app this server doesn't know (any more). Remove the connection in the assistant and add it again.",
        ));
    };
    if !redirect_registered(&client.redirect_uris, &p.redirect_uri) {
        return Err(error_page(
            StatusCode::BAD_REQUEST,
            "This sign-in link is not valid",
            "It would send you to an address the assistant didn't register. Nothing was shared.",
        ));
    }
    Ok(client)
}

/// Problems the client caused, sent back to it.
fn check_parameters(p: &AuthorizeParams) -> Result<(), Response> {
    let state = p.state.as_deref().unwrap_or("");
    let fail = |error: &str, description: &str| {
        let mut params = vec![("error", error), ("error_description", description)];
        if !state.is_empty() {
            params.push(("state", state));
        }
        Err(redirect_with(&p.redirect_uri, &params))
    };
    if p.response_type != "code" {
        return fail("unsupported_response_type", "only response_type=code is supported");
    }
    if p.code_challenge_method != "S256" || p.code_challenge.len() < 43 || p.code_challenge.len() > 128 {
        return fail("invalid_request", "PKCE with code_challenge_method=S256 is required");
    }
    Ok(())
}

pub async fn authorize_page(State(state): State<Arc<AppState>>, Query(p): Query<AuthorizeParams>) -> Response {
    let client = match check_request(&state, &p) {
        Ok(c) => c,
        Err(resp) => return resp,
    };
    if let Err(resp) = check_parameters(&p) {
        return resp;
    }
    let p = AuthorizeParams { password: String::new(), ..p };
    sign_in_page(StatusCode::OK, &p, &client, None)
}

pub async fn authorize(
    State(state): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Form(p): Form<AuthorizeParams>,
) -> Response {
    let client = match check_request(&state, &p) {
        Ok(c) => c,
        Err(resp) => return resp,
    };
    if let Err(resp) = check_parameters(&p) {
        return resp;
    }
    let oauth_state = p.state.clone().unwrap_or_default();
    if p.decision.as_deref() == Some("deny") {
        let mut params = vec![("error", "access_denied"), ("error_description", "the user cancelled")];
        if !oauth_state.is_empty() {
            params.push(("state", &oauth_state));
        }
        return redirect_with(&p.redirect_uri, &params);
    }
    let scope = if p.allow_changes.is_some() { format!("{SCOPE_READ} {SCOPE_WRITE}") } else { SCOPE_READ.to_string() };
    let ip = client_ip(&state, &headers, peer);
    let (password, challenge, redirect_uri, client_id) = (p.password.clone(), p.code_challenge.clone(), p.redirect_uri.clone(), client.id.clone());
    let s = state.clone();
    // `Ok(None)`: wrong password. `Err(None)`: too many wrong ones.
    let code = tokio::task::spawn_blocking(move || -> Result<Option<String>, Option<anyhow::Error>> {
        let db = s.db.lock();
        match auth::sign_in(&db, &s.limiter, &ip, &password).map_err(|e| Some(e.into()))? {
            Err(()) => return Err(None),
            Ok(false) => return Ok(None),
            Ok(true) => {}
        }
        let code = auth::new_token();
        let now = now_ms();
        let store = || -> rusqlite::Result<()> {
            db.execute("DELETE FROM oauth_codes WHERE expires_at < ?1", [now])?;
            db.execute(
                "INSERT INTO oauth_codes (code_hash, client_id, redirect_uri, challenge, scope, expires_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![auth::token_hash(&code), client_id, redirect_uri, challenge, scope, now + CODE_TTL_MS],
            )?;
            Ok(())
        };
        store().map_err(|e| Some(e.into()))?;
        Ok(Some(code))
    })
    .await;
    match code {
        Ok(Ok(Some(code))) => {
            let mut params = vec![("code", code.as_str())];
            if !oauth_state.is_empty() {
                params.push(("state", &oauth_state));
            }
            redirect_with(&p.redirect_uri, &params)
        }
        Ok(Ok(None)) => sign_in_page(StatusCode::UNAUTHORIZED, &p, &client, Some("Wrong password.")),
        Ok(Err(None)) => sign_in_page(StatusCode::TOO_MANY_REQUESTS, &p, &client, Some("Too many failed sign-ins. Wait a few minutes.")),
        Ok(Err(Some(e))) => {
            eprintln!("cigmail-server: assistant sign-in: {e:#}");
            sign_in_page(StatusCode::INTERNAL_SERVER_ERROR, &p, &client, Some("Something went wrong. Try again."))
        }
        Err(e) => {
            eprintln!("cigmail-server: assistant sign-in: {e}");
            sign_in_page(StatusCode::INTERNAL_SERVER_ERROR, &p, &client, Some("Something went wrong. Try again."))
        }
    }
}

// ----- tokens -------------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct TokenRequest {
    #[serde(default)]
    grant_type: String,
    code: Option<String>,
    code_verifier: Option<String>,
    redirect_uri: Option<String>,
    client_id: Option<String>,
    client_secret: Option<String>,
    refresh_token: Option<String>,
    token: Option<String>,
}

/// The client id and secret from the form or from HTTP Basic.
fn client_credentials(headers: &HeaderMap, req: &TokenRequest) -> (Option<String>, Option<String>) {
    let basic = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Basic "))
        .and_then(|v| STANDARD.decode(v.trim()).ok())
        .and_then(|v| String::from_utf8(v).ok())
        .and_then(|v| v.split_once(':').map(|(id, secret)| (id.to_string(), secret.to_string())));
    match basic {
        Some((id, secret)) => (Some(id), Some(secret)),
        None => (req.client_id.clone(), req.client_secret.clone()),
    }
}

fn pkce_matches(verifier: &str, challenge: &str) -> bool {
    (43..=128).contains(&verifier.len()) && URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes())) == challenge
}

fn token_response(access: &str, refresh: &str, scope: &str) -> Response {
    no_store(Json(json!({
        "access_token": access,
        "token_type": "Bearer",
        "expires_in": ACCESS_TTL_MS / 1000,
        "refresh_token": refresh,
        "scope": scope,
    })))
}

type TokenResult = Result<Response, (&'static str, &'static str)>;

fn exchange_code(conn: &Connection, client: &Client, req: &TokenRequest) -> rusqlite::Result<TokenResult> {
    let (Some(code), Some(verifier)) = (req.code.as_deref(), req.code_verifier.as_deref()) else {
        return Ok(Err(("invalid_request", "code and code_verifier are required")));
    };
    let now = now_ms();
    let hash = auth::token_hash(code);
    let row: Option<(String, String, String, String, i64)> = conn
        .query_row(
            "SELECT client_id, redirect_uri, challenge, scope, expires_at FROM oauth_codes WHERE code_hash = ?1",
            [&hash],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .optional()?;
    // A code works once, whatever comes of it.
    conn.execute("DELETE FROM oauth_codes WHERE code_hash = ?1", [&hash])?;
    let Some((client_id, redirect_uri, challenge, scope, expires_at)) = row else {
        return Ok(Err(("invalid_grant", "the code is unknown or was already used")));
    };
    if client_id != client.id || expires_at < now {
        return Ok(Err(("invalid_grant", "the code expired or belongs to another client")));
    }
    if req.redirect_uri.as_deref().is_some_and(|u| u != redirect_uri) {
        return Ok(Err(("invalid_grant", "redirect_uri differs from the one the code was issued for")));
    }
    if !pkce_matches(verifier, &challenge) {
        return Ok(Err(("invalid_grant", "code_verifier doesn't match the code_challenge")));
    }
    let (device_id, _unused_token) = auth::add_device(conn, "assistant", &client.name)?;
    let (access, refresh) = (auth::new_token(), auth::new_token());
    conn.execute(
        "INSERT INTO oauth_grants (id, device_id, client_id, scope, access_hash, access_expires_at, refresh_hash, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            uuid::Uuid::new_v4().simple().to_string(),
            device_id,
            client.id,
            scope,
            auth::token_hash(&access),
            now + ACCESS_TTL_MS,
            auth::token_hash(&refresh),
            now
        ],
    )?;
    Ok(Ok(token_response(&access, &refresh, &scope)))
}

fn refresh(conn: &Connection, client: &Client, req: &TokenRequest) -> rusqlite::Result<TokenResult> {
    let Some(token) = req.refresh_token.as_deref() else {
        return Ok(Err(("invalid_request", "refresh_token is required")));
    };
    let now = now_ms();
    let hash = auth::token_hash(token);
    let row: Option<(String, String, String)> = conn
        .query_row(
            "SELECT g.id, g.scope, g.client_id FROM oauth_grants g JOIN devices d ON d.id = g.device_id
             WHERE d.revoked_at IS NULL AND (g.refresh_hash = ?1 OR (g.prev_refresh_hash = ?1 AND g.prev_valid_until > ?2))",
            params![hash, now],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    let Some((grant_id, scope, _)) = row.filter(|r| r.2 == client.id) else {
        return Ok(Err(("invalid_grant", "the refresh token is unknown, was replaced or the assistant was disconnected")));
    };
    let (access, refresh) = (auth::new_token(), auth::new_token());
    conn.execute(
        "UPDATE oauth_grants SET access_hash = ?2, access_expires_at = ?3, prev_refresh_hash = refresh_hash,
         prev_valid_until = ?4, refresh_hash = ?5 WHERE id = ?1",
        params![grant_id, auth::token_hash(&access), now + ACCESS_TTL_MS, now + REFRESH_GRACE_MS, auth::token_hash(&refresh)],
    )?;
    Ok(Ok(token_response(&access, &refresh, &scope)))
}

/// The client a token request comes from, with its secret checked if it has one.
fn authenticate_client(conn: &Connection, headers: &HeaderMap, req: &TokenRequest) -> rusqlite::Result<Option<Client>> {
    let (id, secret) = client_credentials(headers, req);
    let Some(found) = id.map(|id| client(conn, &id)).transpose()?.flatten() else { return Ok(None) };
    if let Some(expected) = &found.secret_hash
        && secret.as_deref().map(auth::token_hash).as_ref() != Some(expected)
    {
        return Ok(None);
    }
    Ok(Some(found))
}

pub async fn token(State(state): State<Arc<AppState>>, headers: HeaderMap, Form(req): Form<TokenRequest>) -> Response {
    let result = tokio::task::spawn_blocking(move || -> rusqlite::Result<TokenResult> {
        let db = state.db.lock();
        let Some(client) = authenticate_client(&db, &headers, &req)? else {
            return Ok(Err(("invalid_client", "unknown client or wrong client secret")));
        };
        let tx = db.unchecked_transaction()?;
        let out = match req.grant_type.as_str() {
            "authorization_code" => exchange_code(&db, &client, &req)?,
            "refresh_token" => refresh(&db, &client, &req)?,
            _ => Err(("unsupported_grant_type", "only authorization_code and refresh_token are supported")),
        };
        tx.commit()?;
        Ok(out)
    })
    .await;
    match result {
        Ok(Ok(Ok(resp))) => resp,
        Ok(Ok(Err((error, description)))) => {
            let status = if error == "invalid_client" { StatusCode::UNAUTHORIZED } else { StatusCode::BAD_REQUEST };
            oauth_error(status, error, description)
        }
        Ok(Err(e)) => {
            eprintln!("cigmail-server: issuing a token: {e}");
            oauth_error(StatusCode::INTERNAL_SERVER_ERROR, "server_error", "could not issue a token")
        }
        Err(e) => {
            eprintln!("cigmail-server: issuing a token: {e}");
            oauth_error(StatusCode::INTERNAL_SERVER_ERROR, "server_error", "could not issue a token")
        }
    }
}

/// RFC 7009: the assistant disconnects itself. Answers 200 whether or not the token existed.
pub async fn revoke(State(state): State<Arc<AppState>>, Form(req): Form<TokenRequest>) -> Response {
    let Some(token) = req.token else { return oauth_error(StatusCode::BAD_REQUEST, "invalid_request", "token is required") };
    let hash = auth::token_hash(&token);
    let revoked = (|| -> rusqlite::Result<()> {
        let db = state.db.lock();
        let device: Option<String> = db
            .query_row("SELECT device_id FROM oauth_grants WHERE access_hash = ?1 OR refresh_hash = ?1", [&hash], |r| r.get(0))
            .optional()?;
        if let Some(device) = device {
            auth::revoke_device(&db, &device)?;
        }
        Ok(())
    })();
    if let Err(e) = revoked {
        eprintln!("cigmail-server: revoking a token: {e}");
    }
    no_store(StatusCode::OK)
}

// ----- what `/mcp` needs ----------------------------------------------------------------------

/// A connected assistant, from its access token.
#[derive(Debug, Clone)]
pub struct Assistant {
    pub device_id: String,
    /// The name the assistant registered with, e.g. "Claude".
    pub name: String,
    pub can_write: bool,
}

/// Whether each connected assistant may change things, by device id.
pub fn write_access(conn: &Connection) -> rusqlite::Result<std::collections::HashMap<String, bool>> {
    conn.prepare("SELECT device_id, scope FROM oauth_grants")?
        .query_map([], |r| {
            let scope: String = r.get(1)?;
            Ok((r.get(0)?, scope.split_whitespace().any(|s| s == SCOPE_WRITE)))
        })?
        .collect()
}

pub fn assistant_by_token(conn: &Connection, token: &str) -> rusqlite::Result<Option<Assistant>> {
    let now = now_ms();
    let found: Option<(Assistant, Option<i64>)> = conn
        .query_row(
            "SELECT g.device_id, c.name, g.scope, d.last_seen_at FROM oauth_grants g
             JOIN devices d ON d.id = g.device_id JOIN oauth_clients c ON c.id = g.client_id
             WHERE g.access_hash = ?1 AND g.access_expires_at > ?2 AND d.revoked_at IS NULL",
            params![auth::token_hash(token), now],
            |r| {
                let scope: String = r.get(2)?;
                let can_write = scope.split_whitespace().any(|s| s == SCOPE_WRITE);
                Ok((Assistant { device_id: r.get(0)?, name: r.get(1)?, can_write }, r.get(3)?))
            },
        )
        .optional()?;
    let Some((assistant, seen)) = found else { return Ok(None) };
    if seen.is_none_or(|s| now - s > 60_000) {
        conn.execute("UPDATE devices SET last_seen_at = ?2 WHERE id = ?1", params![assistant.device_id, now])?;
    }
    Ok(Some(assistant))
}

pub fn record_call(conn: &Connection, who: &Assistant, tool: &str, arguments: &serde_json::Value, error: Option<&str>) -> rusqlite::Result<()> {
    let now = now_ms();
    let mut arguments = arguments.to_string();
    if arguments.len() > 4000 {
        let cut = (0..=4000).rev().find(|i| arguments.is_char_boundary(*i)).unwrap_or(0);
        arguments.truncate(cut);
        arguments.push('…');
    }
    conn.execute(
        "INSERT INTO assistant_log (device_id, assistant, at, tool, arguments, ok, error) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![who.device_id, who.name, now, tool, arguments, error.is_none(), error],
    )?;
    conn.execute("DELETE FROM assistant_log WHERE at < ?1", [now - LOG_RETENTION_MS])?;
    Ok(())
}

/// One thing an assistant did.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    pub at: i64,
    pub assistant: String,
    pub device_id: String,
    pub tool: String,
    pub arguments: serde_json::Value,
    pub ok: bool,
    pub error: Option<String>,
}

#[derive(Deserialize)]
pub struct LogQuery {
    limit: Option<usize>,
}

/// `GET /v1/assistants/log`: what assistants did, newest first.
pub async fn activity(State(state): State<Arc<AppState>>, _me: App, Query(q): Query<LogQuery>) -> ApiResult<Json<Vec<LogEntry>>> {
    let limit = q.limit.unwrap_or(100).clamp(1, 1000) as i64;
    blocking(move || {
        let db = state.db.lock();
        let rows = db
            .prepare("SELECT at, assistant, device_id, tool, arguments, ok, error FROM assistant_log ORDER BY id DESC LIMIT ?1")?
            .query_map([limit], |r| {
                let arguments: String = r.get(4)?;
                Ok(LogEntry {
                    at: r.get(0)?,
                    assistant: r.get(1)?,
                    device_id: r.get(2)?,
                    tool: r.get(3)?,
                    arguments: serde_json::from_str(&arguments).unwrap_or(serde_json::Value::String(arguments)),
                    ok: r.get(5)?,
                    error: r.get(6)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok::<_, ApiError>(Json(rows))
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redirect_uris() {
        assert!(valid_redirect_uri("https://claude.ai/api/mcp/auth_callback"));
        assert!(valid_redirect_uri("http://localhost:3334/callback"));
        assert!(valid_redirect_uri("http://127.0.0.1/callback"));
        assert!(valid_redirect_uri("cursor://anysphere.cursor-mcp/oauth/callback"));
        assert!(!valid_redirect_uri("http://example.org/callback"), "plain HTTP only on this machine");
        assert!(!valid_redirect_uri("javascript:alert(1)"));
        assert!(!valid_redirect_uri("data:text/html,hi"));
        assert!(!valid_redirect_uri("https://claude.ai@evil.example/cb"), "user info hides the real host");
        assert!(!valid_redirect_uri("https://a.example/cb#frag"));
        assert!(!valid_redirect_uri("/relative"));

        let registered = vec!["https://claude.ai/api/mcp/auth_callback".to_string(), "http://localhost/callback".to_string()];
        assert!(redirect_registered(&registered, "https://claude.ai/api/mcp/auth_callback"));
        assert!(!redirect_registered(&registered, "https://claude.ai/api/mcp/auth_callback/x"));
        assert!(!redirect_registered(&registered, "https://claude.ai.evil.example/api/mcp/auth_callback"));
        assert!(redirect_registered(&registered, "http://localhost:51234/callback"), "any port on loopback");
        assert!(!redirect_registered(&registered, "http://localhost:51234/other"));
        assert!(!redirect_registered(&registered, "http://127.0.0.1:51234/callback"), "a different host name");

        assert_eq!(redirect_target("https://claude.ai/api/mcp/auth_callback"), "claude.ai");
        assert_eq!(redirect_target("http://localhost:1234/cb"), "an app on this computer");
    }

    #[test]
    fn pkce() {
        // The challenge is the URL-safe base64 of the verifier's SHA-256, worked out with Python.
        let (verifier, challenge) = ("dBjftJeZ4CVP-mKxrNdbk3-jl2-C5ESvpoGUDDeWr1Lk", "88fUqDugiMisAuDnpy7D3Qk9rNr-BOhkOgf3LowNQsE");
        assert!(pkce_matches(verifier, challenge));
        assert!(!pkce_matches(&verifier.replace("Lk", "Lx"), challenge));
        assert!(!pkce_matches("short", challenge));
    }

    #[test]
    fn escaping() {
        assert_eq!(esc(r#"<b a="1">&'"#), "&lt;b a=&quot;1&quot;&gt;&amp;&#39;");
        assert_eq!(urlencode("a b&c=d/é"), "a%20b%26c%3Dd%2F%C3%A9");
    }
}
