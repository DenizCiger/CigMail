//! Whoever runs the engine: the app, with a window to update and a system to notify, or a server
//! with neither.

use serde_json::Value;

pub trait Host: Send + Sync + 'static {
    /// Something changed that a UI would want to redraw for. Events: `mail:changed`,
    /// `sync:status`, `ops:failed`, `outbox:sent`, `outbox:failed`.
    fn emit(&self, event: &str, payload: Value);

    /// A system notification: new mail, a snoozed conversation coming back.
    fn notify(&self, title: &str, body: &str);
}

/// Nobody is watching.
pub struct NoHost;

impl Host for NoHost {
    fn emit(&self, _event: &str, _payload: Value) {}
    fn notify(&self, _title: &str, _body: &str) {}
}
