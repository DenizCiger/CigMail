//! `cigmail-server` runs the server; `cigmail-server set-password` sets the password that
//! protects it. Settings come from `CIGMAIL_*` environment variables, see `Config`.

use std::io::{BufRead, IsTerminal, Write};

use cigmail_server::{Config, auth};

const HELP: &str = "cigmail-server              run the server
cigmail-server set-password set the password for connecting the app and assistants
                            (asks for it, or reads it from standard input)

Environment:
  CIGMAIL_BIND         where to listen (default 127.0.0.1:8788); put a TLS proxy in front
  CIGMAIL_DATA_DIR     where the mail cache and the logins are kept (default ./cigmail-data)
  CIGMAIL_PUBLIC_URL   the address clients use, e.g. https://mail.example.org
                       (default: https://<Host header>)
  CIGMAIL_TRUST_PROXY  1 to take client addresses from X-Forwarded-For
  CIGMAIL_TIMEZONE     the IANA time zone assistants work in, e.g. Europe/Vienna
  CIGMAIL_PASSWORD     sets the password at start, instead of set-password";

fn set_password(config: &Config) -> anyhow::Result<()> {
    let stdin = std::io::stdin();
    if stdin.is_terminal() {
        eprint!("New password (at least {} characters; it is shown as you type): ", auth::MIN_PASSWORD_LEN);
        std::io::stderr().flush()?;
    }
    let mut password = String::new();
    stdin.lock().read_line(&mut password)?;
    let password = password.trim_end_matches(['\r', '\n']);
    std::fs::create_dir_all(&config.data_dir)?;
    let db = auth::open(&config.data_dir.join("server.db"))?;
    auth::set_password(&db, password)?;
    eprintln!("cigmail-server: password set. Apps and assistants that were connected stay connected.");
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let config = Config::from_env()?;
    match std::env::args().nth(1).as_deref() {
        None => {}
        Some("set-password") => return set_password(&config),
        Some("-h" | "--help" | "help") => {
            println!("{HELP}");
            return Ok(());
        }
        Some(other) => anyhow::bail!("unknown argument {other}; see --help"),
    }
    let filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "cigmail_core=info,cigmail_server=info,warn,html5ever=error".into());
    tracing_subscriber::fmt().with_env_filter(filter).init();
    // Several dependencies pull in rustls; pick one crypto provider for all of them.
    let _ = rustls::crypto::ring::default_provider().install_default();

    tokio::runtime::Builder::new_multi_thread().enable_all().build()?.block_on(async {
        let data_dir = config.data_dir.clone();
        let server = cigmail_server::start(config).await?;
        eprintln!("cigmail-server: listening on {} with data in {}", server.url(), data_dir.display());
        if !server.state.has_password() {
            eprintln!("cigmail-server: no password is set yet, so nothing can connect. Run `cigmail-server set-password`.");
        }
        tokio::signal::ctrl_c().await?;
        eprintln!("cigmail-server: shutting down");
        server.stop().await;
        Ok(())
    })
}
