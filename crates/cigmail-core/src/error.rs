/// Raised by providers when the stored credentials are no longer valid and the
/// user has to sign in again.
#[derive(Debug, thiserror::Error)]
#[error("authentication expired, please reconnect this account")]
pub struct AuthExpired;
