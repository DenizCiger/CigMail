mod ai;
mod background;
mod commands;
mod error;
mod mailto;
#[cfg(target_os = "linux")]
mod linux;
mod server;

use std::{collections::HashMap, sync::Arc, time::Duration};

use parking_lot::Mutex;
use tauri::{AppHandle, Emitter, Manager};
#[cfg(not(target_os = "linux"))]
use tauri_plugin_notification::NotificationExt;

// The mail engine lives in its own crate, shared with the server for AI assistants.
pub(crate) use cigmail_core::{config, db, http_client, mime, models, oauth, providers, secrets, sync};

use commands::AppState;

/// The engine's view of the app: events go to the window, notifications to the system.
struct AppHost {
    app: AppHandle,
    /// Clickable notifications; the notification plugin's can't be clicked on Linux.
    #[cfg(target_os = "linux")]
    notifier: Arc<linux::Notifier>,
}

impl AppHost {
    fn new(app: AppHandle) -> Self {
        AppHost {
            #[cfg(target_os = "linux")]
            notifier: Arc::new(linux::Notifier::new(app.clone())),
            app,
        }
    }
}

impl cigmail_core::Host for AppHost {
    fn emit(&self, event: &str, payload: serde_json::Value) {
        let _ = self.app.emit(event, payload);
    }

    fn notify(&self, title: &str, body: &str) {
        #[cfg(target_os = "linux")]
        self.notifier.show(title, body, None);
        #[cfg(not(target_os = "linux"))]
        let _ = self.app.notification().builder().title(title).body(body).show();
    }

    fn notify_thread(&self, title: &str, body: &str, thread_id: &str) {
        #[cfg(target_os = "linux")]
        self.notifier.show(title, body, Some(thread_id));
        // Elsewhere a click opens the app at best; the thread is still at the top of the inbox.
        #[cfg(not(target_os = "linux"))]
        {
            let _ = thread_id;
            self.notify(title, body);
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "cigmail_lib=info,warn,html5ever=error".into());
    #[cfg(target_os = "android")]
    {
        use tracing_subscriber::layer::SubscriberExt as _;
        use tracing_subscriber::util::SubscriberInitExt as _;
        tracing_subscriber::registry().with(filter).with(paranoid_android::layer("cigmail")).init();
    }
    #[cfg(not(target_os = "android"))]
    tracing_subscriber::fmt().with_env_filter(filter).init();
    // A panicking command task otherwise vanishes silently on Android (no
    // stderr), leaving the UI waiting for a reply that never comes.
    #[cfg(target_os = "android")]
    std::panic::set_hook(Box::new(|info| tracing::error!("panic: {info}")));
    // Several dependencies pull in rustls; pick one crypto provider for all of them.
    let _ = rustls::crypto::ring::default_provider().install_default();

    #[allow(unused_mut)] // only desktop adds plugins here
    let mut builder = tauri::Builder::default();
    // A second launch (say, from the menu while the app sits in the tray after starting at
    // login) opens the running instance's window instead of starting a second engine.
    #[cfg(desktop)]
    {
        // A `mailto:` link opened elsewhere (the desktop entry registers CigMail for them) starts
        // the app with the link as an argument; in a second launch it goes to the running one.
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            background::show_main(app);
            if let Some(url) = mailto::find(args) {
                mailto::open(url);
            }
        }));
        if let Some(url) = mailto::find(std::env::args().skip(1)) {
            mailto::open(url);
        }
    }
    builder
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let db = db::Db::open(&dir.join("cigmail.db"))?;
            let secrets = secrets::Secrets::new(db.clone());
            let http = http_client()
                .connect_timeout(Duration::from_secs(15))
                .timeout(Duration::from_secs(120))
                .build()?;
            let engine = sync::Engine::new(
                Arc::new(AppHost::new(app.handle().clone())),
                tauri::async_runtime::handle().inner().clone(),
                db.clone(),
                secrets.clone(),
                http.clone(),
            );
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
            #[cfg(target_os = "android")]
            let _ = background::ENGINE.set(engine.clone());
            mailto::init(app.handle());
            background::setup(app)?;
            Ok(())
        })
        .on_window_event(background::on_window_event)
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
            commands::edit_server_draft,
            commands::discard_server_draft,
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
            server::server_status,
            server::server_connect,
            server::server_push,
            server::server_disconnect,
            server::server_remove_device,
            background::get_autostart,
            background::set_autostart,
            mailto::take_mailto,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| background::on_run_event(app, &event));
}
