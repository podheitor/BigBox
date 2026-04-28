// Prevents extra console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    #[cfg(target_os = "linux")]
    {
        // Disable AT-SPI bridge: WebKit loads libatk-bridge-2.0, which segfaults
        // in spi_register_object_to_path during webview init when the a11y bus
        // isn't reachable (timeout → bad pointer). NO_AT_BRIDGE=1 prevents the
        // module from loading at all.
        std::env::set_var("NO_AT_BRIDGE", "1");
        // DMABUF renderer is unstable on NVIDIA proprietary drivers; keep off.
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        // WebKit's accelerated-compositing texture upload path is broken on
        // NVIDIA proprietary + X11 + GTK3: video frames render as grey diff
        // tiles and dark UI backgrounds paint black. Disabling AC routes
        // layer painting through Cairo, which renders correctly at the cost
        // of CPU-side CSS animations.
        std::env::set_var("WEBKIT_DISABLE_COMPOSITING_MODE", "1");
        // DASH/HLS so MSE-based players (WhatsApp, Telegram) can stream
        // chunked video. Note: WEBKIT_GST_USE_PLAYBIN3 is intentionally
        // left unset — playbin3 issues parallel Range requests on blob://
        // URLs which scrambles qtdemux input ("atom has bogus size").
        std::env::set_var("WEBKIT_GST_ENABLE_DASH_SUPPORT", "1");
        std::env::set_var("WEBKIT_GST_ENABLE_HLS_SUPPORT", "1");
    }

    bigbox_lib::run();
}
