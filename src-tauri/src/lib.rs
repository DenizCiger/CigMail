mod ai;
mod commands;
mod config;
mod db;
mod error;
mod mime;
mod models;
mod oauth;
mod providers;
mod query;
mod secrets;
mod sync;

use std::{collections::HashMap, sync::Arc, time::Duration};

use parking_lot::Mutex;
use tauri::Manager;

use commands::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "cigmail_lib=info,warn,html5ever=error".into()),
        )
        .init();
    // Several dependencies pull in rustls; pick one crypto provider for all of them.
    let _ = rustls::crypto::ring::default_provider().install_default();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let db = db::Db::open(&dir.join("cigmail.db"))?;
            let secrets = secrets::Secrets::new(db.clone());
            let http = reqwest::Client::builder()
                .user_agent(concat!("CigMail/", env!("CARGO_PKG_VERSION")))
                .connect_timeout(Duration::from_secs(15))
                .timeout(Duration::from_secs(120))
                .build()?;
            let engine = sync::Engine::new(app.handle().clone(), db.clone(), secrets.clone(), http.clone());
            let ai = Arc::new(ai::Ai::new(db.clone(), secrets.clone(), http.clone()));
            app.manage(AppState {
                db,
                secrets,
                http,
                engine: engine.clone(),
                ai,
                ai_tasks: Mutex::new(HashMap::new()),
            });
            engine.start_all()?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_accounts,
            commands::add_oauth_account,
            commands::add_imap_account,
            commands::test_imap,
            commands::detect_servers,
            commands::reconnect_account,
            commands::update_account,
            commands::remove_account,
            commands::cancel_oauth,
            commands::sync_now,
            commands::resync_account,
            commands::list_threads,
            commands::count_unread,
            commands::get_thread,
            commands::thread_action,
            commands::snooze,
            commands::reply_draft,
            commands::send_message,
            commands::cancel_send,
            commands::failed_sends,
            commands::save_draft,
            commands::list_drafts,
            commands::delete_draft,
            commands::suggest_contacts,
            commands::open_attachment,
            commands::save_attachments,
            commands::stage_attachment,
            commands::get_settings,
            commands::set_setting,
            commands::get_splits,
            commands::set_splits,
            commands::oauth_config_status,
            commands::ai_status,
            commands::ai_login,
            commands::ai_logout,
            commands::ai_models,
            commands::ai_cancel,
            commands::ai_summarize,
            commands::ai_compose,
            commands::ai_rewrite,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
