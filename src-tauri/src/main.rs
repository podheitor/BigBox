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
        // DASH/HLS so MSE-based players (WhatsApp, Telegram) can stream
        // chunked video.
        std::env::set_var("WEBKIT_GST_ENABLE_DASH_SUPPORT", "1");
        std::env::set_var("WEBKIT_GST_ENABLE_HLS_SUPPORT", "1");
        // Force software compositing. WebKit's GL compositor on NVIDIA
        // proprietary + X11 paints vertical grey strips across the chat
        // area and leaves video frames grey. Software compositing avoids
        // the broken GL texture upload.
        // Override via BB_NO_FORCE_SW_COMPOSITING for diagnostic runs.
        if std::env::var("BB_NO_FORCE_SW_COMPOSITING").is_err() {
            std::env::set_var("WEBKIT_DISABLE_COMPOSITING_MODE", "1");
        }
    }

    bigbox_lib::run();
}
