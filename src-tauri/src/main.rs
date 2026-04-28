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
        // chunked video. WEBKIT_GST_USE_PLAYBIN3 is left unset — playbin3
        // issues parallel Range requests on blob:// URLs which scrambles
        // qtdemux input ("atom has bogus size"). MSE/blob already force
        // playbin3 internally where it's needed.
        std::env::set_var("WEBKIT_GST_ENABLE_DASH_SUPPORT", "1");
        std::env::set_var("WEBKIT_GST_ENABLE_HLS_SUPPORT", "1");
        // WhatsApp serves AAC-HE / AAC-HEv2 audio. libav's avdec_aac (the
        // default at rank primary) only decodes AAC-LC and produces
        // "Number of bands exceeds limit" / "Prediction is not allowed in
        // AAC-LC" errors that abort the whole video pipeline. Promote
        // fdkaacdec (proper AAC-HE decoder) above the broken decoders.
        std::env::set_var(
            "GST_PLUGIN_FEATURE_RANK",
            "fdkaacdec:MAX,faad:512,avdec_aac:NONE",
        );
    }

    bigbox_lib::run();
}
