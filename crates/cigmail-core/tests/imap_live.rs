//! Drafts against a real IMAP server. Needs a mailbox, so it only runs when asked:
//!
//! ```bash
//! CIGMAIL_TEST_IMAP=127.0.0.1:1143 CIGMAIL_TEST_USER=anna@example.org CIGMAIL_TEST_PASSWORD=... \
//!   cargo test -p cigmail-core --test imap_live -- --ignored
//! ```
//!
//! The server has to accept a plain connection (no TLS) and have a Drafts folder.

use std::sync::Arc;

use cigmail_core::db::{Db, ThreadFilter};
use cigmail_core::models::{Account, AccountKind, Address, Draft};
use cigmail_core::providers::Provider;
use cigmail_core::secrets::Secrets;
use cigmail_core::sync::Engine;
use cigmail_core::{NoHost, mime};

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs an IMAP mailbox; see the top of this file"]
async fn a_draft_is_saved_edited_and_removed() {
    let var = |k: &str| std::env::var(k).unwrap_or_else(|_| panic!("set {k}"));
    let (host, port) = var("CIGMAIL_TEST_IMAP").rsplit_once(':').map(|(h, p)| (h.to_string(), p.parse::<u16>().unwrap())).expect("host:port");
    let user = var("CIGMAIL_TEST_USER");

    let dir = std::env::temp_dir().join(format!("cigmail-live-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let db = Db::open(&dir.join("cigmail.db")).unwrap();
    let secrets = Secrets::new(db.clone());
    let account = Account {
        id: "live".into(),
        kind: AccountKind::Imap,
        email: user.clone(),
        name: "Live Test".into(),
        label: None,
        color: "#5e6ad2".into(),
        created_at: 0,
        last_sync: None,
        last_error: None,
        signature: None,
    };
    let config = serde_json::json!({ "imapHost": host, "imapPort": port, "imapSecurity": "none", "smtpHost": host, "smtpPort": 9,
                                     "smtpSecurity": "none", "username": user, "auth": "password" });
    db.insert_account(&account, &config).unwrap();
    // Straight to the database: a build machine has no keyring.
    db.set_fallback_secret("password:live", Some(&var("CIGMAIL_TEST_PASSWORD"))).unwrap();
    let engine = Engine::new(Arc::new(NoHost), tokio::runtime::Handle::current(), db.clone(), secrets, reqwest::Client::new());
    let ctx = engine.ctx("live").unwrap();
    let mut provider = Provider::new(&ctx).unwrap();
    let drafts = || db.list_threads(&ThreadFilter { view: "drafts".into(), account_id: None, query: None, limit: 100, offset: 0 }).unwrap();

    let subject = format!("Live draft {}", std::process::id());
    let written = Draft {
        account_id: "live".into(),
        to: vec![Address { name: Some("Mia".into()), email: "mia@example.net".into() }],
        subject: subject.clone(),
        html: "<p>Written by the test.</p>".into(),
        ..Default::default()
    };
    provider.save_draft(&ctx, &written).await.unwrap();
    provider.sync(&ctx).await.unwrap();
    let thread = drafts().into_iter().find(|t| t.subject == subject).expect("the saved draft shows in Drafts after a sync");
    let message = db.thread(&thread.id).unwrap().messages.into_iter().find(|m| m.is_draft).unwrap();

    // Opened for editing, it is the mail that was written.
    let editing = mime::draft_from_message(&message);
    assert_eq!((editing.subject.as_str(), editing.html.as_str()), (subject.as_str(), "<p>Written by the test.</p>"));
    assert_eq!(editing.to, written.to);
    assert_eq!(editing.replaces.as_deref(), Some(message.id.as_str()));

    // Sent or discarded: gone here at once, and gone on the server too.
    engine.discard_draft(&message.id).await.unwrap();
    assert!(drafts().iter().all(|t| t.subject != subject), "removed from the cache right away");
    provider.sync(&ctx).await.unwrap();
    assert!(drafts().iter().all(|t| t.subject != subject), "and a sync doesn't bring it back");
    assert!(engine.discard_draft(&message.id).await.is_err(), "nothing left to discard");
    let _ = std::fs::remove_dir_all(&dir);
}
