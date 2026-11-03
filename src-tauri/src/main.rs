// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // On Wayland with fractional scaling (e.g. Hyprland at 1.25), WebKitGTK's
    // accelerated paths draw the page at a stale size and the compositor squashes
    // it to fit the window, so everything looks stretched. Rendering without
    // DMA-BUF and accelerated compositing keeps the page at the real window size.
    // Must be set before GTK starts; explicit values from the environment win.
    #[cfg(target_os = "linux")]
    for var in ["WEBKIT_DISABLE_DMABUF_RENDERER", "WEBKIT_DISABLE_COMPOSITING_MODE"] {
        if std::env::var_os(var).is_none() {
            // SAFETY: still single-threaded; nothing else reads the environment yet.
            unsafe { std::env::set_var(var, "1") };
        }
    }
    cigmail_lib::run()
}
