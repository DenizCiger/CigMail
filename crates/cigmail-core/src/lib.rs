//! CigMail without a window: the local mail cache, the providers (Gmail, Microsoft Graph,
//! IMAP/SMTP) and the engine that keeps the cache in step with them.
//!
//! The desktop and Android app (`src-tauri`) and the server for AI assistants
//! (`crates/cigmail-server`) both run this; what differs is who listens, see [`Host`].

pub mod config;
pub mod db;
pub mod error;
pub mod host;
pub mod mime;
pub mod models;
pub mod oauth;
pub mod providers;
pub mod query;
pub mod secrets;
pub mod sync;

pub use host::{Host, NoHost};

/// Every HTTP client starts here. reqwest checks certificates with the
/// platform verifier, which on Android needs a JNI handshake first and panics
/// without it; use the bundled Mozilla roots there instead (as IMAP/SMTP do).
pub fn http_client() -> reqwest::ClientBuilder {
    let builder = reqwest::Client::builder().user_agent(concat!("CigMail/", env!("CARGO_PKG_VERSION")));
    #[cfg(target_os = "android")]
    let builder = {
        let roots = rustls::RootCertStore { roots: webpki_roots::TLS_SERVER_ROOTS.to_vec() };
        let mut tls = rustls::ClientConfig::builder().with_root_certificates(roots).with_no_client_auth();
        // reqwest is built without HTTP/2 here, so don't let servers pick it.
        tls.alpn_protocols = vec![b"http/1.1".to_vec()];
        builder.tls_backend_preconfigured(tls)
    };
    builder
}
