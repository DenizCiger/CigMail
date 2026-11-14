//! Parsing incoming RFC 5322 messages and building outgoing ones. Every
//! provider hands us raw MIME, so this is the single place where mail content
//! is interpreted.

use std::path::Path;

use anyhow::{Context, Result};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use lettre::message::{
    Attachment, Mailbox, MultiPart, SinglePart,
    header::{ContentType, InReplyTo, References},
};
use mail_parser::{MessageParser, PartKind};

use crate::models::{Account, Address, AttachmentMeta, Draft, Message};

const MAX_HTML: usize = 5 * 1024 * 1024;
const MAX_INLINE_IMAGE: usize = 1024 * 1024;

#[derive(Debug, Clone)]
pub struct Parsed {
    pub message_id: Option<String>,
    pub in_reply_to: Option<String>,
    pub refs: Vec<String>,
    pub subject: String,
    pub from: Address,
    pub to: Vec<Address>,
    pub cc: Vec<Address>,
    pub bcc: Vec<Address>,
    pub reply_to: Vec<Address>,
    pub date: Option<i64>,
    pub snippet: String,
    pub body_text: Option<String>,
    pub body_html: Option<String>,
    pub attachments: Vec<AttachmentMeta>,
    pub list_unsubscribe: Option<String>,
    pub bulk: bool,
    /// The receiving server's spam verdict, from headers like X-Spam-Flag.
    pub spam_flagged: bool,
    /// SPF or DMARC failed, i.e. the From address is probably forged.
    pub auth_failed: bool,
}

fn addresses(list: Option<mail_parser::AddressList<'_>>) -> Vec<Address> {
    let Some(list) = list else { return Vec::new() };
    list.mailboxes()
        .filter_map(|m| {
            let email = m.address()?.trim().to_string();
            if email.is_empty() {
                return None;
            }
            Some(Address {
                name: m.name().map(|n| n.trim().trim_matches('"').to_string()).filter(|n| !n.is_empty()),
                email,
            })
        })
        .collect()
}

fn clean_id(s: &str) -> String {
    s.trim().trim_start_matches('<').trim_end_matches('>').to_string()
}

pub fn html_to_text(html: &str) -> String {
    html2text::from_read(html.as_bytes(), 100).unwrap_or_default()
}

/// Removes decoration that html-to-text conversion leaves behind (horizontal
/// rules drawn with box characters, long runs of `-`, `=`, `_` ...).
pub fn clean_snippet(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut run_char = '\0';
    let mut run = 0;
    for ch in s.chars() {
        if ('\u{2500}'..='\u{257F}').contains(&ch) || ch == '\u{00A0}' || ch == '\u{200C}' || ch == '\u{034F}' {
            continue;
        }
        if ch == run_char && matches!(ch, '-' | '=' | '_' | '*' | '~' | '.' | '#') {
            run += 1;
            if run >= 3 {
                continue;
            }
        } else {
            run_char = ch;
            run = 1;
        }
        out.push(ch);
    }
    out.split_whitespace()
        .filter(|w| !w.chars().all(|c| matches!(c, '-' | '=' | '_' | '*' | '~' | '.' | '#' | '|')) || w.len() == 1)
        .collect::<Vec<_>>()
        .join(" ")
}

/// A one-line preview without quoted replies or excess whitespace.
pub fn make_snippet(text: &str) -> String {
    let mut out = String::new();
    for line in text.lines() {
        let l = line.trim();
        if l.starts_with('>') {
            continue;
        }
        // Stop at the "On <date>, <person> wrote:" line that introduces a quote.
        if l.starts_with("On ") && l.ends_with("wrote:") {
            break;
        }
        if l.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(l);
        if out.len() > 300 {
            break;
        }
    }
    clean_snippet(&out).chars().take(220).collect()
}

pub fn parse(raw: &[u8]) -> Result<Parsed> {
    let msg = MessageParser::new().parse(raw).context("unparseable message")?;

    let has_html_part = msg.html_body().any(|p| matches!(p.kind(), PartKind::Html));
    let mut body_html = if has_html_part {
        let joined: String = msg.html_bodies().collect::<Vec<_>>().join("\n");
        Some(joined)
    } else {
        None
    };

    let mut attachments = Vec::new();
    for (index, part) in msg.attachments().enumerate() {
        let ct = part.content_type();
        let mime = ct
            .map(|c| format!("{}/{}", c.ctype(), c.subtype().unwrap_or("octet-stream")))
            .unwrap_or_else(|| "application/octet-stream".into())
            .to_lowercase();
        let content_id = part.content_id().map(clean_id);
        let inline = part.is_inline() && content_id.is_some();
        // Embed small inline images (logos, signatures) so they render offline
        // without loading anything remote.
        if let (Some(cid), Some(html)) = (&content_id, body_html.as_mut()) {
            if mime.starts_with("image/") && part.decoded_len() <= MAX_INLINE_IMAGE {
                let needle = format!("cid:{cid}");
                if html.contains(&needle) {
                    let data = format!("data:{mime};base64,{}", STANDARD.encode(part.decoded()));
                    *html = html.replace(&needle, &data);
                }
            }
        }
        attachments.push(AttachmentMeta {
            index,
            filename: part
                .attachment_name()
                .map(String::from)
                .unwrap_or_else(|| format!("attachment-{}", index + 1)),
            mime,
            size: part.decoded_len(),
            inline,
            content_id,
        });
    }

    if let Some(h) = &body_html {
        if h.len() > MAX_HTML {
            body_html = None;
        }
    }

    let text: String = msg.text_bodies().collect::<Vec<_>>().join("\n");
    let body_text = if !text.trim().is_empty() && !has_html_part {
        Some(text)
    } else if let Some(h) = &body_html {
        Some(html_to_text(h))
    } else if !text.trim().is_empty() {
        Some(text)
    } else {
        None
    };

    let headers = msg.headers();
    let bulk = headers.get("List-Id").is_some()
        || headers
            .get("Precedence")
            .and_then(|h| std::str::from_utf8(h.raw_value()).ok().map(|v| v.trim().to_ascii_lowercase()))
            .is_some_and(|v| v == "bulk" || v == "list");
    let header_text = |name: &str| -> Option<String> {
        headers
            .get(name)
            .and_then(|h| std::str::from_utf8(h.raw_value()).ok())
            .map(|v| v.split_whitespace().collect::<Vec<_>>().join(" ").to_ascii_lowercase())
    };
    let spam_flagged = header_text("X-Spam-Flag").is_some_and(|v| v.starts_with("yes"))
        || header_text("X-Spam-Status").is_some_and(|v| v.starts_with("yes"))
        || header_text("X-Spam").is_some_and(|v| v.starts_with("yes") || v.starts_with("true"))
        || header_text("X-Rspamd-Action")
            .is_some_and(|v| v.starts_with("add header") || v.starts_with("rewrite subject") || v.starts_with("reject"))
        || header_text("X-Spam-Level").is_some_and(|v| v.chars().filter(|c| *c == '*').count() >= 6);
    let auth_failed = header_text("Authentication-Results")
        .is_some_and(|v| v.contains("dmarc=fail") || v.contains("spf=fail"));
    let list_unsubscribe = headers
        .get("List-Unsubscribe")
        .and_then(|h| std::str::from_utf8(h.raw_value()).ok())
        .map(|v| v.split_whitespace().collect::<Vec<_>>().join(" "));

    let from = addresses(msg.from()).into_iter().next().unwrap_or(Address {
        name: None,
        email: String::new(),
    });

    Ok(Parsed {
        message_id: msg.message_id().map(clean_id),
        in_reply_to: msg.in_reply_to().and_then(|l| l.first().map(clean_id)),
        refs: msg
            .references()
            .map(|l| l.iter().map(clean_id).collect())
            .unwrap_or_default(),
        subject: msg.subject().unwrap_or("").trim().to_string(),
        from,
        to: addresses(msg.to()),
        cc: addresses(msg.cc()),
        bcc: addresses(msg.bcc()),
        reply_to: addresses(msg.reply_to()),
        date: msg.date().map(|d| d.to_timestamp()),
        snippet: body_text.as_deref().map(make_snippet).unwrap_or_default(),
        body_text,
        body_html,
        attachments,
        list_unsubscribe,
        bulk,
        spam_flagged,
        auth_failed,
    })
}

/// Decides which split a message falls into: "primary", "newsletter" or "notification".
pub fn classify(from_email: &str, labels: &[String], bulk: bool, list_unsubscribe: bool) -> &'static str {
    if labels.iter().any(|l| l == "CATEGORY_PROMOTIONS") {
        return "newsletter";
    }
    if labels
        .iter()
        .any(|l| matches!(l.as_str(), "CATEGORY_SOCIAL" | "CATEGORY_UPDATES" | "CATEGORY_FORUMS"))
    {
        return "notification";
    }
    let local = from_email.split('@').next().unwrap_or("").to_ascii_lowercase();
    const NOTIFY: &[&str] = &[
        "noreply", "no-reply", "no_reply", "donotreply", "do-not-reply", "do_not_reply",
        "notification", "notifications", "notify", "alert", "alerts", "mailer-daemon",
        "postmaster", "automated", "security", "bounce", "bounces", "system", "reminder",
    ];
    if NOTIFY.iter().any(|n| local == *n || local.starts_with(&format!("{n}-")) || local.starts_with(&format!("{n}+")) || local.contains("noreply") || local.contains("no-reply")) {
        return "notification";
    }
    const NEWS: &[&str] = &["newsletter", "news", "marketing", "promo", "offers", "deals", "digest"];
    if bulk || list_unsubscribe || NEWS.iter().any(|n| local.contains(n)) {
        return "newsletter";
    }
    "primary"
}

// ------------------------------------------------------------------ outgoing

fn mailbox(a: &Address) -> Result<Mailbox> {
    Ok(Mailbox::new(
        a.name.clone().filter(|n| !n.is_empty()),
        a.email.trim().parse().with_context(|| format!("invalid address: {}", a.email))?,
    ))
}

fn guess_mime(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("pdf") => "application/pdf",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("svg") => "image/svg+xml",
        Some("txt" | "md") => "text/plain",
        Some("html" | "htm") => "text/html",
        Some("csv") => "text/csv",
        Some("json") => "application/json",
        Some("zip") => "application/zip",
        Some("doc") => "application/msword",
        Some("docx") => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        Some("xls") => "application/vnd.ms-excel",
        Some("xlsx") => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        Some("ppt") => "application/vnd.ms-powerpoint",
        Some("pptx") => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        Some("ics") => "text/calendar",
        _ => "application/octet-stream",
    }
}

pub struct Built {
    pub raw: Vec<u8>,
    pub envelope: lettre::address::Envelope,
}

/// Renders a draft to raw MIME. `keep_bcc` must be true for API providers
/// (Gmail/Graph read recipients from the headers) and false for SMTP.
pub fn build(draft: &Draft, from: &Account, keep_bcc: bool) -> Result<Built> {
    let domain = from.email.split('@').nth(1).unwrap_or("cigmail.local");
    let message_id = format!("{}@{}", uuid::Uuid::new_v4().simple(), domain);

    let mut b = lettre::Message::builder()
        .from(mailbox(&Address { name: Some(from.name.clone()), email: from.email.clone() })?)
        .subject(draft.subject.clone())
        .message_id(Some(format!("<{message_id}>")))
        .date_now()
        .user_agent("CigMail".into());
    for a in &draft.to {
        b = b.to(mailbox(a)?);
    }
    for a in &draft.cc {
        b = b.cc(mailbox(a)?);
    }
    for a in &draft.bcc {
        b = b.bcc(mailbox(a)?);
    }
    if keep_bcc {
        b = b.keep_bcc();
    }
    if let Some(irt) = &draft.in_reply_to {
        b = b.header(InReplyTo::from(format!("<{}>", clean_id(irt))));
    }
    if !draft.references.is_empty() {
        let refs = draft
            .references
            .iter()
            .map(|r| format!("<{}>", clean_id(r)))
            .collect::<Vec<_>>()
            .join(" ");
        b = b.header(References::from(refs));
    }

    let mut html = draft.html.clone();
    if let Some(q) = &draft.quoted_html {
        html.push_str("<br>");
        html.push_str(q);
    }
    let html_doc = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"></head><body>{html}</body></html>"
    );
    let text = if draft.text.trim().is_empty() { html_to_text(&html) } else { draft.text.clone() };
    let alternative = MultiPart::alternative_plain_html(text, html_doc);

    let message = if draft.attachments.is_empty() {
        b.multipart(alternative)?
    } else {
        let mut mixed = MultiPart::mixed().multipart(alternative);
        for att in &draft.attachments {
            let path = Path::new(&att.path);
            let bytes = std::fs::read(path).with_context(|| format!("reading {}", att.path))?;
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("attachment").to_string();
            let part: SinglePart =
                Attachment::new(name).body(bytes, ContentType::parse(guess_mime(path))?);
            mixed = mixed.singlepart(part);
        }
        b.multipart(mixed)?
    };
    let envelope = message.envelope().clone();
    Ok(Built { raw: message.formatted(), envelope })
}

pub fn extract_attachment(raw: &[u8], index: usize) -> Result<(String, String, Vec<u8>)> {
    let msg = MessageParser::new().parse(raw).context("unparseable message")?;
    let part = msg.attachments().nth(index).context("attachment not found")?;
    let name = part
        .attachment_name()
        .map(String::from)
        .unwrap_or_else(|| format!("attachment-{}", index + 1));
    let mime = part
        .content_type()
        .map(|c| format!("{}/{}", c.ctype(), c.subtype().unwrap_or("octet-stream")))
        .unwrap_or_else(|| "application/octet-stream".into());
    Ok((name, mime, part.decoded().into_owned()))
}

// ------------------------------------------------------------------ replies

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

pub fn text_to_html(t: &str) -> String {
    escape(t).replace("\r\n", "\n").replace('\n', "<br>")
}

fn strip_prefixes(subject: &str) -> &str {
    let mut s = subject.trim();
    loop {
        let lower = s.to_ascii_lowercase();
        let stripped = ["re:", "fwd:", "fw:", "aw:", "wg:", "sv:"]
            .iter()
            .find(|p| lower.starts_with(*p))
            .map(|p| s[p.len()..].trim_start());
        match stripped {
            Some(rest) => s = rest,
            None => return s,
        }
    }
}

fn dedupe(list: Vec<Address>, exclude: &[String]) -> Vec<Address> {
    let mut out: Vec<Address> = Vec::new();
    for a in list {
        let e = a.email.to_lowercase();
        if exclude.contains(&e) || out.iter().any(|o| o.email.to_lowercase() == e) {
            continue;
        }
        out.push(a);
    }
    out
}

/// What sits between `<body>` and `</body>`, or all of it for a fragment.
fn body_inner(html: &str) -> &str {
    let lower = html.to_ascii_lowercase();
    let Some(open) = lower.find("<body") else { return html };
    let Some(start) = lower[open..].find('>').map(|i| open + i + 1) else { return html };
    let end = lower.rfind("</body>").filter(|e| *e >= start).unwrap_or(html.len());
    &html[start..end]
}

/// Turns a draft from the account's Drafts folder back into something the composer can edit.
/// A quoted original in it is part of the text now, not a separate quote.
pub fn draft_from_message(msg: &Message) -> Draft {
    let html = match &msg.body_html {
        Some(h) => body_inner(h).trim().to_string(),
        None => text_to_html(msg.body_text.as_deref().unwrap_or("").trim()),
    };
    let is_reply = !msg.references.is_empty();
    Draft {
        account_id: msg.account_id.clone(),
        to: msg.to.clone(),
        cc: msg.cc.clone(),
        bcc: msg.bcc.clone(),
        subject: msg.subject.clone(),
        html,
        // By convention the last reference is the message being answered.
        in_reply_to: msg.references.last().cloned(),
        references: msg.references.clone(),
        thread_id: is_reply.then(|| msg.thread_id.clone()),
        mode: Some(if is_reply { "reply" } else { "new" }.into()),
        replaces: Some(msg.id.clone()),
        ..Default::default()
    }
}

pub fn reply_draft(msg: &Message, account: &Account, mode: &str) -> Draft {
    let me = vec![account.email.to_lowercase()];
    let from_me = msg.from.email.eq_ignore_ascii_case(&account.email);
    let primary: Vec<Address> = if from_me {
        msg.to.clone()
    } else if !msg.reply_to.is_empty() {
        msg.reply_to.clone()
    } else {
        vec![msg.from.clone()]
    };

    let date = chrono::DateTime::from_timestamp(msg.date, 0)
        .map(|d| d.with_timezone(&chrono::Local).format("%a, %b %-d, %Y at %H:%M").to_string())
        .unwrap_or_default();
    let body = msg
        .body_html
        .clone()
        .unwrap_or_else(|| text_to_html(msg.body_text.as_deref().unwrap_or("")));

    let base = strip_prefixes(&msg.subject).to_string();
    let mut refs = msg.references.clone();
    if let Some(id) = &msg.message_id {
        refs.push(id.clone());
    }

    match mode {
        "forward" => {
            let header = format!(
                "---------- Forwarded message ----------<br>From: {}<br>Date: {}<br>Subject: {}<br>To: {}<br><br>",
                escape(&msg.from.display()),
                escape(&date),
                escape(&msg.subject),
                escape(&msg.to.iter().map(Address::display).collect::<Vec<_>>().join(", ")),
            );
            Draft {
                account_id: account.id.clone(),
                subject: format!("Fwd: {base}"),
                thread_id: Some(msg.thread_id.clone()),
                mode: Some("forward".into()),
                quoted_html: Some(format!("<div class=\"cm-forward\">{header}{body}</div>")),
                ..Default::default()
            }
        }
        _ => {
            let (to, cc) = if mode == "replyAll" {
                let to = dedupe(primary.iter().cloned().chain(msg.to.iter().cloned()).collect(), &me);
                let taken: Vec<String> = to.iter().map(|a| a.email.to_lowercase()).chain(me.clone()).collect();
                (to, dedupe(msg.cc.clone(), &taken))
            } else {
                (dedupe(primary, if from_me { &[] } else { &me }), Vec::new())
            };
            Draft {
                account_id: account.id.clone(),
                to,
                cc,
                subject: format!("Re: {base}"),
                in_reply_to: msg.message_id.clone(),
                references: refs,
                thread_id: Some(msg.thread_id.clone()),
                mode: Some(mode.to_string()),
                quoted_html: Some(format!(
                    "<div class=\"cm-quote\"><p>On {}, {} wrote:</p><blockquote style=\"margin:0 0 0 .8ex;border-left:1px solid #ccc;padding-left:1ex\">{body}</blockquote></div>",
                    escape(&date),
                    escape(&msg.from.display()),
                )),
                ..Default::default()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_saved_draft_opens_in_the_composer_again() {
        let account = Account {
            id: "acc".into(),
            kind: crate::models::AccountKind::Imap,
            email: "anna@example.org".into(),
            name: "Anna".into(),
            label: None,
            color: String::new(),
            created_at: 0,
            last_sync: None,
            last_error: None,
            signature: None,
        };
        let written = Draft {
            account_id: "acc".into(),
            to: vec![Address { name: Some("Mia Huber".into()), email: "mia@example.net".into() }],
            cc: vec![Address { name: None, email: "tom@example.net".into() }],
            subject: "Re: Lunch on Friday?".into(),
            html: "<p>Friday works &amp; I'll bring Tom.</p>".into(),
            quoted_html: Some("<blockquote>Are you free?</blockquote>".into()),
            in_reply_to: Some("m1@example.net".into()),
            references: vec!["m0@example.net".into(), "m1@example.net".into()],
            ..Default::default()
        };
        // What an assistant's draft looks like once it has been to the mail server and back.
        let parsed = parse(&build(&written, &account, true).unwrap().raw).unwrap();
        let stored = Message {
            id: "acc:42".into(),
            account_id: "acc".into(),
            thread_id: "acc:t:m0@example.net".into(),
            message_id: parsed.message_id,
            references: parsed.refs,
            subject: parsed.subject,
            from: parsed.from,
            to: parsed.to,
            cc: parsed.cc,
            bcc: parsed.bcc,
            reply_to: vec![],
            date: 0,
            snippet: String::new(),
            body_html: parsed.body_html,
            body_text: parsed.body_text,
            is_read: true,
            is_starred: false,
            is_draft: true,
            in_sent: false,
            attachments: vec![],
            list_unsubscribe: None,
        };
        let d = draft_from_message(&stored);
        assert_eq!(d.replaces.as_deref(), Some("acc:42"), "sending it removes the copy in Drafts");
        assert_eq!((d.account_id.as_str(), d.subject.as_str()), ("acc", "Re: Lunch on Friday?"));
        assert_eq!(d.to, written.to);
        assert_eq!(d.cc, written.cc);
        assert_eq!(d.html, "<p>Friday works &amp; I'll bring Tom.</p><br><blockquote>Are you free?</blockquote>", "the body without the document around it");
        assert_eq!(d.quoted_html, None, "the quote is part of the text now, not added a second time");
        assert_eq!((d.in_reply_to.as_deref(), d.references.len()), (Some("m1@example.net"), 2), "still a reply in its thread");
        assert_eq!((d.thread_id.as_deref(), d.mode.as_deref()), (Some("acc:t:m0@example.net"), Some("reply")));

        assert_eq!(body_inner("<HTML><BODY class=x>hi</BODY></HTML>"), "hi");
        assert_eq!(body_inner("<p>just a fragment</p>"), "<p>just a fragment</p>");
        let plain = draft_from_message(&Message { body_html: None, body_text: Some("a\nb".into()), references: vec![], ..stored });
        assert_eq!((plain.html.as_str(), plain.mode.as_deref(), plain.thread_id), ("a<br>b", Some("new"), None));
    }

    #[test]
    fn parses_basic_message() {
        let raw = b"From: Ann <ann@example.com>\r\nTo: bob@example.com\r\nSubject: Hi\r\nMessage-ID: <abc@x>\r\nReferences: <r1@x> <r2@x>\r\nList-Unsubscribe: <mailto:u@x>\r\n\r\nHello there\r\n> quoted\r\n";
        let p = parse(raw).unwrap();
        assert_eq!(p.subject, "Hi");
        assert_eq!(p.from.email, "ann@example.com");
        assert_eq!(p.from.name.as_deref(), Some("Ann"));
        assert_eq!(p.message_id.as_deref(), Some("abc@x"));
        assert_eq!(p.refs, vec!["r1@x", "r2@x"]);
        assert_eq!(p.snippet, "Hello there");
        assert!(p.list_unsubscribe.is_some());
    }

    #[test]
    fn classifies() {
        assert_eq!(classify("noreply@github.com", &[], false, true), "notification");
        assert_eq!(classify("hello@shop.com", &[], false, true), "newsletter");
        assert_eq!(classify("prof@uni.edu", &[], false, false), "primary");
        assert_eq!(classify("x@y.com", &["CATEGORY_PROMOTIONS".into()], false, false), "newsletter");
    }

    #[test]
    fn cleans_snippets() {
        assert_eq!(clean_snippet("New login ────────── from Linux"), "New login from Linux");
        assert_eq!(clean_snippet("Hi -- there ===== ok"), "Hi there ok");
    }

    #[test]
    fn detects_spam_headers() {
        let raw = b"From: x@y.com\r\nX-Spam-Flag: YES\r\nSubject: s\r\n\r\nbody";
        assert!(parse(raw).unwrap().spam_flagged);
        let raw = b"From: me@ciger.dev\r\nAuthentication-Results: mx; spf=fail smtp.mailfrom=ciger.dev; dmarc=fail\r\n\r\nbody";
        assert!(parse(raw).unwrap().auth_failed);
    }

    #[test]
    fn strips_subject_prefixes() {
        assert_eq!(strip_prefixes("Re: Fwd: RE: hello"), "hello");
    }
}
