//! Keeping the mail engine running while nobody is looking at the window, so new mail still
//! gets a notification.
//!
//! Desktop: closing the window hides it to a tray icon instead of quitting (on Linux the tray
//! lives in `linux.rs`), and the app can start at login without opening its window. Android: a
//! foreground service (`MailService.kt`) keeps the process alive, and an alarm wakes the engine
//! for a sync while the phone dozes.

use tauri::{AppHandle, Manager, RunEvent, WindowEvent};

use crate::commands::AppState;

/// Started this way at login: run in the tray without opening the window.
pub const BACKGROUND_ARG: &str = "--background";

/// Whether closing the window keeps the app running in the tray. On by default.
fn close_to_tray(app: &AppHandle) -> bool {
    app.state::<AppState>().db.get_setting("close_to_tray").ok().flatten().is_none_or(|v| v != "false")
}

/// Brings the main window back from the tray.
#[cfg(desktop)]
pub fn show_main(app: &AppHandle) {
    #[cfg(target_os = "linux")]
    return crate::linux::present(app, None);
    #[cfg(not(target_os = "linux"))]
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// Creates the main window (hidden when started at login) and, on desktop, the tray icon.
pub fn setup(app: &tauri::App) -> tauri::Result<()> {
    let config = app.config().app.windows.iter().find(|w| w.label == "main").cloned().expect("main window config");
    let hidden = cfg!(desktop) && std::env::args().any(|a| a == BACKGROUND_ARG);
    tauri::WebviewWindowBuilder::from_config(app.handle(), &config)?.visible(!hidden).build()?;
    #[cfg(target_os = "linux")]
    crate::linux::tray(app);
    #[cfg(any(target_os = "macos", windows))]
    tray(app)?;
    #[cfg(desktop)]
    autostart::refresh();
    Ok(())
}

/// macOS and Windows; Linux has its own (`linux.rs`).
#[cfg(any(target_os = "macos", windows))]
fn tray(app: &tauri::App) -> tauri::Result<()> {
    use tauri::{
        menu::{Menu, MenuItem},
        tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    };
    let open = MenuItem::with_id(app, "open", "Open CigMail", true, None::<&str>)?;
    let sync = MenuItem::with_id(app, "sync", "Check for mail", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit CigMail", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &sync, &quit])?;
    let mut tray = TrayIconBuilder::with_id("cigmail")
        .tooltip("CigMail")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, e| match e.id.as_ref() {
            "open" => show_main(app),
            "sync" => app.state::<AppState>().engine.wake_all(),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, e| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
                show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

/// Window events: closing hides to the tray instead of quitting (desktop).
pub fn on_window_event(window: &tauri::Window, event: &WindowEvent) {
    #[cfg(desktop)]
    if let WindowEvent::CloseRequested { api, .. } = event
        && window.label() == "main"
        && close_to_tray(window.app_handle())
    {
        api.prevent_close();
        let _ = window.hide();
    }
    #[cfg(mobile)]
    let _ = (window, event);
}

pub fn on_run_event(app: &AppHandle, event: &RunEvent) {
    match event {
        // Android tears the window down when the app is swiped away from recents; the process
        // lives on in the foreground service and keeps syncing. An explicit `exit` has a code.
        #[cfg(target_os = "android")]
        RunEvent::ExitRequested { code: None, api, .. } => api.prevent_exit(),
        // Clicking the dock icon after the window was closed.
        #[cfg(target_os = "macos")]
        RunEvent::Reopen { .. } => show_main(app),
        _ => {}
    }
    let _ = app;
}

#[tauri::command]
pub fn get_autostart() -> Option<bool> {
    #[cfg(desktop)]
    return autostart::enabled();
    #[cfg(mobile)]
    None
}

#[tauri::command]
pub fn set_autostart(enabled: bool) -> crate::error::CmdResult<()> {
    #[cfg(desktop)]
    autostart::set(enabled)?;
    #[cfg(mobile)]
    let _ = enabled;
    Ok(())
}

/// Starting at login. Linux only for now, through an XDG autostart entry; the autostart plugin
/// can't be used because it points at the executable, which for an AppImage is a temporary mount.
#[cfg(desktop)]
mod autostart {
    #[cfg(target_os = "linux")]
    fn entry() -> Option<std::path::PathBuf> {
        let config = std::env::var_os("XDG_CONFIG_HOME")
            .map(std::path::PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config")))?;
        Some(config.join("autostart").join("cigmail.desktop"))
    }

    #[cfg(target_os = "linux")]
    fn exe() -> anyhow::Result<String> {
        Ok(match std::env::var("APPIMAGE") {
            Ok(p) if !p.is_empty() => p,
            _ => std::env::current_exe()?.display().to_string(),
        })
    }

    #[cfg(target_os = "linux")]
    pub fn enabled() -> Option<bool> {
        Some(entry()?.exists())
    }

    #[cfg(target_os = "linux")]
    pub fn set(enabled: bool) -> anyhow::Result<()> {
        let path = entry().ok_or_else(|| anyhow::anyhow!("no home directory"))?;
        if !enabled {
            if path.exists() {
                std::fs::remove_file(&path)?;
            }
            return Ok(());
        }
        std::fs::create_dir_all(path.parent().unwrap())?;
        let exe = exe()?.replace('\\', "\\\\").replace('"', "\\\"");
        std::fs::write(
            &path,
            format!(
                "[Desktop Entry]\nType=Application\nName=CigMail\nComment=Check for new mail in the background\nExec=\"{exe}\" {}\nIcon=cigmail\nTerminal=false\nX-GNOME-Autostart-enabled=true\n",
                super::BACKGROUND_ARG
            ),
        )?;
        Ok(())
    }

    /// The app may have moved (a new AppImage, a rebuilt binary) since the entry was written.
    #[cfg(target_os = "linux")]
    pub fn refresh() {
        if enabled() == Some(true)
            && let Err(e) = set(true)
        {
            tracing::warn!("could not update the autostart entry: {e:#}");
        }
    }

    #[cfg(not(target_os = "linux"))]
    pub fn enabled() -> Option<bool> {
        None
    }

    #[cfg(not(target_os = "linux"))]
    pub fn set(_enabled: bool) -> anyhow::Result<()> {
        anyhow::bail!("starting at login isn't supported on this system yet")
    }

    #[cfg(not(target_os = "linux"))]
    pub fn refresh() {}
}

/// The engine, for the Android service's alarm to wake. Set once at startup.
#[cfg(target_os = "android")]
pub static ENGINE: std::sync::OnceLock<std::sync::Arc<crate::sync::Engine>> = std::sync::OnceLock::new();

/// `NativeBridge.nativeWake()` in Kotlin: the doze-proof alarm fired; sync every account now.
/// The service holds a wake lock for a minute so the sync can finish before the CPU sleeps.
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_ciger_cigmail_NativeBridge_nativeWake(
    _env: *mut std::ffi::c_void,
    _class: *mut std::ffi::c_void,
) {
    if let Some(engine) = ENGINE.get() {
        tracing::info!("background alarm: syncing");
        engine.wake_all();
    }
}
