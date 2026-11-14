//! The HTTP API for the app: pairing, handing over the mail accounts, and seeing who is connected.
//! The routes for assistants (`/mcp`, `/oauth/...`) have their own sign-in, see `oauth`.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::{ConnectInfo, DefaultBodyLimit, FromRequestParts, Path, State};
use axum::http::request::Parts;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use cigmail_core::models::Account;
use serde::{Deserialize, Serialize};

use crate::{AppState, auth, mcp, oauth};

pub fn router(state: Arc<AppState>) -> Router {
    // Assistants running in a browser (and tools for trying a server out) call these from another
    // origin. They carry a bearer token, never a cookie, so any origin may.
    let assistants = Router::new()
        .route("/mcp", post(mcp::post).get(mcp::get).options(preflight))
        .route("/.well-known/oauth-protected-resource", get(oauth::resource_metadata).options(preflight))
        .route("/.well-known/oauth-protected-resource/mcp", get(oauth::resource_metadata).options(preflight))
        .route("/.well-known/oauth-authorization-server", get(oauth::server_metadata).options(preflight))
        .route("/.well-known/oauth-authorization-server/mcp", get(oauth::server_metadata).options(preflight))
        .route("/oauth/register", post(oauth::register).options(preflight))
        .route("/oauth/token", post(oauth::token).options(preflight))
        .route("/oauth/revoke", post(oauth::revoke).options(preflight))
        .layer(axum::middleware::map_response(any_origin));
    Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/v1/pair", post(pair))
        .route("/v1/unpair", post(unpair))
        .route("/v1/status", get(status))
        .route("/v1/accounts", put(put_accounts))
        .route("/v1/accounts/{id}", delete(delete_account))
        .route("/v1/devices/{id}", delete(revoke_device))
        .route("/v1/assistants/log", get(oauth::activity))
        .route("/oauth/authorize", get(oauth::authorize_page).post(oauth::authorize))
        .merge(assistants)
        .layer(DefaultBodyLimit::max(4 << 20))
        .with_state(state)
}

async fn preflight() -> StatusCode {
    StatusCode::NO_CONTENT
}

async fn any_origin(mut resp: Response) -> Response {
    let h = resp.headers_mut();
    h.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, header::HeaderValue::from_static("*"));
    h.insert(header::ACCESS_CONTROL_ALLOW_METHODS, header::HeaderValue::from_static("GET, POST, OPTIONS"));
    h.insert(header::ACCESS_CONTROL_ALLOW_HEADERS, header::HeaderValue::from_static("authorization, content-type, mcp-protocol-version, mcp-session-id"));
    h.insert(header::ACCESS_CONTROL_EXPOSE_HEADERS, header::HeaderValue::from_static("www-authenticate"));
    h.insert(header::ACCESS_CONTROL_MAX_AGE, header::HeaderValue::from_static("86400"));
    resp
}

#[derive(Serialize)]
struct ErrorBody {
    error: String,
}

pub struct ApiError(StatusCode, String);

impl ApiError {
    pub(crate) fn new(status: StatusCode, msg: impl Into<String>) -> Self {
        Self(status, msg.into())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        if self.0.is_server_error() {
            eprintln!("cigmail-server: {}", self.1);
        }
        (self.0, Json(ErrorBody { error: self.1 })).into_response()
    }
}

macro_rules! internal {
    ($($t:ty),*) => {$(
        impl From<$t> for ApiError {
            fn from(e: $t) -> Self {
                Self(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}"))
            }
        }
    )*};
}
internal!(anyhow::Error, rusqlite::Error, tokio::task::JoinError);

pub(crate) type ApiResult<T> = Result<T, ApiError>;

/// Runs database or CPU-heavy work off the async threads.
pub(crate) async fn blocking<T: Send + 'static>(f: impl FnOnce() -> ApiResult<T> + Send + 'static) -> ApiResult<T> {
    tokio::task::spawn_blocking(f).await?
}

/// A paired app, from its bearer token.
pub struct App {
    pub device_id: String,
}

impl FromRequestParts<Arc<AppState>> for App {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &Arc<AppState>) -> Result<Self, Self::Rejection> {
        let token = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .ok_or_else(|| ApiError::new(StatusCode::UNAUTHORIZED, "connect to the server first"))?;
        let device_id = auth::app_by_token(&state.db.lock(), token)?
            .ok_or_else(|| ApiError::new(StatusCode::UNAUTHORIZED, "this app was disconnected from the server"))?;
        Ok(Self { device_id })
    }
}

pub(crate) fn client_ip(state: &AppState, headers: &HeaderMap, peer: SocketAddr) -> String {
    if state.config.trust_proxy
        && let Some(ip) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()).and_then(|v| v.split(',').next())
    {
        return ip.trim().to_string();
    }
    peer.ip().to_string()
}

// ----- pairing --------------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PairRequest {
    password: String,
    /// Shown in the list of what's connected, e.g. "Laptop".
    #[serde(default)]
    device_name: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Paired {
    device_id: String,
    /// Shown once; the server only keeps its hash.
    token: String,
}

async fn pair(
    State(state): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(req): Json<PairRequest>,
) -> ApiResult<Json<Paired>> {
    let ip = client_ip(&state, &headers, peer);
    blocking(move || {
        let db = state.db.lock();
        if auth::password_hash(&db)?.is_none() {
            return Err(ApiError::new(StatusCode::SERVICE_UNAVAILABLE, "the server has no password yet; run `cigmail-server set-password` on it"));
        }
        match auth::sign_in(&db, &state.limiter, &ip, &req.password)? {
            Err(()) => Err(ApiError::new(StatusCode::TOO_MANY_REQUESTS, "too many failed sign-ins; wait a few minutes")),
            Ok(false) => Err(ApiError::new(StatusCode::UNAUTHORIZED, "wrong password")),
            Ok(true) => {
                let (device_id, token) = auth::add_device(&db, "app", &req.device_name)?;
                Ok(Json(Paired { device_id, token }))
            }
        }
    })
    .await
}

async fn unpair(State(state): State<Arc<AppState>>, me: App) -> ApiResult<StatusCode> {
    auth::revoke_device(&state.db.lock(), &me.device_id)?;
    Ok(StatusCode::NO_CONTENT)
}

// ----- status ---------------------------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AccountStatus {
    id: String,
    email: String,
    kind: String,
    /// Unix seconds.
    last_sync: Option<i64>,
    last_error: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Status {
    version: &'static str,
    /// The device asking.
    device_id: String,
    accounts: Vec<AccountStatus>,
    /// Paired apps and connected assistants.
    devices: Vec<auth::DeviceInfo>,
}

fn devices(state: &AppState) -> rusqlite::Result<Vec<auth::DeviceInfo>> {
    let db = state.db.lock();
    let write = oauth::write_access(&db)?;
    db.prepare("SELECT id, kind, name, created_at, last_seen_at FROM devices WHERE revoked_at IS NULL ORDER BY created_at")?
        .query_map([], |r| {
            let id: String = r.get(0)?;
            Ok(auth::DeviceInfo { can_write: write.get(&id).copied(), id, kind: r.get(1)?, name: r.get(2)?, created_at: r.get(3)?, last_seen_at: r.get(4)? })
        })?
        .collect()
}

async fn status(State(state): State<Arc<AppState>>, me: App) -> ApiResult<Json<Status>> {
    blocking(move || {
        let accounts = state
            .mail
            .accounts()?
            .into_iter()
            .map(|a| AccountStatus { id: a.id, email: a.email, kind: a.kind.as_str().into(), last_sync: a.last_sync, last_error: a.last_error })
            .collect();
        Ok(Json(Status { version: env!("CARGO_PKG_VERSION"), device_id: me.device_id, accounts, devices: devices(&state)? }))
    })
    .await
}

async fn revoke_device(State(state): State<Arc<AppState>>, _me: App, Path(id): Path<String>) -> ApiResult<StatusCode> {
    if !auth::revoke_device(&state.db.lock(), &id)? {
        return Err(ApiError::new(StatusCode::NOT_FOUND, "nothing connected under that id"));
    }
    Ok(StatusCode::NO_CONTENT)
}

// ----- accounts -------------------------------------------------------------------------------

/// Settings the providers need: which OAuth apps the tokens belong to, and how far back to sync.
const SETTINGS: &[&str] = &["google_client_id", "google_client_secret", "microsoft_client_id", "microsoft_tenant", "sync_days"];
/// The logins an account can have, stored as `<kind>:<account id>`.
const SECRETS: &[&str] = &["oauth", "password"];

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AccountBundle {
    account: Account,
    /// The provider's settings (the IMAP servers); `{}` for Gmail and Outlook.
    #[serde(default)]
    config: serde_json::Value,
    /// "oauth" (the tokens, as JSON text) and/or "password".
    #[serde(default)]
    secrets: HashMap<String, String>,
}

/// What the app hands over: its accounts as they are right now.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Bundle {
    #[serde(default)]
    settings: HashMap<String, String>,
    accounts: Vec<AccountBundle>,
    /// Also forget accounts the server has that aren't listed.
    #[serde(default)]
    replace: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Imported {
    added: usize,
    updated: usize,
    removed: usize,
}

fn forget_account(state: &AppState, id: &str) -> anyhow::Result<()> {
    state.engine.stop(id);
    for kind in SECRETS {
        let _ = state.secrets.delete(&format!("{kind}:{id}"));
    }
    state.mail.delete_account(id)
}

/// `DELETE /v1/accounts/{id}`: the account was removed in the app; its mail and login go here too.
async fn delete_account(State(state): State<Arc<AppState>>, _me: App, Path(id): Path<String>) -> ApiResult<StatusCode> {
    blocking(move || {
        if state.mail.account(&id).is_err() {
            return Err(ApiError::new(StatusCode::NOT_FOUND, "no such account"));
        }
        forget_account(&state, &id)?;
        Ok(StatusCode::NO_CONTENT)
    })
    .await
}

/// `PUT /v1/accounts`: the app's accounts and their logins. The server syncs them from then on,
/// on its own: it shares the logins with the app, not the mail.
async fn put_accounts(State(state): State<Arc<AppState>>, _me: App, Json(bundle): Json<Bundle>) -> ApiResult<Json<Imported>> {
    blocking(move || {
        for a in &bundle.accounts {
            let id = &a.account.id;
            if id.is_empty() || id.len() > 64 || !id.chars().all(|c| c.is_ascii_alphanumeric()) {
                return Err(ApiError::new(StatusCode::BAD_REQUEST, format!("\"{id}\" is not an account id")));
            }
            if !a.account.email.contains('@') {
                return Err(ApiError::new(StatusCode::BAD_REQUEST, format!("\"{}\" is not an email address", a.account.email)));
            }
            if a.secrets.keys().any(|k| !SECRETS.contains(&k.as_str())) || a.secrets.is_empty() {
                return Err(ApiError::new(StatusCode::BAD_REQUEST, format!("{} needs its login: \"oauth\" or \"password\"", a.account.email)));
            }
        }
        for (key, value) in &bundle.settings {
            if SETTINGS.contains(&key.as_str()) {
                state.mail.set_setting(key, value)?;
            }
        }
        let mut out = Imported { added: 0, updated: 0, removed: 0 };
        for a in &bundle.accounts {
            let id = &a.account.id;
            // A changed login or server means starting over with a clean connection.
            state.engine.stop(id);
            let config = if a.config.is_null() { serde_json::json!({}) } else { a.config.clone() };
            if state.mail.upsert_account(&a.account, &config)? {
                out.added += 1;
            } else {
                out.updated += 1;
                // The old sign-in may have expired; this one is fresh.
                state.mail.set_sync_result(id, None)?;
            }
            for kind in SECRETS {
                let key = format!("{kind}:{id}");
                match a.secrets.get(*kind) {
                    Some(value) => state.secrets.set(&key, value)?,
                    None => state.secrets.delete(&key)?,
                }
            }
            state.engine.start(id);
        }
        if bundle.replace {
            for known in state.mail.accounts()? {
                if !bundle.accounts.iter().any(|a| a.account.id == known.id) {
                    forget_account(&state, &known.id)?;
                    out.removed += 1;
                }
            }
        }
        Ok(Json(out))
    })
    .await
}
