//! Microsoft Graph (Outlook.com and Microsoft 365). Uses per-folder delta
//! queries for sync and immutable ids so messages keep their id when moved.

use std::collections::HashMap;

use anyhow::Result;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use futures::{StreamExt, stream};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{Ctx, api, http_status};
use crate::{
    db::{Flags, MsgRef, ThreadAction},
    mime,
    models::Draft,
    oauth::OAuthClient,
};

const API: &str = "https://graph.microsoft.com/v1.0/me";
const PREFER: &str = r#"IdType="ImmutableId", odata.maxpagesize=100"#;
const SYNCED: [&str; 3] = ["inbox", "sentitems", "archive"];
const KNOWN: [&str; 6] = ["inbox", "sentitems", "archive", "deleteditems", "junkemail", "drafts"];
const SELECT: &str = "id,conversationId,isRead,flag,parentFolderId,receivedDateTime,isDraft,inferenceClassification";

#[derive(Default, Serialize, Deserialize)]
struct State {
    /// well-known name -> folder id
    folders: HashMap<String, String>,
    /// well-known name -> deltaLink
    delta: HashMap<String, String>,
}

pub struct Graph {
    oauth: OAuthClient,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Item {
    id: String,
    conversation_id: Option<String>,
    is_read: Option<bool>,
    flag: Option<Value>,
    parent_folder_id: Option<String>,
    received_date_time: Option<String>,
    is_draft: Option<bool>,
    inference_classification: Option<String>,
    #[serde(rename = "@removed")]
    removed: Option<Value>,
}

impl State {
    fn role_of(&self, folder_id: &str) -> Option<&str> {
        self.folders
            .iter()
            .find(|(_, id)| id.as_str() == folder_id)
            .map(|(k, _)| k.as_str())
    }

    fn flags(&self, item: &Item) -> Flags {
        let role = item.parent_folder_id.as_deref().and_then(|f| self.role_of(f));
        Flags {
            read: item.is_read.unwrap_or(true),
            starred: item
                .flag
                .as_ref()
                .and_then(|f| f["flagStatus"].as_str())
                .is_some_and(|s| s == "flagged"),
            inbox: role == Some("inbox"),
            sent: role == Some("sentitems"),
            drafts: role == Some("drafts") || item.is_draft.unwrap_or(false),
            trash: role == Some("deleteditems"),
            spam: role == Some("junkemail"),
        }
    }
}

impl Graph {
    pub fn new(oauth: OAuthClient) -> Self {
        Graph { oauth }
    }

    async fn get_json(&self, ctx: &Ctx, url: &str) -> Result<Value> {
        let resp = api(ctx, &self.oauth, |c, t| c.get(url).bearer_auth(t).header("Prefer", PREFER)).await?;
        Ok(resp.json().await?)
    }

    async fn send_json(&self, ctx: &Ctx, method: reqwest::Method, url: &str, body: &Value) -> Result<Value> {
        let resp = api(ctx, &self.oauth, |c, t| {
            c.request(method.clone(), url).bearer_auth(t).header("Prefer", PREFER).json(body)
        })
        .await?;
        let text = resp.text().await?;
        Ok(if text.is_empty() { Value::Null } else { serde_json::from_str(&text)? })
    }

    async fn ensure_folders(&self, ctx: &Ctx, st: &mut State) -> Result<()> {
        if st.folders.contains_key("inbox") {
            return Ok(());
        }
        for wk in KNOWN {
            match self.get_json(ctx, &format!("{API}/mailFolders/{wk}?$select=id")).await {
                Ok(v) => {
                    if let Some(id) = v["id"].as_str() {
                        st.folders.insert(wk.to_string(), id.to_string());
                    }
                }
                // Not every mailbox has an Archive folder.
                Err(e) if http_status(&e) == Some(404) && wk == "archive" => {}
                // Some Outlook.com mailboxes aren't reachable through Graph at all
                // and answer 404 for every folder.
                Err(e) if e.to_string().contains("MailboxNotEnabledForRESTAPI") => {
                    return Err(e.context(
                        "This mailbox can't be reached through Microsoft Graph. \
                         Remove it and add it again as IMAP with Microsoft sign-in.",
                    ));
                }
                Err(e) => return Err(e.context(format!("looking up the {wk} folder"))),
            }
        }
        Ok(())
    }

    pub async fn sync(&mut self, ctx: &Ctx) -> Result<()> {
        let mut st: State = ctx.db.sync_state(&ctx.account.id)?;
        self.ensure_folders(ctx, &mut st).await?;
        for wk in SYNCED {
            let Some(folder_id) = st.folders.get(wk).cloned() else { continue };
            match self.sync_folder(ctx, &mut st, wk, &folder_id).await {
                Ok(()) => {}
                // Delta token expired: restart this folder from scratch.
                Err(e) if matches!(http_status(&e), Some(410) | Some(400)) && st.delta.contains_key(wk) => {
                    st.delta.remove(wk);
                    self.sync_folder(ctx, &mut st, wk, &folder_id).await?;
                }
                Err(e) => return Err(e),
            }
            ctx.db.set_sync_state(&ctx.account.id, &st)?;
        }
        Ok(())
    }

    async fn sync_folder(&self, ctx: &Ctx, st: &mut State, wk: &str, folder_id: &str) -> Result<()> {
        let mut url = match st.delta.get(wk) {
            Some(link) => link.clone(),
            None => {
                let since = (chrono::Utc::now() - chrono::Duration::days(ctx.sync_days()))
                    .format("%Y-%m-%dT%H:%M:%SZ");
                format!(
                    "{API}/mailFolders/{wk}/messages/delta?$select={SELECT}&$filter=receivedDateTime+ge+{since}"
                )
            }
        };
        let mut new_items: Vec<Item> = Vec::new();
        let mut changed = false;
        loop {
            let v = self.get_json(ctx, &url).await?;
            for raw in v["value"].as_array().into_iter().flatten() {
                let Ok(item) = serde_json::from_value::<Item>(raw.clone()) else { continue };
                if item.removed.is_some() {
                    // Moved out of this folder or deleted. Only drop our copy if
                    // we still think it lives here (it may have been re-added by
                    // another folder's delta already).
                    let refs = ctx.db.msg_refs(&[crate::db::local_id(&ctx.account.id, &item.id)])?;
                    if refs.first().is_some_and(|r| r.folder.as_deref() == Some(folder_id)) {
                        ctx.db.delete_message(&ctx.account.id, &item.id)?;
                        changed = true;
                    }
                    continue;
                }
                let flags = st.flags(&item);
                if ctx.db.sync_flags(
                    &ctx.account.id,
                    &item.id,
                    flags,
                    None,
                    item.parent_folder_id.as_deref(),
                )? {
                    changed = true;
                } else {
                    new_items.push(item);
                }
            }
            if let Some(next) = v["@odata.nextLink"].as_str() {
                url = next.to_string();
                continue;
            }
            if let Some(delta) = v["@odata.deltaLink"].as_str() {
                st.delta.insert(wk.to_string(), delta.to_string());
            }
            break;
        }
        if changed {
            ctx.emit_changed();
        }

        let total = new_items.len() as u32;
        let mut done = 0;
        let st_ref = &*st;
        let mut results = stream::iter(new_items)
            .map(|item| async move {
                let raw = self.fetch_raw(ctx, &item.id).await?;
                let date = item
                    .received_date_time
                    .as_deref()
                    .and_then(|d| chrono::DateTime::parse_from_rfc3339(d).ok())
                    .map(|d| d.timestamp());
                let hint = match item.inference_classification.as_deref() {
                    Some("other") => Some("notification"),
                    _ => None,
                };
                ctx.ingest(
                    &item.id,
                    item.conversation_id.as_deref().or(Some(&item.id)),
                    &raw,
                    st_ref.flags(&item),
                    Vec::new(),
                    item.parent_folder_id.clone(),
                    None,
                    date,
                    hint,
                )
            })
            .buffer_unordered(6);
        while let Some(r) = results.next().await {
            if let Err(e) = r {
                tracing::warn!("graph: failed to fetch message: {e:#}");
                if e.is::<crate::error::AuthExpired>() {
                    return Err(e);
                }
            }
            done += 1;
            if done % 20 == 0 || done == total {
                ctx.emit_progress(done, total);
                ctx.emit_changed();
            }
        }
        Ok(())
    }

    pub async fn apply(&mut self, ctx: &Ctx, action: ThreadAction, refs: &[MsgRef]) -> Result<()> {
        let patch = match action {
            ThreadAction::Read => Some(json!({ "isRead": true })),
            ThreadAction::Unread => Some(json!({ "isRead": false })),
            ThreadAction::Star => Some(json!({ "flag": { "flagStatus": "flagged" } })),
            ThreadAction::Unstar => Some(json!({ "flag": { "flagStatus": "notFlagged" } })),
            _ => None,
        };
        let dest = match action {
            ThreadAction::Archive => Some("archive"),
            ThreadAction::Trash => Some("deleteditems"),
            ThreadAction::Inbox => Some("inbox"),
            ThreadAction::Spam => Some("junkemail"),
            _ => None,
        };
        let this: &Graph = self;
        let results: Vec<Result<()>> = stream::iter(refs.iter().cloned())
            .map(|r: MsgRef| {
                let patch = patch.clone();
                async move {
                    let url = format!("{API}/messages/{}", r.remote_id);
                    if let Some(p) = patch {
                        this.send_json(ctx, reqwest::Method::PATCH, &url, &p).await?;
                    } else if let Some(d) = dest {
                        let moved = this
                            .send_json(ctx, reqwest::Method::POST, &format!("{url}/move"), &json!({ "destinationId": d }))
                            .await?;
                        if let Some(folder) = moved["parentFolderId"].as_str() {
                            let mut f = r.flags;
                            f.inbox = d == "inbox";
                            f.trash = d == "deleteditems";
                            f.spam = d == "junkemail";
                            ctx.db.update_flags(&ctx.account.id, &r.remote_id, f, None, Some(folder))?;
                        }
                    }
                    Ok(())
                }
            })
            .buffer_unordered(4)
            .collect()
            .await;
        results.into_iter().collect()
    }

    pub async fn send(&mut self, ctx: &Ctx, draft: &Draft) -> Result<()> {
        let built = mime::build(draft, &ctx.account, true)?;
        let body = STANDARD.encode(&built.raw);
        api(ctx, &self.oauth, |c, t| {
            c.post(format!("{API}/sendMail"))
                .bearer_auth(t)
                .header("Content-Type", "text/plain")
                .body(body.clone())
        })
        .await?;
        Ok(())
    }

    pub async fn fetch_raw(&self, ctx: &Ctx, remote_id: &str) -> Result<Vec<u8>> {
        let url = format!("{API}/messages/{remote_id}/$value");
        let resp = api(ctx, &self.oauth, |c, t| c.get(&url).bearer_auth(t).header("Prefer", PREFER)).await?;
        Ok(resp.bytes().await?.to_vec())
    }
}
