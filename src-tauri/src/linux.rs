//! Linux desktop integration that Tauri's plugins don't cover.
//!
//! - The tray icon is a StatusNotifierItem of our own (ksni): Tauri's appindicator tray can only
//!   show a menu, so a left click couldn't open the window.
//! - Notifications go straight to `org.freedesktop.Notifications` with a default action, so
//!   clicking one opens the conversation; Tauri's notification plugin ignores clicks.
//!
//! Both pass on the XDG activation token the shell hands out with the click: Wayland
//! compositors (Hyprland among them) only let a window take focus with one.

use std::{collections::HashMap, sync::Arc, time::Duration};

use futures::StreamExt;
use parking_lot::Mutex;
use tauri::{AppHandle, Emitter, Manager};

/// Shows the main window and gives it focus, using the activation token if there is one.
pub fn present(app: &AppHandle, token: Option<String>) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        let Some(w) = handle.get_webview_window("main") else { return };
        let _ = w.unminimize();
        let _ = w.show();
        match (token, w.gtk_window()) {
            (Some(token), Ok(gtk_window)) => {
                use gtk::prelude::GtkWindowExt;
                gtk_window.set_startup_id(&token);
                gtk_window.present();
            }
            _ => {
                let _ = w.set_focus();
            }
        }
    });
}

// ------------------------------------------------------------------ tray

struct Tray {
    app: AppHandle,
    icon: Vec<ksni::Icon>,
}

/// The activation token for the click being handled. It can reach us a moment after the click
/// itself, so wait for it briefly.
fn click_token() -> Option<String> {
    for _ in 0..25 {
        if let Some(t) = ksni::current_xdg_activation_token() {
            return Some(t);
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    None
}

impl ksni::Tray for Tray {
    fn id(&self) -> String {
        "cigmail".into()
    }

    fn title(&self) -> String {
        "CigMail".into()
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        self.icon.clone()
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip { title: "CigMail".into(), ..Default::default() }
    }

    /// Left click: open the window, or put it away if it's already in front.
    fn activate(&mut self, _x: i32, _y: i32) {
        let token = click_token();
        let in_front = self
            .app
            .get_webview_window("main")
            .is_some_and(|w| w.is_visible().unwrap_or(false) && w.is_focused().unwrap_or(false));
        if in_front {
            if let Some(w) = self.app.get_webview_window("main") {
                let _ = w.hide();
            }
        } else {
            present(&self.app, token);
        }
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::StandardItem;
        vec![
            StandardItem {
                label: "Open CigMail".into(),
                activate: Box::new(|t: &mut Self| present(&t.app, click_token())),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Check for mail".into(),
                activate: Box::new(|t: &mut Self| t.app.state::<crate::commands::AppState>().engine.wake_all()),
                ..Default::default()
            }
            .into(),
            ksni::MenuItem::Separator,
            StandardItem {
                label: "Quit CigMail".into(),
                activate: Box::new(|t: &mut Self| t.app.exit(0)),
                ..Default::default()
            }
            .into(),
        ]
    }
}

/// The handle keeps the tray registered for as long as the app runs.
struct TrayHandle(#[allow(dead_code)] ksni::Handle<Tray>);

pub fn tray(app: &tauri::App) {
    // The window icon, converted from RGBA to the ARGB the tray protocol wants.
    let icon = app
        .default_window_icon()
        .map(|i| {
            let mut data = i.rgba().to_vec();
            for px in data.as_chunks_mut::<4>().0 {
                px.rotate_right(1);
            }
            vec![ksni::Icon { width: i.width() as i32, height: i.height() as i32, data }]
        })
        .unwrap_or_default();
    let handle = app.handle().clone();
    tauri::async_runtime::spawn(async move {
        use ksni::TrayMethods;
        match (Tray { app: handle.clone(), icon }).spawn().await {
            Ok(tray) => {
                handle.manage(TrayHandle(tray));
            }
            Err(e) => tracing::warn!("no tray icon: {e}"),
        }
    });
}

// ------------------------------------------------------------------ notifications

#[zbus::proxy(
    interface = "org.freedesktop.Notifications",
    default_service = "org.freedesktop.Notifications",
    default_path = "/org/freedesktop/Notifications"
)]
trait Notifications {
    #[allow(clippy::too_many_arguments)]
    fn notify(
        &self,
        app_name: &str,
        replaces_id: u32,
        app_icon: &str,
        summary: &str,
        body: &str,
        actions: &[&str],
        hints: HashMap<&str, zbus::zvariant::Value<'_>>,
        expire_timeout: i32,
    ) -> zbus::Result<u32>;

    #[zbus(signal)]
    fn action_invoked(&self, id: u32, action_key: String) -> zbus::Result<()>;

    #[zbus(signal)]
    fn activation_token(&self, id: u32, activation_token: String) -> zbus::Result<()>;

    #[zbus(signal)]
    fn notification_closed(&self, id: u32, reason: u32) -> zbus::Result<()>;
}

/// What a click on one of our notifications should do.
enum Target {
    /// Open the app.
    App,
    /// Open this conversation.
    Thread(String),
}

#[derive(Default)]
struct Shown {
    targets: HashMap<u32, Target>,
    /// Activation tokens arrive just before the click they belong to.
    tokens: HashMap<u32, String>,
}

pub struct Notifier {
    proxy: tokio::sync::OnceCell<Option<NotificationsProxy<'static>>>,
    shown: Arc<Mutex<Shown>>,
    app: AppHandle,
}

impl Notifier {
    pub fn new(app: AppHandle) -> Self {
        Notifier { proxy: tokio::sync::OnceCell::new(), shown: Arc::default(), app }
    }

    /// Connects on first use and starts listening for clicks.
    async fn proxy(&self) -> Option<&NotificationsProxy<'static>> {
        self.proxy
            .get_or_init(|| async {
                let proxy = async {
                    let conn = zbus::Connection::session().await?;
                    NotificationsProxy::new(&conn).await
                }
                .await;
                match proxy {
                    Ok(p) => {
                        self.listen(p.clone());
                        Some(p)
                    }
                    Err(e) => {
                        tracing::warn!("no notification service: {e}");
                        None
                    }
                }
            })
            .await
            .as_ref()
    }

    fn listen(&self, proxy: NotificationsProxy<'static>) {
        let shown = self.shown.clone();
        let app = self.app.clone();
        tauri::async_runtime::spawn(async move {
            let (Ok(actions), Ok(tokens), Ok(closed)) = (
                proxy.receive_action_invoked().await,
                proxy.receive_activation_token().await,
                proxy.receive_notification_closed().await,
            ) else {
                tracing::warn!("can't listen for notification clicks");
                return;
            };
            enum Ev {
                Action(u32, String),
                Token(u32, String),
                Closed(u32),
            }
            let actions = actions.filter_map(|s| async move { s.args().ok().map(|a| Ev::Action(a.id, a.action_key)) });
            let tokens = tokens.filter_map(|s| async move { s.args().ok().map(|a| Ev::Token(a.id, a.activation_token)) });
            let closed = closed.filter_map(|s| async move { s.args().ok().map(|a| Ev::Closed(a.id)) });
            let mut events = futures::stream::select(futures::stream::select(actions, tokens), closed).boxed();
            while let Some(ev) = events.next().await {
                match ev {
                    Ev::Token(id, token) => {
                        let mut s = shown.lock();
                        if s.targets.contains_key(&id) {
                            s.tokens.insert(id, token);
                        }
                    }
                    Ev::Action(id, key) if key == "default" => {
                        let (target, token) = {
                            let mut s = shown.lock();
                            (s.targets.remove(&id), s.tokens.remove(&id))
                        };
                        let Some(target) = target else { continue };
                        present(&app, token);
                        if let Target::Thread(thread) = target {
                            let _ = app.emit("notification:open", thread);
                        }
                    }
                    Ev::Action(..) => {}
                    Ev::Closed(id) => {
                        let mut s = shown.lock();
                        s.targets.remove(&id);
                        s.tokens.remove(&id);
                    }
                }
            }
        });
    }

    pub fn show(self: &Arc<Self>, title: &str, body: &str, thread: Option<&str>) {
        let me = self.clone();
        let (title, body, target) =
            (title.to_string(), body.to_string(), thread.map_or(Target::App, |t| Target::Thread(t.into())));
        tauri::async_runtime::spawn(async move {
            let Some(proxy) = me.proxy().await else { return };
            let mut hints = HashMap::new();
            // Lets the shell match the notification to the app's launcher entry and icon.
            hints.insert("desktop-entry", zbus::zvariant::Value::from("cigmail"));
            match proxy.notify("CigMail", 0, "cigmail", &title, &body, &["default", "Open"], hints, -1).await {
                Ok(id) => {
                    me.shown.lock().targets.insert(id, target);
                }
                Err(e) => tracing::warn!("notification failed: {e}"),
            }
        });
    }
}
