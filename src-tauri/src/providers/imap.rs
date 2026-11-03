//! Generic IMAP + SMTP. Also used with XOAUTH2 as a fallback for Microsoft 365
//! tenants that block Graph.

use std::{collections::HashMap, fmt::Debug, sync::Arc, time::Duration};

use anyhow::{Context, Result, anyhow, bail};
use async_imap::{
    Client, Session,
    imap_proto::NameAttribute,
    types::{Fetch, Flag},
};
use futures::TryStreamExt;
use lettre::{
    AsyncSmtpTransport, AsyncTransport, Tokio1Executor,
    transport::smtp::authentication::{Credentials, Mechanism},
};
use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader},
    net::TcpStream,
};

use super::{Ctx, oauth_token};
use crate::{
    config,
    db::{Flags, MsgRef, ThreadAction},
    error::AuthExpired,
    mime,
    models::{Draft, ImapAuth, ImapConfig, Security},
};

pub trait ImapStream: AsyncRead + AsyncWrite + Unpin + Send + Sync + Debug {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send + Sync + Debug> ImapStream for T {}

type Sess = Session<Box<dyn ImapStream>>;

const FETCH_CHUNK: usize = 25;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_INITIAL_PER_FOLDER: usize = 3000;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Folders {
    inbox: String,
    sent: Option<String>,
    archive: Option<String>,
    trash: Option<String>,
    spam: Option<String>,
    drafts: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct BoxState {
    uid_validity: u32,
    last_uid: u32,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct State {
    folders: Option<Folders>,
    boxes: HashMap<String, BoxState>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    Inbox,
    Sent,
    Archive,
}

pub struct Imap {
    cfg: ImapConfig,
    session: Option<Sess>,
    has_move: bool,
}

struct XOAuth2 {
    user: String,
    token: String,
}

impl async_imap::Authenticator for &XOAuth2 {
    type Response = String;
    fn process(&mut self, _challenge: &[u8]) -> Self::Response {
        format!("user={}\x01auth=Bearer {}\x01\x01", self.user, self.token)
    }
}

fn tls_connector() -> tokio_rustls::TlsConnector {
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let cfg = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    tokio_rustls::TlsConnector::from(Arc::new(cfg))
}

async fn tls_wrap(host: &str, tcp: TcpStream) -> Result<Box<dyn ImapStream>> {
    let name = rustls::pki_types::ServerName::try_from(host.to_string())
        .map_err(|_| anyhow!("invalid host name {host}"))?;
    Ok(Box::new(tls_connector().connect(name, tcp).await.context("TLS handshake")?))
}

/// Reads the greeting, issues STARTTLS on the plain socket and returns it for wrapping.
async fn starttls(tcp: TcpStream) -> Result<TcpStream> {
    let mut reader = BufReader::new(tcp);
    let mut line = String::new();
    reader.read_line(&mut line).await?;
    reader.get_mut().write_all(b"a0 STARTTLS\r\n").await?;
    loop {
        line.clear();
        if reader.read_line(&mut line).await? == 0 {
            bail!("connection closed during STARTTLS");
        }
        if line.starts_with("a0 ") {
            if !line[3..].trim_start().to_ascii_uppercase().starts_with("OK") {
                bail!("server refused STARTTLS: {}", line.trim());
            }
            break;
        }
    }
    Ok(reader.into_inner())
}

fn imap_date(ts: i64) -> String {
    chrono::DateTime::from_timestamp(ts, 0)
        .unwrap_or_default()
        .format("%d-%b-%Y")
        .to_string()
}

fn remote_id(folder: &str, validity: u32, uid: u32) -> String {
    format!("{folder}/{validity}/{uid}")
}

/// Compacts sorted UIDs into an IMAP sequence set like `1:5,8,10:12`.
fn uid_set(uids: &[u32]) -> String {
    let mut sorted = uids.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    let mut parts = Vec::new();
    let mut i = 0;
    while i < sorted.len() {
        let start = sorted[i];
        let mut end = start;
        while i + 1 < sorted.len() && sorted[i + 1] == end + 1 {
            i += 1;
            end = sorted[i];
        }
        parts.push(if start == end { start.to_string() } else { format!("{start}:{end}") });
        i += 1;
    }
    parts.join(",")
}

fn fetch_flags(f: &Fetch) -> (bool, bool, bool) {
    let mut seen = false;
    let mut flagged = false;
    let mut draft = false;
    for fl in f.flags() {
        match fl {
            Flag::Seen => seen = true,
            Flag::Flagged => flagged = true,
            Flag::Draft => draft = true,
            _ => {}
        }
    }
    (seen, flagged, draft)
}

fn role_flags(role: Role, seen: bool, flagged: bool, draft: bool) -> Flags {
    Flags {
        read: seen,
        starred: flagged,
        inbox: role == Role::Inbox,
        sent: role == Role::Sent,
        drafts: draft,
        trash: false,
        spam: false,
    }
}

fn find_folder(names: &[(String, Vec<String>)], attr: &str, fallbacks: &[&str]) -> Option<String> {
    if let Some((n, _)) = names.iter().find(|(_, a)| a.iter().any(|x| x == attr)) {
        return Some(n.clone());
    }
    for fb in fallbacks {
        for (n, _) in names {
            let leaf = n.rsplit(['/', '.']).next().unwrap_or(n);
            if leaf.eq_ignore_ascii_case(fb) || n.eq_ignore_ascii_case(fb) {
                return Some(n.clone());
            }
        }
    }
    None
}

impl Imap {
    pub fn new(ctx: &Ctx) -> Result<Self> {
        let cfg: ImapConfig = ctx.db.account_config(&ctx.account.id)?;
        Ok(Imap { cfg, session: None, has_move: false })
    }

    async fn credential(&self, ctx: &Ctx) -> Result<String> {
        match self.cfg.auth {
            ImapAuth::Password => ctx
                .secrets
                .get(&format!("password:{}", ctx.account.id))?
                .ok_or_else(|| AuthExpired.into()),
            ImapAuth::MicrosoftOAuth => oauth_token(ctx, &config::microsoft_imap(&ctx.db)?, false).await,
        }
    }

    pub async fn connect_with(cfg: &ImapConfig, secret: &str) -> Result<(Sess, bool)> {
        let connect = async {
            let tcp = TcpStream::connect((cfg.imap_host.as_str(), cfg.imap_port))
                .await
                .with_context(|| format!("connecting to {}:{}", cfg.imap_host, cfg.imap_port))?;
            let stream: Box<dyn ImapStream> = match cfg.imap_security {
                Security::Tls => tls_wrap(&cfg.imap_host, tcp).await?,
                Security::StartTls => tls_wrap(&cfg.imap_host, starttls(tcp).await?).await?,
                Security::None => Box::new(tcp),
            };
            let mut client = Client::new(stream);
            if cfg.imap_security != Security::StartTls {
                client.read_response().await?.context("server closed the connection")?;
            }
            anyhow::Ok(client)
        };
        let client = tokio::time::timeout(CONNECT_TIMEOUT, connect).await.map_err(|_| {
            anyhow!(
                "IMAP connection to {}:{} timed out. Port 993 usually needs SSL/TLS, 143 needs STARTTLS",
                cfg.imap_host,
                cfg.imap_port
            )
        })??;
        let mut session = match cfg.auth {
            ImapAuth::Password => client
                .login(&cfg.username, secret)
                .await
                .map_err(|(e, _)| anyhow::Error::new(AuthExpired).context(e.to_string()))?,
            ImapAuth::MicrosoftOAuth => {
                let auth = XOAuth2 { user: cfg.username.clone(), token: secret.to_string() };
                client
                    .authenticate("XOAUTH2", &auth)
                    .await
                    .map_err(|(e, _)| anyhow::Error::new(AuthExpired).context(e.to_string()))?
            }
        };
        let caps = session.capabilities().await?;
        let has_move = caps.has_str("MOVE");
        Ok((session, has_move))
    }

    async fn session(&mut self, ctx: &Ctx) -> Result<&mut Sess> {
        if let Some(s) = self.session.as_mut() {
            if s.noop().await.is_ok() {
                return Ok(self.session.as_mut().unwrap());
            }
            self.session = None;
        }
        let secret = self.credential(ctx).await?;
        let (s, has_move) = Self::connect_with(&self.cfg, &secret).await?;
        self.has_move = has_move;
        self.session = Some(s);
        Ok(self.session.as_mut().unwrap())
    }

    async fn discover_folders(&mut self, ctx: &Ctx) -> Result<Folders> {
        let s = self.session(ctx).await?;
        let names: Vec<(String, Vec<String>)> = {
            let list = s.list(Some(""), Some("*")).await?;
            let names: Vec<_> = list.try_collect().await?;
            names
                .iter()
                .filter(|n| !n.attributes().contains(&NameAttribute::NoSelect))
                .map(|n| {
                    let attrs = n
                        .attributes()
                        .iter()
                        .map(|a| match a {
                            NameAttribute::Sent => "sent".to_string(),
                            NameAttribute::Archive => "archive".to_string(),
                            NameAttribute::Trash => "trash".to_string(),
                            NameAttribute::Junk => "junk".to_string(),
                            NameAttribute::Drafts => "drafts".to_string(),
                            _ => String::new(),
                        })
                        .collect();
                    (n.name().to_string(), attrs)
                })
                .collect()
        };
        if !names.iter().any(|(n, _)| n.eq_ignore_ascii_case("INBOX")) {
            bail!("server's folder list has no INBOX");
        }
        Ok(Folders {
            inbox: "INBOX".into(),
            sent: find_folder(
                &names,
                "sent",
                &["Sent", "Sent Items", "Sent Messages", "Sent Mail", "Gesendet", "Gesendete Elemente", "Gesendete Objekte"],
            ),
            archive: find_folder(&names, "archive", &["Archive", "Archives", "Archiv"]),
            trash: find_folder(
                &names,
                "trash",
                &["Trash", "Deleted Items", "Deleted Messages", "Bin", "Papierkorb", "Gel&APY-schte Elemente", "Gelöschte Elemente"],
            ),
            spam: find_folder(&names, "junk", &["Junk", "Spam", "Junk E-Mail", "Junk Email", "Junk-E-Mail"]),
            drafts: find_folder(&names, "drafts", &["Drafts", "Entw&APw-rfe", "Entwürfe"]),
        })
    }

    pub async fn sync(&mut self, ctx: &Ctx) -> Result<()> {
        let mut st: State = ctx.db.sync_state(&ctx.account.id)?;
        let folders = match st.folders.clone() {
            Some(f) if f.sent.is_some() => f,
            // Keep looking for a Sent folder until there is one: it may not have
            // existed yet when the account was added.
            cached => {
                let mut f = self.discover_folders(ctx).await?;
                if let Some(c) = cached {
                    f.archive = c.archive.or(f.archive);
                    f.trash = c.trash.or(f.trash);
                    f.spam = c.spam.or(f.spam);
                    f.drafts = c.drafts.or(f.drafts);
                }
                st.folders = Some(f.clone());
                f
            }
        };
        let mut targets = vec![(folders.inbox.clone(), Role::Inbox)];
        if let Some(s) = &folders.sent {
            targets.push((s.clone(), Role::Sent));
        }
        if let Some(a) = &folders.archive {
            targets.push((a.clone(), Role::Archive));
        }
        for (folder, role) in targets {
            let mut bs = st.boxes.get(&folder).cloned().unwrap_or_default();
            self.sync_folder(ctx, &folder, role, &mut bs).await?;
            st.boxes.insert(folder, bs);
            ctx.db.set_sync_state(&ctx.account.id, &st)?;
        }
        Ok(())
    }

    async fn sync_folder(&mut self, ctx: &Ctx, folder: &str, role: Role, bs: &mut BoxState) -> Result<()> {
        let acc = ctx.account.id.clone();
        let days = ctx.sync_days();
        let s = self.session(ctx).await?;
        let mbox = s.select(folder).await.with_context(|| format!("selecting {folder}"))?;
        // A healthy SELECT always reports UIDVALIDITY. Without it the connection
        // is out of step, and treating it as a reset would wipe the local cache.
        let validity = mbox
            .uid_validity
            .with_context(|| format!("server sent no UIDVALIDITY for {folder}"))?;
        if validity != bs.uid_validity {
            // UIDs were reset by the server; everything we have for this folder is stale.
            for (_, r) in ctx.db.folder_uids(&acc, folder)? {
                ctx.db.delete_message(&acc, &r.remote_id)?;
            }
            *bs = BoxState { uid_validity: validity, last_uid: 0 };
        }

        // 1. New messages.
        let mut new_uids: Vec<u32> = if bs.last_uid == 0 {
            let since = imap_date(chrono::Utc::now().timestamp() - days * 86400);
            s.uid_search(format!("SINCE {since}")).await?.into_iter().collect()
        } else {
            s.uid_search(format!("UID {}:*", bs.last_uid + 1))
                .await?
                .into_iter()
                .filter(|u| *u > bs.last_uid)
                .collect()
        };
        new_uids.sort_unstable();
        if new_uids.len() > MAX_INITIAL_PER_FOLDER {
            new_uids.drain(..new_uids.len() - MAX_INITIAL_PER_FOLDER);
        }
        let total = new_uids.len() as u32;
        let mut done = 0u32;
        for chunk in new_uids.chunks(FETCH_CHUNK) {
            let fetches: Vec<Fetch> = {
                let stream = s
                    .uid_fetch(uid_set(chunk), "(UID FLAGS INTERNALDATE BODY.PEEK[])")
                    .await?;
                stream.try_collect().await?
            };
            for f in &fetches {
                let (Some(uid), Some(body)) = (f.uid, f.body()) else { continue };
                let (seen, flagged, draft) = fetch_flags(f);
                let rid = remote_id(folder, validity, uid);
                if let Err(e) = ctx.ingest(
                    &rid,
                    None,
                    body,
                    role_flags(role, seen, flagged, draft),
                    Vec::new(),
                    Some(folder.to_string()),
                    Some(uid),
                    f.internal_date().map(|d| d.timestamp()),
                    None,
                ) {
                    tracing::warn!("imap: could not store {rid}: {e:#}");
                }
                bs.last_uid = bs.last_uid.max(uid);
            }
            done += chunk.len() as u32;
            ctx.emit_progress(done, total);
            ctx.emit_changed();
        }
        if let Some(max) = new_uids.last() {
            bs.last_uid = bs.last_uid.max(*max);
        }

        // 2. Flag changes and deletions for what we already have.
        let local = ctx.db.folder_uids(&acc, folder)?;
        if let Some(min) = local.keys().min() {
            let fetches: Vec<Fetch> = {
                let stream = s.uid_fetch(format!("{min}:*"), "(UID FLAGS)").await?;
                stream.try_collect().await?
            };
            let mut remote: HashMap<u32, (bool, bool, bool)> = HashMap::new();
            for f in &fetches {
                if let Some(uid) = f.uid {
                    remote.insert(uid, fetch_flags(f));
                }
            }
            let mut changed = false;
            for (uid, r) in &local {
                match remote.get(uid) {
                    None => {
                        ctx.db.delete_message(&acc, &r.remote_id)?;
                        changed = true;
                    }
                    Some((seen, flagged, draft)) => {
                        let mut f = role_flags(role, *seen, *flagged, *draft);
                        // Keep the "sent by me" and spam markers set at ingest time.
                        f.sent |= r.flags.sent;
                        f.spam |= r.flags.spam && role == Role::Inbox;
                        if f != r.flags {
                            ctx.db.sync_flags(&acc, &r.remote_id, f, None, None)?;
                            changed = true;
                        }
                    }
                }
            }
            if changed {
                ctx.emit_changed();
            }
        }
        Ok(())
    }

    async fn ensure_folder(&mut self, ctx: &Ctx, which: &str) -> Result<String> {
        let mut st: State = ctx.db.sync_state(&ctx.account.id)?;
        let mut folders = match st.folders.clone() {
            Some(f) => f,
            None => self.discover_folders(ctx).await?,
        };
        let (slot, default) = match which {
            "sent" => (&mut folders.sent, "Sent"),
            "archive" => (&mut folders.archive, "Archive"),
            "trash" => (&mut folders.trash, "Trash"),
            "spam" => (&mut folders.spam, "Junk"),
            _ => return Ok(folders.inbox.clone()),
        };
        if let Some(f) = slot.clone() {
            return Ok(f);
        }
        let s = self.session(ctx).await?;
        // May already exist without a SPECIAL-USE attribute; ignore that error.
        let _ = s.create(default).await;
        *slot = Some(default.to_string());
        st.folders = Some(folders);
        ctx.db.set_sync_state(&ctx.account.id, &st)?;
        Ok(default.to_string())
    }

    pub async fn apply(&mut self, ctx: &Ctx, action: ThreadAction, refs: &[MsgRef]) -> Result<()> {
        let mut by_folder: HashMap<String, Vec<&MsgRef>> = HashMap::new();
        for r in refs {
            if let (Some(f), Some(_)) = (&r.folder, r.imap_uid) {
                by_folder.entry(f.clone()).or_default().push(r);
            }
        }
        let target = match action {
            ThreadAction::Archive => Some(self.ensure_folder(ctx, "archive").await?),
            ThreadAction::Trash => Some(self.ensure_folder(ctx, "trash").await?),
            ThreadAction::Inbox => Some("INBOX".to_string()),
            ThreadAction::Spam => Some(self.ensure_folder(ctx, "spam").await?),
            _ => None,
        };
        for (folder, msgs) in by_folder {
            let uids: Vec<u32> = msgs.iter().filter_map(|m| m.imap_uid).collect();
            let set = uid_set(&uids);
            let has_move = self.has_move;
            let s = self.session(ctx).await?;
            s.select(&folder).await?;
            let store = |q: &'static str| q;
            match action {
                ThreadAction::Read | ThreadAction::Unread | ThreadAction::Star | ThreadAction::Unstar => {
                    let q = store(match action {
                        ThreadAction::Read => "+FLAGS.SILENT (\\Seen)",
                        ThreadAction::Unread => "-FLAGS.SILENT (\\Seen)",
                        ThreadAction::Star => "+FLAGS.SILENT (\\Flagged)",
                        _ => "-FLAGS.SILENT (\\Flagged)",
                    });
                    let _: Vec<Fetch> = s.uid_store(&set, q).await?.try_collect().await?;
                }
                ThreadAction::Archive | ThreadAction::Trash | ThreadAction::Inbox | ThreadAction::Spam => {
                    let dest = target.clone().unwrap();
                    if dest == folder {
                        continue;
                    }
                    if has_move {
                        s.uid_mv(&set, &dest).await?;
                    } else {
                        s.uid_copy(&set, &dest).await?;
                        let _: Vec<Fetch> =
                            s.uid_store(&set, "+FLAGS.SILENT (\\Deleted)").await?.try_collect().await?;
                        let _: Vec<u32> = s.uid_expunge(&set).await?.try_collect().await?;
                    }
                    // The copy in the destination folder shows up on the next sync.
                    for m in msgs {
                        ctx.db.delete_message(&ctx.account.id, &m.remote_id)?;
                    }
                }
            }
        }
        Ok(())
    }

    pub async fn send(&mut self, ctx: &Ctx, draft: &Draft) -> Result<()> {
        let built = mime::build(draft, &ctx.account, false)?;
        let secret = match self.cfg.auth {
            ImapAuth::Password => self.credential(ctx).await?,
            ImapAuth::MicrosoftOAuth => oauth_token(ctx, &config::microsoft_imap(&ctx.db)?, false).await?,
        };
        let host = self.cfg.smtp_host.as_str();
        smtp_transport(&self.cfg, &secret)?
            .send_raw(&built.envelope, &built.raw)
            .await
            .map_err(|e| anyhow!("SMTP send failed: {e}. {}", smtp_hint(&self.cfg)))?;

        // Most big providers file sent mail themselves; for the rest, append a copy.
        let auto_saves = ["gmail.com", "googlemail.com", "office365.com", "outlook.com", "icloud.com"]
            .iter()
            .any(|h| host.ends_with(h));
        if !auto_saves {
            match self.ensure_folder(ctx, "sent").await {
                Ok(sent) => {
                    let s = self.session(ctx).await?;
                    if let Err(e) = s.append(&sent, Some("(\\Seen)"), None, &built.raw).await {
                        tracing::warn!("could not save sent copy: {e}");
                    }
                }
                Err(e) => tracing::warn!("could not find or create a Sent folder: {e:#}"),
            }
        }
        Ok(())
    }

    pub async fn fetch_raw(&mut self, ctx: &Ctx, r: &MsgRef) -> Result<Vec<u8>> {
        let (Some(folder), Some(uid)) = (r.folder.clone(), r.imap_uid) else {
            bail!("message has no IMAP location");
        };
        let s = self.session(ctx).await?;
        s.select(&folder).await?;
        let fetches: Vec<Fetch> = s.uid_fetch(uid.to_string(), "BODY.PEEK[]").await?.try_collect().await?;
        fetches
            .iter()
            .find_map(|f| f.body().map(|b| b.to_vec()))
            .context("message no longer on server")
    }

    /// Verifies IMAP and SMTP settings before an account is saved.
    pub async fn test(cfg: &ImapConfig, secret: &str) -> Result<()> {
        let (mut s, _) = Self::connect_with(cfg, secret).await?;
        s.select("INBOX").await?;
        let _ = s.logout().await;
        let ok = smtp_transport(cfg, secret)?
            .test_connection()
            .await
            .map_err(|e| anyhow!("SMTP connection failed: {e}. {}", smtp_hint(cfg)))?;
        if !ok {
            bail!("SMTP server did not accept the connection");
        }
        Ok(())
    }
}

fn smtp_transport(cfg: &ImapConfig, secret: &str) -> Result<AsyncSmtpTransport<Tokio1Executor>> {
    let host = cfg.smtp_host.as_str();
    let builder = match cfg.smtp_security {
        Security::Tls => AsyncSmtpTransport::<Tokio1Executor>::relay(host)?,
        Security::StartTls => AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(host)?,
        Security::None => AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(host),
    };
    let mut builder = builder
        .port(cfg.smtp_port)
        .timeout(Some(CONNECT_TIMEOUT))
        .credentials(Credentials::new(cfg.username.clone(), secret.to_string()));
    if cfg.auth == ImapAuth::MicrosoftOAuth {
        builder = builder.authentication(vec![Mechanism::Xoauth2]);
    }
    Ok(builder.build())
}

/// Explains the most common SMTP misconfiguration: port and encryption not matching.
fn smtp_hint(cfg: &ImapConfig) -> String {
    match (cfg.smtp_port, cfg.smtp_security) {
        (587, Security::Tls) => "Port 587 uses STARTTLS, not SSL/TLS".into(),
        (465, Security::StartTls) => "Port 465 uses SSL/TLS, not STARTTLS".into(),
        _ => format!(
            "Check that {}:{} is reachable; many servers only offer port 465 (SSL/TLS) or 587 (STARTTLS)",
            cfg.smtp_host, cfg.smtp_port
        ),
    }
}

async fn port_open(host: &str, port: u16) -> bool {
    matches!(
        tokio::time::timeout(Duration::from_secs(4), TcpStream::connect((host, port))).await,
        Ok(Ok(_))
    )
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Detected {
    pub imap_host: String,
    pub imap_port: u16,
    pub imap_security: Security,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub smtp_security: Security,
}

/// Finds working IMAP/SMTP servers for a domain by probing the usual host
/// names and ports in parallel. Returns `None` if nothing answers.
pub async fn detect(domain: &str) -> Option<Detected> {
    let domain = domain.trim().trim_start_matches('@').to_lowercase();
    if domain.is_empty() {
        return None;
    }
    let imap_hosts = [format!("imap.{domain}"), format!("mail.{domain}"), domain.clone()];
    let smtp_hosts = [format!("smtp.{domain}"), format!("mail.{domain}"), domain.clone()];
    let imap_candidates: Vec<(String, u16, Security)> = imap_hosts
        .iter()
        .flat_map(|h| [(h.clone(), 993, Security::Tls), (h.clone(), 143, Security::StartTls)])
        .collect();
    let smtp_candidates: Vec<(String, u16, Security)> = smtp_hosts
        .iter()
        .flat_map(|h| [(h.clone(), 465, Security::Tls), (h.clone(), 587, Security::StartTls)])
        .collect();
    let probe = |cands: Vec<(String, u16, Security)>| async move {
        let open = futures::future::join_all(cands.iter().map(|(h, p, _)| port_open(h, *p))).await;
        // Candidates are in preference order; take the first that answered.
        cands.into_iter().zip(open).find(|(_, ok)| *ok).map(|(c, _)| c)
    };
    let (imap, smtp) = tokio::join!(probe(imap_candidates), probe(smtp_candidates));
    let (imap_host, imap_port, imap_security) = imap?;
    let (smtp_host, smtp_port, smtp_security) = smtp?;
    Some(Detected { imap_host, imap_port, imap_security, smtp_host, smtp_port, smtp_security })
}

#[cfg(test)]
mod tests {
    use super::uid_set;

    /// Hits the network; run with `cargo test -- --ignored detects`.
    #[tokio::test]
    #[ignore]
    async fn detects_real_domain() {
        let d = super::detect("ciger.dev").await;
        println!("{d:?}");
        assert!(d.is_some());
    }

    #[test]
    fn compacts_uid_sets() {
        assert_eq!(uid_set(&[5, 1, 2, 3, 8, 10, 11]), "1:3,5,8,10:11");
        assert_eq!(uid_set(&[7]), "7");
    }
}
