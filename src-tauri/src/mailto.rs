//! Opening `mailto:` links. The system hands CigMail the links it's registered for (the desktop
//! entry on Linux, an intent filter on Android). A link always waits here until the UI takes it:
//! the UI asks once it has loaded, and again whenever a `mailto:open` event says another one came
//! in while it was running. Going through one slot means a link is never opened twice.

use std::sync::OnceLock;

use parking_lot::Mutex;
use tauri::{AppHandle, Emitter};

static PENDING: Mutex<Option<String>> = Mutex::new(None);

/// The running app, once set up; links arriving before that just wait.
static APP: OnceLock<AppHandle> = OnceLock::new();

pub fn init(app: &AppHandle) {
    let _ = APP.set(app.clone());
}

/// The first `mailto:` link among the arguments.
#[cfg_attr(mobile, allow(dead_code))] // desktop gets links as arguments
pub fn find<I: IntoIterator<Item = S>, S: AsRef<str>>(args: I) -> Option<String> {
    args.into_iter()
        .map(|a| a.as_ref().to_owned())
        .find(|a| a.len() > 7 && a[..7].eq_ignore_ascii_case("mailto:"))
}

/// A link to open: kept for the UI, which is told about it if it's already running.
pub fn open(url: String) {
    *PENDING.lock() = Some(url);
    if let Some(app) = APP.get() {
        let _ = app.emit("mailto:open", ());
    }
}

/// The link waiting to be opened, if any (returned once).
#[tauri::command]
pub fn take_mailto() -> Option<String> {
    PENDING.lock().take()
}

/// `NativeBridge.nativeMailto(url)` in Kotlin: the activity was opened with a `mailto:` link.
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_ciger_cigmail_NativeBridge_nativeMailto(
    mut env: jni::JNIEnv,
    _class: jni::objects::JClass,
    url: jni::objects::JString,
) {
    if let Ok(url) = env.get_string(&url) {
        open(url.into());
    }
}

#[cfg(test)]
mod tests {
    use super::find;

    #[test]
    fn finds_link_among_args() {
        assert_eq!(find(["cigmail", "MAILTO:a@b.c"]), Some("MAILTO:a@b.c".into()));
        assert_eq!(find(["cigmail", "--background"]), None);
        assert_eq!(find(["mailto:"]), None);
    }
}
