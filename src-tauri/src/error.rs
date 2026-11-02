use serde::{Serialize, Serializer};

/// Error type returned to the frontend from Tauri commands. Internally we use
/// `anyhow` everywhere and only convert at the command boundary.
#[derive(Debug)]
pub struct CmdError(pub String);

impl<E: Into<anyhow::Error>> From<E> for CmdError {
    fn from(e: E) -> Self {
        let e: anyhow::Error = e.into();
        // `{:#}` includes the context chain ("syncing gmail: network error: ...").
        CmdError(format!("{e:#}"))
    }
}

impl Serialize for CmdError {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0)
    }
}

pub type CmdResult<T> = std::result::Result<T, CmdError>;

/// Raised by providers when the stored credentials are no longer valid and the
/// user has to sign in again.
#[derive(Debug, thiserror::Error)]
#[error("authentication expired, please reconnect this account")]
pub struct AuthExpired;
