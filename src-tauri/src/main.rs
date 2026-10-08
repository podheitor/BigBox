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
        // DASH/HLS so MSE-based players (WhatsApp, Telegram) can stream
        // chunked video.
        std::env::set_var("WEBKIT_GST_ENABLE_DASH_SUPPORT", "1");
        std::env::set_var("WEBKIT_GST_ENABLE_HLS_SUPPORT", "1");
        // NVIDIA-only workarounds. On AMD/Intel they cost a GPU readback +
        // CPU copy per frame, which made typing in the webviews laggy.
        if nvidia_driver_loaded() {
            // DMABUF renderer is unstable on NVIDIA drivers; keep off.
            std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
            // Force software compositing. WebKit's GL compositor on NVIDIA
            // + X11 paints vertical grey strips across the chat area and
            // leaves video frames grey.
            // Override via BB_NO_FORCE_SW_COMPOSITING for diagnostic runs.
            if std::env::var("BB_NO_FORCE_SW_COMPOSITING").is_err() {
                std::env::set_var("WEBKIT_DISABLE_COMPOSITING_MODE", "1");
            }
        }
    }

    bigbox_lib::run();
}

/// True when an NVIDIA kernel driver (proprietary or open) is loaded.
#[cfg(target_os = "linux")]
fn nvidia_driver_loaded() -> bool {
    std::path::Path::new("/proc/driver/nvidia/version").exists()
}
