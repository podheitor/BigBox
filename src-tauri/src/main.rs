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
    }

    bigbox_lib::run();
}
