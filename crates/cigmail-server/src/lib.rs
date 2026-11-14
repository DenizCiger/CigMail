//! CigMail's server for AI assistants.
//!
//! It runs the same engine as the app (`cigmail-core`) with nobody watching: it keeps a mail
//! cache in step with the user's accounts and offers it as tools on `/mcp` (`mcp`), after an
//! assistant was signed in (`oauth`). The accounts and their logins come from the app, which
//! pairs with the server and sends them over (`api`).
//!
//! One server serves one person. A single password (`auth`) protects pairing and sign-in.
//! An assistant can read mail, tidy up and save drafts; there is no tool that sends.
//!
//! Plain HTTP: run it behind a reverse proxy that terminates TLS.

pub mod api;
pub mod auth;
pub mod mcp;
pub mod oauth;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use cigmail_core::db::Db;
use cigmail_core::secrets::Secrets;
use cigmail_core::sync::Engine;
use parking_lot::Mutex;
use rusqlite::Connection;

#[derive(Debug, Clone)]
pub struct Config {
    /// Where to listen. Keep it on localhost and put a TLS-terminating reverse proxy in front.
    pub bind: SocketAddr,
    /// Holds `cigmail.db` (the mail cache, the accounts and their logins) and `server.db` (the
    /// password, paired apps, connected assistants and what they did).
    pub data_dir: PathBuf,
    /// The address clients reach the server at, e.g. `https://mail.example.org`, for the links
    /// handed to assistants. Unset, it's taken from each request: `https://<Host>`.
    pub public_url: Option<String>,
    /// Take the client address from `X-Forwarded-For` (only behind a proxy that sets it).
    pub trust_proxy: bool,
    /// The IANA time zone assistants work in when they don't name one. Unset: the server's.
    pub timezone: Option<String>,
    /// Sets the password at start. Otherwise `cigmail-server set-password` does.
    pub password: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            bind: "127.0.0.1:8788".parse().unwrap(),
            data_dir: PathBuf::from("cigmail-data"),
            public_url: None,
            trust_proxy: false,
            timezone: None,
            password: None,
        }
    }
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        Self::from_vars(|k| std::env::var(k).ok())
    }

    fn from_vars(var: impl Fn(&str) -> Option<String>) -> anyhow::Result<Self> {
        let mut c = Self::default();
        let flag = |v: String| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on");
        let set = |k: &str| var(k).filter(|v| !v.trim().is_empty());
        if let Some(v) = set("CIGMAIL_BIND") {
            c.bind = v.parse().with_context(|| format!("CIGMAIL_BIND={v}"))?;
        }
        if let Some(v) = set("CIGMAIL_DATA_DIR") {
            c.data_dir = v.into();
        }
        c.public_url = set("CIGMAIL_PUBLIC_URL");
        c.trust_proxy = set("CIGMAIL_TRUST_PROXY").is_some_and(flag);
        if let Some(tz) = set("CIGMAIL_TIMEZONE") {
            tz.trim().parse::<chrono_tz::Tz>().map_err(|_| anyhow::anyhow!("CIGMAIL_TIMEZONE={tz}: not an IANA time zone like Europe/Vienna"))?;
            c.timezone = Some(tz.trim().to_string());
        }
        c.password = set("CIGMAIL_PASSWORD");
        Ok(c)
    }
}

pub struct AppState {
    pub config: Config,
    /// `server.db`.
    pub db: Mutex<Connection>,
    /// The mail cache, as in the app.
    pub mail: Db,
    pub secrets: Secrets,
    pub http: reqwest::Client,
    pub engine: Arc<Engine>,
    pub limiter: Mutex<auth::Limiter>,
}

impl AppState {
    pub fn has_password(&self) -> bool {
        auth::password_hash(&self.db.lock()).ok().flatten().is_some()
    }
}

/// A server listening in the background.
pub struct Running {
    pub addr: SocketAddr,
    pub state: Arc<AppState>,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<()>,
}

impl Running {
    pub fn url(&self) -> String {
        format!("http://{}", self.addr)
    }

    pub async fn stop(mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        if tokio::time::timeout(Duration::from_secs(2), &mut self.task).await.is_err() {
            self.task.abort();
        }
        for a in self.state.mail.accounts().unwrap_or_default() {
            self.state.engine.stop(&a.id);
        }
    }
}

/// Binds, starts syncing every account and serves until `stop`. Call from within a runtime.
pub async fn start(config: Config) -> anyhow::Result<Running> {
    std::fs::create_dir_all(&config.data_dir).with_context(|| format!("creating {}", config.data_dir.display()))?;
    // Mail and logins are nobody else's business on a shared machine.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&config.data_dir, std::fs::Permissions::from_mode(0o700))
            .with_context(|| format!("restricting {}", config.data_dir.display()))?;
    }
    let db = auth::open(&config.data_dir.join("server.db")).context("opening server.db")?;
    oauth::migrate(&db).context("preparing server.db for assistants")?;
    if let Some(password) = &config.password {
        auth::set_password(&db, password).context("CIGMAIL_PASSWORD")?;
    }
    let mail = Db::open(&config.data_dir.join("cigmail.db"))?;
    let secrets = Secrets::new(mail.clone());
    let http = cigmail_core::http_client().connect_timeout(Duration::from_secs(15)).timeout(Duration::from_secs(120)).build()?;
    let engine = Engine::new(Arc::new(cigmail_core::NoHost), tokio::runtime::Handle::current(), mail.clone(), secrets.clone(), http.clone());
    engine.start_all()?;

    let listener = tokio::net::TcpListener::bind(config.bind).await.with_context(|| format!("binding {}", config.bind))?;
    let addr = listener.local_addr()?;
    let state = Arc::new(AppState { config, db: Mutex::new(db), mail, secrets, http, engine, limiter: Default::default() });
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    let app = api::router(state.clone());
    let task = tokio::spawn(async move {
        let serve = axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).with_graceful_shutdown(async {
            let _ = rx.await;
        });
        if let Err(e) = serve.await {
            eprintln!("cigmail-server: {e}");
        }
    });
    Ok(Running { addr, state, shutdown: Some(tx), task })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_from_the_environment() {
        let c = Config::from_vars(|k| match k {
            "CIGMAIL_BIND" => Some("0.0.0.0:9000".into()),
            "CIGMAIL_TRUST_PROXY" => Some("yes".into()),
            "CIGMAIL_TIMEZONE" => Some(" Europe/Vienna ".into()),
            "CIGMAIL_PUBLIC_URL" => Some("".into()),
            _ => None,
        })
        .unwrap();
        assert_eq!(c.bind.port(), 9000);
        assert!(c.trust_proxy);
        assert_eq!(c.timezone.as_deref(), Some("Europe/Vienna"));
        assert_eq!(c.public_url, None, "empty counts as unset");
        assert!(Config::from_vars(|k| (k == "CIGMAIL_TIMEZONE").then(|| "Mars/Olympus".into())).is_err());
    }
}
