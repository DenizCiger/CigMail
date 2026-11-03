//! AI features backed by a ChatGPT subscription through the Codex backend
//! (same flow as openai-oauth / the Codex CLI). Unofficial: OpenAI may change
//! or block it at any time, so everything goes through `Ai::stream` and can be
//! swapped for another provider later.

use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use futures::StreamExt;
use parking_lot::Mutex;
use serde::Serialize;
use serde_json::{Value, json};

use crate::{
    config,
    db::Db,
    error::AuthExpired,
    models::{Account, ThreadDetail},
    oauth::{self, Tokens},
    secrets::Secrets,
};

const KEY: &str = "ai:chatgpt";
const BASE: &str = "https://chatgpt.com/backend-api/codex";
const FALLBACK_CODEX_VERSION: &str = "0.144.1";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiStatus {
    pub connected: bool,
    pub email: Option<String>,
    pub plan: Option<String>,
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub slug: String,
    pub name: String,
    #[serde(skip)]
    pub lite: bool,
}

pub struct Ai {
    db: Db,
    secrets: Secrets,
    http: reqwest::Client,
    models: Mutex<Option<(Instant, Vec<ModelInfo>)>>,
    version: Mutex<Option<String>>,
}

struct Auth {
    token: String,
    account_id: String,
    fedramp: bool,
}

fn auth_claims(t: &Tokens) -> Option<Value> {
    t.id_token
        .as_deref()
        .and_then(oauth::decode_jwt_claims)
        .or_else(|| oauth::decode_jwt_claims(&t.access_token))
}

fn account_id(t: &Tokens) -> Option<String> {
    for tok in [t.id_token.as_deref(), Some(t.access_token.as_str())].into_iter().flatten() {
        let Some(c) = oauth::decode_jwt_claims(tok) else { continue };
        if let Some(id) = c["https://api.openai.com/auth"]["chatgpt_account_id"].as_str() {
            return Some(id.into());
        }
        if let Some(id) = c["chatgpt_account_id"].as_str() {
            return Some(id.into());
        }
        if let Some(id) = c["organizations"][0]["id"].as_str() {
            return Some(id.into());
        }
    }
    None
}

impl Ai {
    pub fn new(db: Db, secrets: Secrets, http: reqwest::Client) -> Self {
        Ai { db, secrets, http, models: Mutex::new(None), version: Mutex::new(None) }
    }

    pub fn status(&self) -> Result<AiStatus> {
        let tokens: Option<Tokens> = self.secrets.get_json(KEY)?;
        let Some(t) = tokens else {
            return Ok(AiStatus { connected: false, email: None, plan: None, model: None });
        };
        let claims = auth_claims(&t).unwrap_or(Value::Null);
        Ok(AiStatus {
            connected: true,
            email: claims["email"].as_str().map(String::from),
            plan: claims["https://api.openai.com/auth"]["chatgpt_plan_type"].as_str().map(String::from),
            model: self.db.get_setting("ai_model")?,
        })
    }

    pub async fn login(&self, open: impl FnOnce(&str) -> Result<()>) -> Result<AiStatus> {
        let tokens = oauth::authorize(&config::codex(), open).await?;
        if account_id(&tokens).is_none() {
            bail!("this OpenAI account has no ChatGPT workspace");
        }
        self.secrets.set_json(KEY, &tokens)?;
        *self.models.lock() = None;
        self.status()
    }

    pub fn logout(&self) -> Result<()> {
        *self.models.lock() = None;
        self.secrets.delete(KEY)
    }

    async fn auth(&self, force_refresh: bool) -> Result<Auth> {
        let mut t: Tokens = self.secrets.get_json(KEY)?.ok_or_else(|| anyhow!("Connect your ChatGPT account in Settings → AI first"))?;
        if !t.is_fresh() || force_refresh {
            let rt = t.refresh_token.clone().ok_or(AuthExpired)?;
            t = oauth::refresh(&self.http, &config::codex(), &rt).await?;
            self.secrets.set_json(KEY, &t)?;
        }
        let fedramp = auth_claims(&t)
            .map(|c| c["https://api.openai.com/auth"]["chatgpt_account_is_fedramp"].as_bool() == Some(true))
            .unwrap_or(false);
        Ok(Auth {
            account_id: account_id(&t).context("missing ChatGPT account id")?,
            token: t.access_token,
            fedramp,
        })
    }

    fn authed(&self, req: reqwest::RequestBuilder, a: &Auth) -> reqwest::RequestBuilder {
        let req = req
            .bearer_auth(&a.token)
            .header("chatgpt-account-id", &a.account_id)
            .header("originator", "codex_cli_rs");
        if a.fedramp { req.header("X-OpenAI-Fedramp", "true") } else { req }
    }

    async fn codex_version(&self) -> String {
        if let Some(v) = self.version.lock().clone() {
            return v;
        }
        let fetched = async {
            let v: Value = self
                .http
                .get("https://registry.npmjs.org/@openai/codex/latest")
                .header("accept", "application/json")
                .timeout(Duration::from_secs(5))
                .send()
                .await?
                .json()
                .await?;
            v["version"].as_str().map(String::from).context("no version")
        }
        .await;
        let v = fetched
            .ok()
            .and_then(|v| {
                let core: String = v.split(['-', '+']).next().unwrap_or("").to_string();
                (core.split('.').count() == 3).then_some(core)
            })
            .unwrap_or_else(|| FALLBACK_CODEX_VERSION.into());
        *self.version.lock() = Some(v.clone());
        v
    }

    pub async fn models(&self) -> Result<Vec<ModelInfo>> {
        if let Some((at, m)) = self.models.lock().as_ref() {
            if at.elapsed() < Duration::from_secs(300) {
                return Ok(m.clone());
            }
        }
        let version = self.codex_version().await;
        let mut force = false;
        let v: Value = loop {
            let a = self.auth(force).await?;
            let resp = self
                .authed(self.http.get(format!("{BASE}/models?client_version={version}")), &a)
                .send()
                .await?;
            if resp.status().as_u16() == 401 && !force {
                force = true;
                continue;
            }
            if !resp.status().is_success() {
                bail!("listing models failed: {} {}", resp.status(), resp.text().await.unwrap_or_default());
            }
            break resp.json().await?;
        };
        let models: Vec<ModelInfo> = v["models"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|m| m["supported_in_api"].as_bool() != Some(false))
            .filter(|m| m["visibility"].as_str().is_none_or(|v| v == "list"))
            .filter_map(|m| {
                let slug = m["slug"].as_str()?.to_string();
                Some(ModelInfo {
                    name: m["display_name"].as_str().map(String::from).unwrap_or_else(|| slug.clone()),
                    lite: m["use_responses_lite"].as_bool().unwrap_or(false),
                    slug,
                })
            })
            .collect();
        if models.is_empty() {
            bail!("your ChatGPT plan has no models available for Codex");
        }
        *self.models.lock() = Some((Instant::now(), models.clone()));
        Ok(models)
    }

    async fn pick_model(&self) -> Result<ModelInfo> {
        let models = self.models().await?;
        if let Some(chosen) = self.db.get_setting("ai_model")? {
            if let Some(m) = models.iter().find(|m| m.slug == chosen) {
                return Ok(m.clone());
            }
        }
        // Mail tasks are short; prefer a fast model by default.
        Ok(models
            .iter()
            .find(|m| m.slug.contains("mini"))
            .or_else(|| models.first())
            .cloned()
            .unwrap())
    }

    /// Streams a single-turn completion, calling `on_delta` for each text chunk.
    pub async fn stream(&self, system: &str, user: &str, mut on_delta: impl FnMut(&str)) -> Result<String> {
        let model = self.pick_model().await?;
        let effort = self.db.get_setting("ai_effort")?.unwrap_or_else(|| "low".into());
        let user_msg = json!({ "type": "message", "role": "user", "content": [{ "type": "input_text", "text": user }] });
        let mut body = json!({
            "model": model.slug,
            "instructions": system,
            "input": [user_msg],
            "stream": true,
            "store": false,
            "include": ["reasoning.encrypted_content"],
            "reasoning": { "effort": effort },
        });
        if model.lite {
            body["instructions"] = json!("");
            body["input"] = json!([
                { "type": "message", "role": "developer", "content": [{ "type": "input_text", "text": system }] },
                user_msg,
            ]);
            body["reasoning"]["context"] = json!("all_turns");
            body["parallel_tool_calls"] = json!(false);
        }

        let mut force = false;
        let resp = loop {
            let a = self.auth(force).await?;
            let mut req = self
                .authed(self.http.post(format!("{BASE}/responses")), &a)
                .header("accept", "text/event-stream")
                .header("session_id", uuid::Uuid::new_v4().to_string())
                .json(&body);
            if model.lite {
                req = req.header("x-openai-internal-codex-responses-lite", "true");
            }
            let resp = req.send().await?;
            if resp.status().as_u16() == 401 && !force {
                force = true;
                continue;
            }
            if !resp.status().is_success() {
                let status = resp.status();
                let text = resp.text().await.unwrap_or_default();
                let detail = serde_json::from_str::<Value>(&text)
                    .ok()
                    .and_then(|v| v["detail"].as_str().or(v["error"]["message"].as_str()).map(String::from))
                    .unwrap_or(text);
                if status.as_u16() == 429 {
                    bail!("ChatGPT usage limit reached: {detail}");
                }
                bail!("AI request failed ({status}): {detail}");
            }
            break resp;
        };

        let mut out = String::new();
        let mut buf = String::new();
        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            buf.push_str(&String::from_utf8_lossy(&chunk?));
            while let Some(pos) = buf.find("\n\n") {
                let event: String = buf.drain(..pos + 2).collect();
                let data: String = event
                    .lines()
                    .filter_map(|l| l.strip_prefix("data:"))
                    .map(str::trim_start)
                    .collect::<Vec<_>>()
                    .join("\n");
                if data.is_empty() || data == "[DONE]" {
                    continue;
                }
                let Ok(v) = serde_json::from_str::<Value>(&data) else { continue };
                match v["type"].as_str().unwrap_or("") {
                    "response.output_text.delta" => {
                        if let Some(d) = v["delta"].as_str() {
                            out.push_str(d);
                            on_delta(d);
                        }
                    }
                    "response.failed" | "error" | "response.error" => {
                        let msg = v["response"]["error"]["message"]
                            .as_str()
                            .or(v["error"]["message"].as_str())
                            .or(v["message"].as_str())
                            .unwrap_or("unknown error");
                        bail!("AI error: {msg}");
                    }
                    "response.completed" => return Ok(out),
                    _ => {}
                }
            }
        }
        Ok(out)
    }
}

// ------------------------------------------------------------------ prompts

fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut t: String = s.chars().take(max).collect();
    t.push_str(" […]");
    t
}

/// Strips quoted history so each message contributes only what's new.
fn fresh_part(text: &str) -> String {
    let mut out = Vec::new();
    for line in text.lines() {
        let l = line.trim_start();
        if l.starts_with('>') {
            continue;
        }
        if (l.starts_with("On ") && l.trim_end().ends_with("wrote:"))
            || l.starts_with("-----Original Message-----")
            || l.starts_with("________________________________")
            || (l.starts_with("Am ") && l.trim_end().ends_with("schrieb:"))
        {
            break;
        }
        out.push(line);
    }
    out.join("\n").trim().to_string()
}

pub fn transcript(t: &ThreadDetail, budget: usize) -> String {
    let per = (budget / t.messages.len().max(1)).clamp(800, 6000);
    let mut s = format!("Subject: {}\n", t.thread.subject);
    for m in &t.messages {
        let when = chrono::DateTime::from_timestamp(m.date, 0)
            .map(|d| d.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M").to_string())
            .unwrap_or_default();
        let to = m.to.iter().map(|a| a.display()).collect::<Vec<_>>().join(", ");
        s.push_str(&format!(
            "\n--- From: {} | To: {} | {}\n{}\n",
            m.from.display(),
            to,
            when,
            clip(&fresh_part(m.body_text.as_deref().unwrap_or(&m.snippet)), per)
        ));
    }
    clip(&s, budget)
}

pub fn summary_prompt(account: &Account) -> String {
    format!(
        "You summarize email threads inside an email client for {} <{}>.\n\
         Reply in the language the thread is written in. Output format, nothing else:\n\
         Line 1: a single-sentence TL;DR of at most 25 words, no prefix.\n\
         Then up to 4 bullet points starting with \"- \" for key facts, dates, amounts or decisions (skip if the thread is trivial).\n\
         If the user has to do something, end with a line starting with \"Action: \".\n\
         No preamble, no markdown headings.",
        account.name, account.email
    )
}

pub fn compose_prompt(account: &Account, samples: &[String]) -> String {
    let mut s = format!(
        "You write emails on behalf of {} <{}>.\n\
         Output only the email body as plain text: no subject line, no markdown, no placeholders like [Name] unless information is truly missing.\n\
         Start with an appropriate greeting and end with a short sign-off using the sender's first name.\n\
         Be concise and natural. Use the language of the conversation unless the instruction says otherwise.",
        account.name, account.email
    );
    let samples: Vec<String> = samples
        .iter()
        .map(|t| clip(&fresh_part(t), 600))
        .filter(|t| t.len() > 40)
        .take(4)
        .collect();
    if !samples.is_empty() {
        s.push_str("\n\nMatch the tone and style of these emails the sender wrote before:\n");
        for (i, t) in samples.iter().enumerate() {
            s.push_str(&format!("\n<example {}>\n{}\n</example>\n", i + 1, t));
        }
    }
    s
}

pub const REWRITE_PROMPT: &str = "You edit text inside an email composer. Rewrite the given text following the instruction. \
Keep the original language and meaning unless told otherwise. Output only the rewritten text as plain text, no quotes, no commentary.";
