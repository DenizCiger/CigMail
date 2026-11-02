use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AccountKind {
    Gmail,
    Outlook,
    Imap,
}

impl AccountKind {
    pub fn as_str(self) -> &'static str {
        match self {
            AccountKind::Gmail => "gmail",
            AccountKind::Outlook => "outlook",
            AccountKind::Imap => "imap",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "gmail" => AccountKind::Gmail,
            "outlook" => AccountKind::Outlook,
            _ => AccountKind::Imap,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: String,
    pub kind: AccountKind,
    pub email: String,
    /// Display name used in the From header.
    pub name: String,
    /// Short user-facing label, e.g. "School" or "Personal".
    pub label: Option<String>,
    pub color: String,
    pub created_at: i64,
    pub last_sync: Option<i64>,
    pub last_error: Option<String>,
    pub signature: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Security {
    #[default]
    Tls,
    StartTls,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum ImapAuth {
    /// Plain LOGIN / SMTP AUTH with a password or app password.
    #[default]
    Password,
    /// XOAUTH2 with a Microsoft token (fallback for M365 tenants that block Graph).
    MicrosoftOAuth,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImapConfig {
    pub imap_host: String,
    pub imap_port: u16,
    #[serde(default)]
    pub imap_security: Security,
    pub smtp_host: String,
    pub smtp_port: u16,
    #[serde(default)]
    pub smtp_security: Security,
    pub username: String,
    #[serde(default)]
    pub auth: ImapAuth,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct Address {
    pub name: Option<String>,
    pub email: String,
}

impl Address {
    pub fn display(&self) -> String {
        match &self.name {
            Some(n) if !n.is_empty() => format!("{n} <{}>", self.email),
            _ => self.email.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentMeta {
    /// Position in `mail_parser`'s attachment list, used to re-extract it.
    pub index: usize,
    pub filename: String,
    pub mime: String,
    pub size: usize,
    pub inline: bool,
    pub content_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadSummary {
    pub id: String,
    pub account_id: String,
    pub subject: String,
    pub snippet: String,
    pub last_date: i64,
    pub participants: Vec<Address>,
    pub message_count: i64,
    pub unread: bool,
    pub starred: bool,
    pub in_inbox: bool,
    pub has_attachments: bool,
    pub category: String,
    pub snoozed_until: Option<i64>,
    pub ai_summary: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: String,
    pub account_id: String,
    pub thread_id: String,
    pub message_id: Option<String>,
    pub references: Vec<String>,
    pub subject: String,
    pub from: Address,
    pub to: Vec<Address>,
    pub cc: Vec<Address>,
    pub bcc: Vec<Address>,
    pub reply_to: Vec<Address>,
    pub date: i64,
    pub snippet: String,
    pub body_html: Option<String>,
    pub body_text: Option<String>,
    pub is_read: bool,
    pub is_starred: bool,
    pub is_draft: bool,
    pub in_sent: bool,
    pub attachments: Vec<AttachmentMeta>,
    pub list_unsubscribe: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadDetail {
    pub thread: ThreadSummary,
    pub messages: Vec<Message>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutgoingAttachment {
    pub path: String,
}

/// A message being composed; also the payload stored in the outbox while the
/// undo-send window is open.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Draft {
    #[serde(default)]
    pub id: Option<String>,
    pub account_id: String,
    #[serde(default)]
    pub to: Vec<Address>,
    #[serde(default)]
    pub cc: Vec<Address>,
    #[serde(default)]
    pub bcc: Vec<Address>,
    #[serde(default)]
    pub subject: String,
    #[serde(default)]
    pub html: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub in_reply_to: Option<String>,
    #[serde(default)]
    pub references: Vec<String>,
    #[serde(default)]
    pub thread_id: Option<String>,
    #[serde(default)]
    pub attachments: Vec<OutgoingAttachment>,
    /// "new" | "reply" | "replyAll" | "forward"
    #[serde(default)]
    pub mode: Option<String>,
    /// Quoted original shown below the editor (already HTML).
    #[serde(default)]
    pub quoted_html: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Split {
    pub id: String,
    pub name: String,
    pub query: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Contact {
    pub name: Option<String>,
    pub email: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncStatus {
    pub account_id: String,
    /// "idle" | "syncing" | "error"
    pub state: String,
    pub message: Option<String>,
    pub progress: Option<(u32, u32)>,
}
