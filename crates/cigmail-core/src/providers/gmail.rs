//! Gmail REST API: full sync via messages.list + raw fetch, incremental sync
//! via the history API.

use std::collections::{HashMap, HashSet};

use anyhow::Result;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use futures::{StreamExt, stream};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{Ctx, LATEST_PER_FOLDER, api, b64url_decode, http_status};
use crate::{
    db::{Flags, MsgRef, ThreadAction},
    mime,
    models::Draft,
    oauth::OAuthClient,
};

const API: &str = "https://gmail.googleapis.com/gmail/v1/users/me";
const MAX_INITIAL: usize = 5000;

#[derive(Default, Serialize, Deserialize)]
struct State {
    history_id: Option<String>,
    /// The newest `LATEST_PER_FOLDER` messages per folder have been fetched once.
    #[serde(default)]
    backfilled: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawMessage {
    id: String,
    thread_id: String,
    #[serde(default)]
    label_ids: Vec<String>,
    internal_date: Option<String>,
    raw: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MinimalMessage {
    id: String,
    #[serde(default)]
    label_ids: Option<Vec<String>>,
}

pub struct Gmail {
    oauth: OAuthClient,
}

pub fn flags_from_labels(labels: &[String]) -> Flags {
    let has = |l: &str| labels.iter().any(|x| x == l);
    Flags {
        read: !has("UNREAD"),
        starred: has("STARRED"),
        inbox: has("INBOX"),
        sent: has("SENT"),
        drafts: has("DRAFT"),
        trash: has("TRASH"),
        spam: has("SPAM"),
    }
}

impl Gmail {
    pub fn new(oauth: OAuthClient) -> Self {
        Gmail { oauth }
    }

    async fn get_json(&self, ctx: &Ctx, url: &str) -> Result<Value> {
        let resp = api(ctx, &self.oauth, |c, t| c.get(url).bearer_auth(t)).await?;
        Ok(resp.json().await?)
    }

    async fn post_json(&self, ctx: &Ctx, url: &str, body: &Value) -> Result<Value> {
        let resp = api(ctx, &self.oauth, |c, t| c.post(url).bearer_auth(t).json(body)).await?;
        let text = resp.text().await?;
        Ok(if text.is_empty() { Value::Null } else { serde_json::from_str(&text)? })
    }

    pub async fn sync(&mut self, ctx: &Ctx) -> Result<()> {
        let mut st: State = ctx.db.sync_state(&ctx.account.id)?;
        match st.history_id.clone() {
            None => self.full_sync(ctx, &mut st).await?,
            Some(h) => match self.incremental(ctx, &h, &mut st).await {
                Ok(()) => {}
                // The history id is too old: start over.
                Err(e) if http_status(&e) == Some(404) => {
                    tracing::info!("gmail history expired for {}, resyncing", ctx.account.email);
                    st.history_id = None;
                    self.full_sync(ctx, &mut st).await?;
                }
                Err(e) => return Err(e),
            },
        }
        ctx.db.set_sync_state(&ctx.account.id, &st)?;
        if !st.backfilled {
            self.backfill(ctx).await?;
            st.backfilled = true;
            ctx.db.set_sync_state(&ctx.account.id, &st)?;
        }
        Ok(())
    }

    /// The listing above only covers the sync window; also fetch the newest
    /// messages in the inbox, in Sent and overall, however old they are.
    async fn backfill(&self, ctx: &Ctx) -> Result<()> {
        let mut ids: Vec<String> = Vec::new();
        for filter in ["&labelIds=INBOX", "&labelIds=SENT", ""] {
            let v = self
                .get_json(ctx, &format!("{API}/messages?maxResults={LATEST_PER_FOLDER}{filter}"))
                .await?;
            for id in v["messages"].as_array().into_iter().flatten().filter_map(|m| m["id"].as_str()) {
                if !ids.iter().any(|i| i == id) && !ctx.db.message_exists(&ctx.account.id, id)? {
                    ids.push(id.to_string());
                }
            }
        }
        self.download(ctx, ids).await
    }

    async fn full_sync(&self, ctx: &Ctx, st: &mut State) -> Result<()> {
        // Take the history id before listing so nothing that changes while we
        // download falls through the cracks.
        let profile = self.get_json(ctx, &format!("{API}/profile")).await?;
        let history_id = profile["historyId"].as_str().map(String::from);

        let days = ctx.sync_days();
        let mut ids: Vec<String> = Vec::new();
        let mut page: Option<String> = None;
        loop {
            let mut url = format!("{API}/messages?maxResults=500&q=newer_than:{days}d");
            if let Some(p) = &page {
                url.push_str(&format!("&pageToken={p}"));
            }
            let v = self.get_json(ctx, &url).await?;
            if let Some(arr) = v["messages"].as_array() {
                ids.extend(arr.iter().filter_map(|m| m["id"].as_str().map(String::from)));
            }
            page = v["nextPageToken"].as_str().map(String::from);
            if page.is_none() || ids.len() >= MAX_INITIAL {
                break;
            }
        }
        let missing: Vec<String> = ids
            .into_iter()
            .filter(|id| !ctx.db.message_exists(&ctx.account.id, id).unwrap_or(false))
            .collect();
        self.download(ctx, missing).await?;
        st.history_id = history_id;
        Ok(())
    }

    async fn download(&self, ctx: &Ctx, ids: Vec<String>) -> Result<()> {
        let total = ids.len() as u32;
        if total == 0 {
            return Ok(());
        }
        let mut done = 0u32;
        let mut failed = 0u32;
        let mut first_err: Option<anyhow::Error> = None;
        let mut results = stream::iter(ids)
            .map(|id| async move { self.fetch_and_store(ctx, &id).await })
            .buffer_unordered(8);
        while let Some(r) = results.next().await {
            if let Err(e) = r {
                tracing::warn!("gmail: failed to fetch message: {e:#}");
                if e.is::<crate::error::AuthExpired>() {
                    return Err(e);
                }
                failed += 1;
                first_err.get_or_insert(e);
            }
            done += 1;
            if done % 20 == 0 || done == total {
                ctx.emit_progress(done, total);
                ctx.emit_changed();
            }
        }
        // The caller keeps its history id, so these are listed again next sync.
        if let Some(e) = first_err {
            anyhow::bail!("{failed} of {total} messages could not be downloaded: {e:#}");
        }
        Ok(())
    }

    async fn fetch_and_store(&self, ctx: &Ctx, id: &str) -> Result<()> {
        let url = format!("{API}/messages/{id}?format=raw");
        let resp = api(ctx, &self.oauth, |c, t| c.get(&url).bearer_auth(t)).await;
        let m: RawMessage = match resp {
            Ok(r) => r.json().await?,
            // Deleted between listing and fetching.
            Err(e) if http_status(&e) == Some(404) => return Ok(()),
            Err(e) => return Err(e),
        };
        let raw = b64url_decode(m.raw.as_deref().unwrap_or(""))?;
        let date = m
            .internal_date
            .and_then(|d| d.parse::<i64>().ok())
            .map(|ms| ms / 1000);
        ctx.ingest(
            &m.id,
            Some(&m.thread_id),
            &raw,
            flags_from_labels(&m.label_ids),
            m.label_ids,
            None,
            None,
            date,
            None,
        )
    }

    async fn incremental(&self, ctx: &Ctx, start: &str, st: &mut State) -> Result<()> {
        let mut added: HashSet<String> = HashSet::new();
        let mut deleted: HashSet<String> = HashSet::new();
        let mut labels: HashMap<String, Vec<String>> = HashMap::new();
        let mut page: Option<String> = None;
        let mut latest = start.to_string();
        loop {
            let mut url = format!("{API}/history?startHistoryId={start}&maxResults=500");
            if let Some(p) = &page {
                url.push_str(&format!("&pageToken={p}"));
            }
            let v = self.get_json(ctx, &url).await?;
            if let Some(h) = v["historyId"].as_str() {
                latest = h.to_string();
            }
            for rec in v["history"].as_array().into_iter().flatten() {
                for m in rec["messagesAdded"].as_array().into_iter().flatten() {
                    if let Some(id) = m["message"]["id"].as_str() {
                        deleted.remove(id);
                        added.insert(id.to_string());
                    }
                }
                for m in rec["messagesDeleted"].as_array().into_iter().flatten() {
                    if let Some(id) = m["message"]["id"].as_str() {
                        added.remove(id);
                        labels.remove(id);
                        deleted.insert(id.to_string());
                    }
                }
                for key in ["labelsAdded", "labelsRemoved"] {
                    for m in rec[key].as_array().into_iter().flatten() {
                        let msg: MinimalMessage = match serde_json::from_value(m["message"].clone()) {
                            Ok(x) => x,
                            Err(_) => continue,
                        };
                        if let Some(l) = msg.label_ids {
                            labels.insert(msg.id, l);
                        }
                    }
                }
            }
            page = v["nextPageToken"].as_str().map(String::from);
            if page.is_none() {
                break;
            }
        }

        for id in &deleted {
            ctx.db.delete_message(&ctx.account.id, id)?;
        }
        let mut to_fetch: Vec<String> = added
            .into_iter()
            .filter(|id| !ctx.db.message_exists(&ctx.account.id, id).unwrap_or(false))
            .collect();
        for (id, l) in labels {
            if to_fetch.contains(&id) {
                continue;
            }
            let updated =
                ctx.db
                    .sync_flags(&ctx.account.id, &id, flags_from_labels(&l), Some(&l), None)?;
            if !updated {
                to_fetch.push(id);
            }
        }
        if !deleted.is_empty() {
            ctx.emit_changed();
        }
        self.download(ctx, to_fetch).await?;
        st.history_id = Some(latest);
        ctx.emit_changed();
        Ok(())
    }

    pub async fn apply(&mut self, ctx: &Ctx, action: ThreadAction, refs: &[MsgRef]) -> Result<()> {
        let ids: Vec<&str> = refs.iter().map(|r| r.remote_id.as_str()).collect();
        let modify = |add: &[&str], remove: &[&str]| {
            json!({ "ids": ids, "addLabelIds": add, "removeLabelIds": remove })
        };
        let url = format!("{API}/messages/batchModify");
        match action {
            ThreadAction::Read => self.post_json(ctx, &url, &modify(&[], &["UNREAD"])).await?,
            ThreadAction::Unread => self.post_json(ctx, &url, &modify(&["UNREAD"], &[])).await?,
            ThreadAction::Star => self.post_json(ctx, &url, &modify(&["STARRED"], &[])).await?,
            ThreadAction::Unstar => self.post_json(ctx, &url, &modify(&[], &["STARRED"])).await?,
            ThreadAction::Archive => self.post_json(ctx, &url, &modify(&[], &["INBOX"])).await?,
            ThreadAction::Spam => self.post_json(ctx, &url, &modify(&["SPAM"], &["INBOX"])).await?,
            ThreadAction::Inbox => {
                for r in refs.iter().filter(|r| r.flags.trash) {
                    self.post_json(ctx, &format!("{API}/messages/{}/untrash", r.remote_id), &json!({}))
                        .await?;
                }
                // Restored sent copies keep their SENT label and stay out of the inbox.
                let inbox: Vec<&str> = refs.iter().filter(|r| r.flags.inbox).map(|r| r.remote_id.as_str()).collect();
                self.post_json(ctx, &url, &modify(&[], &["SPAM", "TRASH"])).await?;
                if !inbox.is_empty() {
                    self.post_json(ctx, &url, &json!({ "ids": inbox, "addLabelIds": ["INBOX"] })).await?
                } else {
                    Value::Null
                }
            }
            ThreadAction::Trash => {
                for r in refs {
                    self.post_json(ctx, &format!("{API}/messages/{}/trash", r.remote_id), &json!({}))
                        .await?;
                }
                Value::Null
            }
        };
        Ok(())
    }

    pub async fn send(&mut self, ctx: &Ctx, draft: &Draft) -> Result<()> {
        let built = mime::build(draft, &ctx.account, true)?;
        let mut body = json!({ "raw": URL_SAFE_NO_PAD.encode(&built.raw) });
        // Keep replies in the same Gmail thread.
        let prefix = format!("{}:", ctx.account.id);
        if let Some(t) = draft.thread_id.as_deref().and_then(|t| t.strip_prefix(&prefix)) {
            if !t.starts_with("t:") {
                body["threadId"] = json!(t);
            }
        }
        let sent = self.post_json(ctx, &format!("{API}/messages/send"), &body).await?;
        if let Some(id) = sent["id"].as_str() {
            // Show the sent message in its thread right away.
            if let Err(e) = self.fetch_and_store(ctx, id).await {
                tracing::warn!("could not fetch sent message: {e:#}");
            }
            ctx.emit_changed();
        }
        Ok(())
    }

    /// Saves the draft to Gmail's Drafts, unsent.
    pub async fn save_draft(&mut self, ctx: &Ctx, draft: &Draft) -> Result<()> {
        let built = mime::build(draft, &ctx.account, true)?;
        let mut message = json!({ "raw": URL_SAFE_NO_PAD.encode(&built.raw) });
        // Keep replies in the same Gmail thread.
        let prefix = format!("{}:", ctx.account.id);
        if let Some(t) = draft.thread_id.as_deref().and_then(|t| t.strip_prefix(&prefix)) {
            if !t.starts_with("t:") {
                message["threadId"] = json!(t);
            }
        }
        let saved = self.post_json(ctx, &format!("{API}/drafts"), &json!({ "message": message })).await?;
        if let Some(id) = saved["message"]["id"].as_str() {
            if let Err(e) = self.fetch_and_store(ctx, id).await {
                tracing::warn!("could not fetch the saved draft: {e:#}");
            }
            ctx.emit_changed();
        }
        Ok(())
    }

    pub async fn fetch_raw(&mut self, ctx: &Ctx, remote_id: &str) -> Result<Vec<u8>> {
        let v = self
            .get_json(ctx, &format!("{API}/messages/{remote_id}?format=raw"))
            .await?;
        b64url_decode(v["raw"].as_str().unwrap_or(""))
    }
}
