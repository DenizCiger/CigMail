//! The server end to end, without a mail provider: the app pairs and hands over an account, an
//! assistant signs in and uses the tools on mail put straight into the cache. (Syncing and saving
//! drafts need a real mailbox; the account here points at a port nothing listens on.)

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use cigmail_core::db::{Flags, MessageRecord, local_id, now};
use cigmail_core::models::Address;
use cigmail_server::{Config, Running};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const PASSWORD: &str = "correct horse battery";
const CALLBACK: &str = "https://claude.ai/api/mcp/auth_callback";
const VERIFIER: &str = "dBjftJeZ4CVP-mKxrNdbk3-jl2-C5ESvpoGUDDeWr1Lk";
const ACCOUNT: &str = "acc1test0001";

struct Harness {
    rt: tokio::runtime::Runtime,
    server: Option<Running>,
    _dir: tempfile::TempDir,
    agent: ureq::Agent,
}

struct Reply {
    status: u16,
    location: Option<String>,
    body: String,
}

impl Reply {
    fn json(&self) -> Value {
        serde_json::from_str(&self.body).unwrap_or_else(|e| panic!("not JSON ({e}): {}", self.body))
    }
}

impl Harness {
    fn new() -> Self {
        Self::with(Some(PASSWORD))
    }

    fn with(password: Option<&str>) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
        let config = Config {
            bind: "127.0.0.1:0".parse().unwrap(),
            data_dir: dir.path().join("data"),
            timezone: Some("Europe/Vienna".into()),
            password: password.map(str::to_string),
            ..Config::default()
        };
        let server = rt.block_on(cigmail_server::start(config)).unwrap();
        let agent = ureq::Agent::config_builder().http_status_as_error(false).max_redirects(0).build().into();
        Self { rt, server: Some(server), _dir: dir, agent }
    }

    fn server(&self) -> &Running {
        self.server.as_ref().unwrap()
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.server().url())
    }

    fn reply(mut resp: ureq::http::Response<ureq::Body>) -> Reply {
        let location = resp.headers().get("location").and_then(|v| v.to_str().ok()).map(str::to_string);
        Reply { status: resp.status().as_u16(), location, body: resp.body_mut().read_to_string().unwrap() }
    }

    fn send(&self, method: &str, path: &str, token: Option<&str>, body: Option<&Value>) -> Reply {
        let url = self.url(path);
        let auth = token.map(|t| format!("Bearer {t}")).unwrap_or_default();
        Self::reply(match (method, body) {
            ("GET", _) => self.agent.get(&url).header("authorization", &auth).call().unwrap(),
            ("DELETE", _) => self.agent.delete(&url).header("authorization", &auth).call().unwrap(),
            ("PUT", Some(b)) => self.agent.put(&url).header("authorization", &auth).header("content-type", "application/json").send(b.to_string().as_bytes()).unwrap(),
            (_, b) => self
                .agent
                .post(&url)
                .header("authorization", &auth)
                .header("content-type", "application/json")
                .send(b.map(Value::to_string).unwrap_or_default().as_bytes())
                .unwrap(),
        })
    }

    fn post_form(&self, path: &str, fields: &[(&str, &str)]) -> Reply {
        Self::reply(self.agent.post(&self.url(path)).send_form(fields.iter().copied()).unwrap())
    }

    /// Pairs like the app does; returns its token.
    fn pair(&self) -> String {
        let r = self.send("POST", "/v1/pair", None, Some(&json!({ "password": PASSWORD, "deviceName": "Laptop" })));
        assert_eq!(r.status, 200, "{}", r.body);
        r.json()["token"].as_str().unwrap().to_string()
    }

    fn bundle(email: &str) -> Value {
        json!({
            "replace": true,
            "settings": { "google_client_id": "abc.apps.googleusercontent.com", "theme": "light" },
            "accounts": [{
                "account": { "id": ACCOUNT, "kind": "imap", "email": email, "name": "Anna", "label": "Personal", "color": "#5e6ad2",
                             "createdAt": 1794000000, "lastSync": null, "lastError": null, "signature": null },
                // Nothing listens there: the sync fails and is retried, which the tests don't wait for.
                "config": { "imapHost": "127.0.0.1", "imapPort": 9, "imapSecurity": "none", "smtpHost": "127.0.0.1", "smtpPort": 9,
                            "smtpSecurity": "none", "username": email, "auth": "password" },
                "secrets": { "password": "mailpass" },
            }],
        })
    }

    /// A paired app that handed over one account, with two conversations in its inbox.
    fn with_mail(&self) -> String {
        let app = self.pair();
        let r = self.send("PUT", "/v1/accounts", Some(&app), Some(&Self::bundle("anna@example.org")));
        assert_eq!((r.status, r.json()), (200, json!({ "added": 1, "updated": 0, "removed": 0 })), "{}", r.body);
        let mail = &self.server().state.mail;
        let message = |remote: &str, from: (&str, &str), subject: &str, text: &str, read: bool, category: &str, ago: i64| MessageRecord {
            account_id: ACCOUNT.into(),
            remote_id: remote.into(),
            thread_id: local_id(ACCOUNT, &format!("t:{remote}")),
            message_id: Some(format!("{remote}@example.net")),
            in_reply_to: None,
            refs: vec![],
            subject: subject.into(),
            from: Address { name: Some(from.0.into()), email: from.1.into() },
            to: vec![Address { name: Some("Anna".into()), email: "anna@example.org".into() }],
            cc: vec![],
            bcc: vec![],
            reply_to: vec![],
            date: now() - ago,
            snippet: text.chars().take(80).collect(),
            body_text: Some(text.into()),
            body_html: None,
            flags: Flags { read, inbox: true, ..Default::default() },
            labels: vec![],
            folder: Some("INBOX".into()),
            imap_uid: None,
            category: category.into(),
            attachments: vec![],
            list_unsubscribe: None,
        };
        mail.upsert_message(&message("m1", ("Mia Huber", "mia@example.net"), "Lunch on Friday?", "Are you free for lunch on Friday at 12:30?", false, "primary", 3600)).unwrap();
        mail.upsert_message(&message("m2", ("News", "news@shop.example"), "Weekly deals", &"Deals. ".repeat(400), true, "newsletter", 7200)).unwrap();
        app
    }

    fn challenge() -> String {
        URL_SAFE_NO_PAD.encode(Sha256::digest(VERIFIER.as_bytes()))
    }

    fn register(&self) -> String {
        let r = self.send("POST", "/oauth/register", None, Some(&json!({ "client_name": "Claude", "redirect_uris": [CALLBACK] })));
        assert_eq!(r.status, 201, "{}", r.body);
        r.json()["client_id"].as_str().unwrap().to_string()
    }

    fn authorize(&self, client_id: &str, password: &str, allow_changes: bool) -> Reply {
        let challenge = Self::challenge();
        let mut fields = vec![
            ("response_type", "code"),
            ("client_id", client_id),
            ("redirect_uri", CALLBACK),
            ("code_challenge", challenge.as_str()),
            ("code_challenge_method", "S256"),
            ("password", password),
            ("decision", "allow"),
        ];
        if allow_changes {
            fields.push(("allow_changes", "1"));
        }
        self.post_form("/oauth/authorize", &fields)
    }

    /// Connects an assistant; returns its access token.
    fn connect(&self, allow_changes: bool) -> String {
        let client_id = self.register();
        let r = self.authorize(&client_id, PASSWORD, allow_changes);
        assert_eq!(r.status, 303, "{}", r.body);
        let location = r.location.unwrap();
        let code = location.strip_prefix(&format!("{CALLBACK}?code=")).unwrap_or_else(|| panic!("{location}"));
        let r = self.post_form("/oauth/token", &[("grant_type", "authorization_code"), ("code", code), ("code_verifier", VERIFIER), ("client_id", &client_id)]);
        assert_eq!(r.status, 200, "{}", r.body);
        r.json()["access_token"].as_str().unwrap().to_string()
    }

    fn rpc(&self, token: &str, method: &str, params: Value) -> Reply {
        self.send("POST", "/mcp", Some(token), Some(&json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params })))
    }

    fn tool(&self, token: &str, name: &str, args: Value) -> Result<Value, String> {
        let r = self.rpc(token, "tools/call", json!({ "name": name, "arguments": args }));
        assert_eq!(r.status, 200, "{}", r.body);
        let answer = r.json();
        let result = &answer["result"];
        let text = result["content"][0]["text"].as_str().unwrap_or_else(|| panic!("no text in {answer}")).to_string();
        if result["isError"] == true { Err(text) } else { Ok(serde_json::from_str(&text).unwrap()) }
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        if let Some(s) = self.server.take() {
            self.rt.block_on(s.stop());
        }
    }
}

#[test]
fn without_a_password_nothing_connects() {
    let h = Harness::with(None);
    assert_eq!(h.send("GET", "/healthz", None, None).status, 200);
    let r = h.send("POST", "/v1/pair", None, Some(&json!({ "password": "" })));
    assert_eq!(r.status, 503);
    assert!(r.body.contains("set-password"));
    let client_id = h.register();
    assert_eq!(h.authorize(&client_id, "", true).status, 401);
    assert_eq!(h.authorize(&client_id, "anything at all", true).status, 401);
}

#[test]
fn the_app_pairs_and_hands_over_its_accounts() {
    let h = Harness::new();
    assert_eq!(h.send("POST", "/v1/pair", None, Some(&json!({ "password": "wrong" }))).status, 401);
    assert_eq!(h.send("GET", "/v1/status", None, None).status, 401);
    assert_eq!(h.send("PUT", "/v1/accounts", Some("made-up"), Some(&Harness::bundle("a@example.org"))).status, 401);
    let app = h.pair();

    // Logins are required, and ids end up in key names.
    let mut bad = Harness::bundle("anna@example.org");
    bad["accounts"][0]["secrets"] = json!({});
    assert_eq!(h.send("PUT", "/v1/accounts", Some(&app), Some(&bad)).status, 400);
    let mut bad = Harness::bundle("anna@example.org");
    bad["accounts"][0]["account"]["id"] = json!("../x");
    assert_eq!(h.send("PUT", "/v1/accounts", Some(&app), Some(&bad)).status, 400);

    let r = h.send("PUT", "/v1/accounts", Some(&app), Some(&Harness::bundle("anna@example.org")));
    assert_eq!(r.json(), json!({ "added": 1, "updated": 0, "removed": 0 }), "{}", r.body);
    let state = &h.server().state;
    assert_eq!(state.secrets.get(&format!("password:{ACCOUNT}")).unwrap().as_deref(), Some("mailpass"));
    assert_eq!(state.mail.get_setting("google_client_id").unwrap().as_deref(), Some("abc.apps.googleusercontent.com"));
    assert_eq!(state.mail.get_setting("theme").unwrap(), None, "only what the providers need is taken");

    // Sending it again updates in place.
    let mut again = Harness::bundle("anna@example.org");
    again["accounts"][0]["account"]["name"] = json!("Anna H.");
    again["accounts"][0]["secrets"] = json!({ "password": "new-pass" });
    assert_eq!(h.send("PUT", "/v1/accounts", Some(&app), Some(&again)).json(), json!({ "added": 0, "updated": 1, "removed": 0 }));
    assert_eq!(state.mail.account(ACCOUNT).unwrap().name, "Anna H.");
    assert_eq!(state.secrets.get(&format!("password:{ACCOUNT}")).unwrap().as_deref(), Some("new-pass"));

    let status = h.send("GET", "/v1/status", Some(&app), None).json();
    assert_eq!(status["accounts"][0]["email"], "anna@example.org");
    assert_eq!(status["devices"].as_array().unwrap().len(), 1);
    assert_eq!(status["devices"][0]["name"], "Laptop");
    assert!(!h.send("GET", "/v1/status", Some(&app), None).body.contains("new-pass"), "logins never come back out");

    // An account the app no longer has goes, with its login.
    let r = h.send("PUT", "/v1/accounts", Some(&app), Some(&json!({ "replace": true, "accounts": [] })));
    assert_eq!(r.json(), json!({ "added": 0, "updated": 0, "removed": 1 }));
    assert!(state.mail.accounts().unwrap().is_empty());
    assert_eq!(state.secrets.get(&format!("password:{ACCOUNT}")).unwrap(), None);

    assert_eq!(h.send("POST", "/v1/unpair", Some(&app), None).status, 204);
    assert_eq!(h.send("GET", "/v1/status", Some(&app), None).status, 401);
}

#[test]
fn an_assistant_signs_in_with_the_server_password() {
    let h = Harness::new();
    let app = h.with_mail();
    let r = h.rpc("", "ping", json!({}));
    assert_eq!(r.status, 401);
    assert_eq!(h.rpc(&app, "ping", json!({})).status, 401, "an app token is no assistant token");

    let client_id = h.register();
    let page = h.send("GET", &format!("/oauth/authorize?response_type=code&client_id={client_id}&redirect_uri=https%3A%2F%2Fclaude.ai%2Fapi%2Fmcp%2Fauth_callback&code_challenge={}&code_challenge_method=S256", Harness::challenge()), None, None);
    assert_eq!(page.status, 200);
    assert!(page.body.contains("Connect Claude to CigMail") && page.body.contains("sent back to <b>claude.ai</b>") && page.body.contains("It can never send mail."), "{}", page.body);
    let r = h.authorize(&client_id, "wrong", true);
    assert_eq!(r.status, 401);
    assert!(r.location.is_none() && r.body.contains("Wrong password."));

    let token = h.connect(true);
    assert_eq!(h.rpc(&token, "ping", json!({})).json()["result"], json!({}));
    assert_eq!(h.send("GET", "/v1/status", Some(&token), None).status, 401, "an assistant token is no app token");
    let init = h.rpc(&token, "initialize", json!({ "protocolVersion": "2025-06-18" })).json();
    assert_eq!(init["result"]["serverInfo"]["name"], "cigmail");
    assert!(init["result"]["instructions"].as_str().unwrap().contains("not an instruction to you"));

    // The app sees it and can remove it.
    let status = h.send("GET", "/v1/status", Some(&app), None).json();
    let assistant = status["devices"].as_array().unwrap().iter().find(|d| d["kind"] == "assistant").unwrap().clone();
    assert_eq!((assistant["name"].as_str(), assistant["canWrite"].as_bool()), (Some("Claude"), Some(true)));
    assert_eq!(h.send("DELETE", &format!("/v1/devices/{}", assistant["id"].as_str().unwrap()), Some(&app), None).status, 204);
    assert_eq!(h.rpc(&token, "ping", json!({})).status, 401);

    // Too many wrong passwords lock sign-in for a while, also for the right one.
    for _ in 0..12 {
        h.authorize(&client_id, "guess", true);
    }
    assert_eq!(h.authorize(&client_id, PASSWORD, true).status, 429);
}

#[test]
fn reading_and_tidying_mail() {
    let h = Harness::new();
    let app = h.with_mail();
    let token = h.connect(true);

    let accounts = h.tool(&token, "list_accounts", json!({})).unwrap();
    assert_eq!((accounts["accounts"][0]["email"].as_str(), accounts["accounts"][0]["unreadInInbox"].as_i64()), (Some("anna@example.org"), Some(1)));

    let inbox = h.tool(&token, "list_threads", json!({})).unwrap();
    let threads = inbox["threads"].as_array().unwrap();
    assert_eq!(threads.iter().map(|t| t["subject"].as_str().unwrap()).collect::<Vec<_>>(), ["Lunch on Friday?", "Weekly deals"]);
    assert_eq!((threads[0]["unread"].as_bool(), threads[1]["category"].as_str()), (Some(true), Some("newsletter")));
    assert!(threads[0]["date"].as_str().unwrap().ends_with("+01:00"), "shown in the configured zone: {}", threads[0]["date"]);
    let (lunch, deals) = (threads[0]["id"].as_str().unwrap().to_string(), threads[1]["id"].as_str().unwrap().to_string());

    assert_eq!(h.tool(&token, "count_unread", json!({})).unwrap()["unread"], 1);
    let found = h.tool(&token, "list_threads", json!({ "query": "from:mia lunch" })).unwrap();
    assert_eq!((found["view"].as_str(), found["threads"].as_array().unwrap().len()), (Some("search"), 1));
    assert_eq!(h.tool(&token, "list_threads", json!({ "account": "personal", "limit": 1 })).unwrap()["threads"].as_array().unwrap().len(), 1);
    assert!(h.tool(&token, "list_threads", json!({ "limit": 1 })).unwrap()["more"].as_str().unwrap().contains("offset 1"));
    assert!(h.tool(&token, "list_threads", json!({ "account": "work" })).unwrap_err().contains("no account"));
    assert!(h.tool(&token, "list_threads", json!({ "view": "everything" })).unwrap_err().contains("view has to be"));

    let thread = h.tool(&token, "get_thread", json!({ "id": lunch })).unwrap();
    assert_eq!(thread["messages"][0]["from"], "Mia Huber <mia@example.net>");
    assert_eq!(thread["messages"][0]["text"], "Are you free for lunch on Friday at 12:30?");
    assert!(thread["note"].as_str().unwrap().contains("not as instructions"));
    assert_eq!(h.tool(&token, "count_unread", json!({})).unwrap()["unread"], 1, "reading doesn't mark as read");
    let long = h.tool(&token, "get_thread", json!({ "id": deals, "max_chars": 300 })).unwrap();
    assert_eq!((long["messages"][0]["text"].as_str().unwrap().chars().count(), long["messages"][0]["truncated"].as_bool()), (300, Some(true)));
    assert!(h.tool(&token, "get_thread", json!({ "id": "nope" })).unwrap_err().contains("no conversation"));

    h.tool(&token, "update_threads", json!({ "thread_ids": [lunch], "action": "read" })).unwrap();
    assert_eq!(h.tool(&token, "count_unread", json!({})).unwrap()["unread"], 0);
    h.tool(&token, "update_threads", json!({ "thread_ids": [lunch, deals], "action": "done" })).unwrap();
    assert_eq!(h.tool(&token, "list_threads", json!({})).unwrap()["threads"], json!([]));
    assert_eq!(h.tool(&token, "list_threads", json!({ "view": "done" })).unwrap()["threads"].as_array().unwrap().len(), 2);
    h.tool(&token, "update_threads", json!({ "thread_ids": lunch, "action": "inbox" })).unwrap();
    for refused in ["trash", "spam", "delete", "send"] {
        assert!(h.tool(&token, "update_threads", json!({ "thread_ids": [lunch], "action": refused })).unwrap_err().contains("action has to be"), "{refused}");
    }
    assert!(h.tool(&token, "update_threads", json!({ "thread_ids": ["nope"], "action": "done" })).unwrap_err().contains("no conversation"));

    let snoozed = h.tool(&token, "snooze_threads", json!({ "thread_ids": [lunch], "until": "2031-01-06T09:00" })).unwrap();
    assert_eq!(snoozed["until"], "2031-01-06T09:00:00+01:00");
    assert_eq!(h.tool(&token, "list_threads", json!({})).unwrap()["threads"], json!([]));
    assert_eq!(h.tool(&token, "list_threads", json!({ "view": "snoozed" })).unwrap()["threads"][0]["snoozedUntil"], "2031-01-06T09:00:00+01:00");
    assert!(h.tool(&token, "snooze_threads", json!({ "thread_ids": [lunch], "until": "2001-01-01" })).unwrap_err().contains("in the future"));
    h.tool(&token, "update_threads", json!({ "thread_ids": [lunch], "action": "inbox" })).unwrap();
    assert_eq!(h.tool(&token, "list_threads", json!({ "view": "snoozed" })).unwrap()["threads"], json!([]));

    // A draft needs its recipients; saving it needs the mailbox, which isn't there in this test.
    assert!(h.tool(&token, "create_draft", json!({ "body": "Hi" })).unwrap_err().contains("to is required"));
    assert!(h.tool(&token, "create_draft", json!({ "body": "Hi", "to": ["mia"], "subject": "x" })).unwrap_err().contains("not an email address"));
    assert!(h.tool(&token, "create_draft", json!({ "body": "Hi", "to": ["mia@example.net"] })).unwrap_err().contains("subject is required"));
    assert!(h.tool(&token, "create_draft", json!({ "body": "Hi", "reply_to_message_id": "nope" })).unwrap_err().contains("no message"));
    let message_id = thread["messages"][0]["id"].as_str().unwrap();
    let e = h.tool(&token, "create_draft", json!({ "body": "Friday works!", "reply_to_message_id": message_id })).unwrap_err();
    assert!(e.contains("could not save the draft in anna@example.org"), "{e}");

    // All of it is on record, without the text of the draft.
    let log = h.send("GET", "/v1/assistants/log?limit=3", Some(&app), None).json();
    assert_eq!((log[0]["tool"].as_str(), log[0]["ok"].as_bool()), (Some("create_draft"), Some(false)));
    assert_eq!(log[0]["arguments"]["body"], "(13 characters)");
    assert_eq!(h.send("GET", "/v1/assistants/log", Some(&token), None).status, 401);
}

#[test]
fn a_read_only_assistant_changes_nothing() {
    let h = Harness::new();
    h.with_mail();
    let token = h.connect(false);
    let names: Vec<String> =
        h.rpc(&token, "tools/list", json!({})).json()["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap().to_string()).collect();
    assert_eq!(names, ["list_accounts", "list_threads", "count_unread", "get_thread"]);
    let id = h.tool(&token, "list_threads", json!({})).unwrap()["threads"][0]["id"].as_str().unwrap().to_string();
    for (tool, args) in [
        ("update_threads", json!({ "thread_ids": [id], "action": "done" })),
        ("snooze_threads", json!({ "thread_ids": [id], "until": "2031-01-01" })),
        ("create_draft", json!({ "to": ["a@example.org"], "subject": "x", "body": "x" })),
    ] {
        assert!(h.tool(&token, tool, args).unwrap_err().contains("read-only"), "{tool}");
    }
    assert_eq!(h.tool(&token, "list_threads", json!({})).unwrap()["threads"].as_array().unwrap().len(), 2);
    // There is no sending tool to call at all.
    assert_eq!(h.rpc(&token, "tools/call", json!({ "name": "send_message" })).json()["error"]["code"], -32602);
}
