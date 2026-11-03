//! OAuth 2.0 authorization-code flow with PKCE and a loopback redirect, shared
//! by Google, Microsoft and the ChatGPT (Codex) login.

use std::{collections::HashMap, sync::LazyLock, time::Duration};

use anyhow::{Context, Result, anyhow, bail};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::Notify,
};
use url::Url;

/// Signalled to abort any login that's currently waiting for its browser redirect.
static CANCEL: LazyLock<Notify> = LazyLock::new(Notify::new);

pub fn cancel_pending() {
    CANCEL.notify_waiters();
}

#[derive(Debug, Clone)]
pub enum Redirect {
    /// `http://127.0.0.1:<random>/` (Google desktop clients).
    Ip,
    /// `http://localhost:<random>` (Microsoft public clients registered with `http://localhost`).
    Localhost,
    /// A fixed `http://localhost:<port><path>` (Codex expects exactly this).
    Fixed { port: u16, path: &'static str },
}

#[derive(Debug, Clone)]
pub struct OAuthClient {
    pub auth_url: String,
    pub token_url: String,
    pub client_id: String,
    pub client_secret: Option<String>,
    pub scopes: String,
    pub extra_params: Vec<(String, String)>,
    pub redirect: Redirect,
    /// The Codex token endpoint wants refresh requests as JSON instead of a form.
    pub refresh_as_json: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tokens {
    pub access_token: String,
    pub refresh_token: Option<String>,
    /// Unix seconds.
    pub expires_at: i64,
    #[serde(default)]
    pub id_token: Option<String>,
}

impl Tokens {
    pub fn is_fresh(&self) -> bool {
        self.expires_at - 90 > chrono::Utc::now().timestamp()
    }
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: Option<i64>,
    id_token: Option<String>,
}

fn random_b64(n: usize) -> String {
    let mut bytes = Vec::with_capacity(n);
    while bytes.len() < n {
        bytes.extend_from_slice(&rand::random::<[u8; 32]>());
    }
    bytes.truncate(n);
    URL_SAFE_NO_PAD.encode(bytes)
}

pub fn decode_jwt_claims(token: &str) -> Option<serde_json::Value> {
    let payload = token.split('.').nth(1)?;
    let bytes = URL_SAFE_NO_PAD.decode(payload.trim_end_matches('=')).ok()?;
    serde_json::from_slice(&bytes).ok()
}

async fn bind_loopback(redirect: &Redirect) -> Result<(Vec<TcpListener>, String, String)> {
    match redirect {
        Redirect::Ip => {
            let l = TcpListener::bind("127.0.0.1:0").await?;
            let port = l.local_addr()?.port();
            Ok((vec![l], format!("http://127.0.0.1:{port}/"), "/".into()))
        }
        Redirect::Localhost => {
            let l = TcpListener::bind("127.0.0.1:0").await?;
            let port = l.local_addr()?.port();
            let mut ls = vec![l];
            // Browsers may resolve localhost to ::1 first.
            if let Ok(l6) = TcpListener::bind(("::1", port)).await {
                ls.push(l6);
            }
            Ok((ls, format!("http://localhost:{port}"), "/".into()))
        }
        Redirect::Fixed { port, path } => {
            // A previous login attempt may still be holding the port for a moment.
            let mut last_err = None;
            for _ in 0..20 {
                match TcpListener::bind(("127.0.0.1", *port)).await {
                    Ok(l) => {
                        let mut ls = vec![l];
                        if let Ok(l6) = TcpListener::bind(("::1", *port)).await {
                            ls.push(l6);
                        }
                        return Ok((ls, format!("http://localhost:{port}{path}"), path.to_string()));
                    }
                    Err(e) => {
                        last_err = Some(e);
                        tokio::time::sleep(Duration::from_millis(150)).await;
                    }
                }
            }
            Err(anyhow!(
                "port {port} is in use (is the Codex CLI login running?): {}",
                last_err.map(|e| e.to_string()).unwrap_or_default()
            ))
        }
    }
}

const PAGE: &str = r#"<!doctype html><html><head><meta charset="utf-8"><title>CigMail</title>
<style>body{font-family:Inter,system-ui,sans-serif;background:#0d0e10;color:#e6e6e8;display:grid;place-items:center;height:100vh;margin:0}
div{text-align:center;padding:0 24px}h1{font-weight:600;font-size:20px;margin:0 0 8px}p{color:#8a8f98;margin:0}
a{display:inline-block;margin-top:24px;padding:12px 22px;border-radius:12px;background:#5e6ad2;color:#fff;text-decoration:none;font-weight:500}</style></head>
<body><div><h1>{title}</h1><p>{body}</p></div></body></html>"#;

async fn respond(sock: &mut tokio::net::TcpStream, status: &str, title: &str, body: &str) {
    let html = PAGE.replace("{title}", title).replace("{body}", body);
    let resp = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{html}",
        html.len()
    );
    let _ = sock.write_all(resp.as_bytes()).await;
    let _ = sock.shutdown().await;
}

async fn wait_for_code(listeners: &[TcpListener], path: &str, state: &str) -> Result<String> {
    loop {
        let accepts = listeners.iter().map(|l| Box::pin(l.accept()));
        let (res, _, _) = futures::future::select_all(accepts).await;
        let (mut sock, _) = res?;
        let mut buf = vec![0u8; 16 * 1024];
        let n = sock.read(&mut buf).await.unwrap_or(0);
        let req = String::from_utf8_lossy(&buf[..n]);
        let target = req
            .lines()
            .next()
            .and_then(|l| l.split_whitespace().nth(1))
            .unwrap_or("/");
        tracing::info!("sign-in: loopback request for {}", target.split('?').next().unwrap_or(target));
        let Ok(url) = Url::parse(&format!("http://localhost{target}")) else {
            respond(&mut sock, "400 Bad Request", "Bad request", "").await;
            continue;
        };
        if url.path() != path {
            respond(&mut sock, "404 Not Found", "Not found", "").await;
            continue;
        }
        let params: HashMap<String, String> = url.query_pairs().into_owned().collect();
        if let Some(err) = params.get("error") {
            let desc = params.get("error_description").cloned().unwrap_or_default();
            respond(&mut sock, "200 OK", "Sign-in failed", &html_escape(&desc)).await;
            bail!("{err}: {desc}");
        }
        let Some(code) = params.get("code") else {
            // e.g. a browser prefetch; keep waiting for the real redirect
            respond(&mut sock, "400 Bad Request", "Missing code", "").await;
            continue;
        };
        if params.get("state").map(String::as_str) != Some(state) {
            respond(&mut sock, "400 Bad Request", "Sign-in failed", "State mismatch.").await;
            bail!("OAuth state mismatch");
        }
        respond(&mut sock, "200 OK", "You're signed in", SIGNED_IN).await;
        return Ok(code.clone());
    }
}

#[cfg(desktop)]
const SIGNED_IN: &str = "You can close this tab and return to CigMail.";
/// Android pauses the app's network while the browser is in front, so send the
/// user straight back; the launcher intent brings the running app forward.
/// Android pauses the app while the browser is in front, so send the user
/// straight back: the page tries on its own, and the button works if the
/// browser wants a tap first. `singleTask` brings the running app forward.
#[cfg(mobile)]
const SIGNED_IN: &str = "Head back to CigMail to finish.<br>\
<a href=\"intent://open#Intent;scheme=cigmail;package=dev.ciger.cigmail;end\">Open CigMail</a>\
<script>location.href='intent://open#Intent;scheme=cigmail;package=dev.ciger.cigmail;end'</script>";

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// Runs the full browser flow. `open` is called with the authorization URL.
pub async fn authorize(
    client: &OAuthClient,
    open: impl FnOnce(&str) -> Result<()>,
) -> Result<Tokens> {
    cancel_pending();
    let (listeners, redirect_uri, path) = bind_loopback(&client.redirect).await?;

    let verifier = random_b64(48);
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let state = random_b64(24);

    let mut url = Url::parse(&client.auth_url)?;
    {
        let mut q = url.query_pairs_mut();
        q.append_pair("response_type", "code")
            .append_pair("client_id", &client.client_id)
            .append_pair("redirect_uri", &redirect_uri)
            .append_pair("scope", &client.scopes)
            .append_pair("state", &state)
            .append_pair("code_challenge", &challenge)
            .append_pair("code_challenge_method", "S256");
        for (k, v) in &client.extra_params {
            q.append_pair(k, v);
        }
    }
    open(url.as_str())?;
    tracing::info!("sign-in: opened the browser, waiting for the redirect to {redirect_uri}");

    let code = tokio::select! {
        r = tokio::time::timeout(Duration::from_secs(300), wait_for_code(&listeners, &path, &state)) => {
            r.map_err(|_| anyhow!("timed out waiting for the browser sign-in"))??
        }
        _ = CANCEL.notified() => bail!("sign-in cancelled"),
    };
    drop(listeners);
    tracing::info!("sign-in: got the authorization code, exchanging it for tokens");

    let mut form = vec![
        ("grant_type", "authorization_code".to_string()),
        ("code", code),
        ("redirect_uri", redirect_uri),
        ("client_id", client.client_id.clone()),
        ("code_verifier", verifier),
    ];
    if let Some(secret) = &client.client_secret {
        form.push(("client_secret", secret.clone()));
    }
    // On Android the app has no network until the user switches back from the
    // browser, so keep retrying for as long as the code is valid. Each attempt
    // gets a fresh client and a hard time limit: a connection begun while the
    // network was blocked can stall well past the shared client's timeouts.
    let started = tokio::time::Instant::now();
    let deadline = started + Duration::from_secs(300);
    let mut attempt = 0u32;
    let resp = loop {
        attempt += 1;
        let once = crate::http_client()
            .connect_timeout(Duration::from_secs(8))
            .timeout(Duration::from_secs(20))
            .pool_max_idle_per_host(0)
            .build()?;
        let err = match tokio::time::timeout(Duration::from_secs(25), once.post(&client.token_url).form(&form).send()).await {
            Ok(Ok(r)) => break r,
            Ok(Err(e)) if e.status().is_none() => anyhow::Error::new(e),
            Ok(Err(e)) => return Err(e.into()),
            Err(_) => anyhow!("no answer within 25s"),
        };
        if tokio::time::Instant::now() >= deadline {
            return Err(err.context("could not reach the sign-in server"));
        }
        tracing::warn!("sign-in: token request attempt {attempt} failed after {:?}: {err:#}", started.elapsed());
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(1)) => {}
            _ = CANCEL.notified() => bail!("sign-in cancelled"),
        }
    };
    tracing::info!("sign-in: token endpoint answered {} after {attempt} attempt(s)", resp.status());
    parse_token_response(resp, None).await
}

pub async fn refresh(http: &reqwest::Client, client: &OAuthClient, refresh_token: &str) -> Result<Tokens> {
    let req = http.post(&client.token_url);
    let req = if client.refresh_as_json {
        req.json(&serde_json::json!({
            "grant_type": "refresh_token",
            "refresh_token": refresh_token,
            "client_id": client.client_id,
        }))
    } else {
        let mut form = vec![
            ("grant_type", "refresh_token".to_string()),
            ("refresh_token", refresh_token.to_string()),
            ("client_id", client.client_id.clone()),
        ];
        if let Some(secret) = &client.client_secret {
            form.push(("client_secret", secret.clone()));
        }
        // Microsoft wants the scopes again on refresh.
        if !client.scopes.is_empty() {
            form.push(("scope", client.scopes.clone()));
        }
        req.form(&form)
    };
    let resp = req.send().await?;
    parse_token_response(resp, Some(refresh_token)).await
}

async fn parse_token_response(resp: reqwest::Response, old_refresh: Option<&str>) -> Result<Tokens> {
    let status = resp.status();
    let body = resp.text().await?;
    if !status.is_success() {
        let msg = serde_json::from_str::<serde_json::Value>(&body)
            .ok()
            .and_then(|v| {
                ["error_description", "message", "detail", "error"]
                    .iter()
                    .find_map(|k| v.get(*k).and_then(|x| x.as_str()).map(String::from))
            })
            .unwrap_or(body);
        if status.as_u16() == 400 || status.as_u16() == 401 {
            return Err(anyhow::Error::new(crate::error::AuthExpired).context(msg));
        }
        bail!("token endpoint returned {status}: {msg}");
    }
    let t: TokenResponse = serde_json::from_str(&body).context("parsing token response")?;
    Ok(Tokens {
        access_token: t.access_token,
        refresh_token: t.refresh_token.or_else(|| old_refresh.map(String::from)),
        expires_at: chrono::Utc::now().timestamp() + t.expires_in.unwrap_or(3600),
        id_token: t.id_token,
    })
}
