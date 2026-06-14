// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Heitor Faria

//! Driver scripts injected into the WhatsApp / WhatsApp-Business / Telegram
//! WebViews. These are *assets*, not code — two JS blobs we iterate on often.
//! Keeping them in their own leaf crate means editing a driver only recompiles
//! this trivial crate + its downstream relink, never the Tauri/wry glue.
//!
//! Communication protocol with the Rust orchestrator:
//!
//!   1. Rust fires `wv.eval("window.__vorcaro.scrapeChats('<platform>')")`.
//!   2. The driver walks the chat-list DOM and assembles a JSON array of rows.
//!   3. The driver calls
//!      `__TAURI__.core.invoke('vorcaro_scrape_result', { platform, rows })`.
//!   4. The Rust handler re-emits an event `vorcaro://scrape-result` that the
//!      studio panel listens for.

/// Injected into the `whatsapp` and `whatsapp_business` WebViews.
pub const VORCARO_WHATSAPP_DRIVER: &str = include_str!("../assets/whatsapp.js");

/// Injected into the `telegram` WebView.
pub const VORCARO_TELEGRAM_DRIVER: &str = include_str!("../assets/telegram.js");

/// Dev override: if `$BB_DRIVER_DIR` is set and holds `<file>`, load it from disk
/// at runtime so iterating on a driver needs **no Rust rebuild** — edit the JS,
/// restart the app. Falls back to the embedded copy. Release builds (no env var,
/// or a missing file) ship the fully embedded driver, so there are no external
/// files to distribute.
fn load_or(file: &str, embedded: &'static str) -> String {
    if let Ok(dir) = std::env::var("BB_DRIVER_DIR") {
        let path = std::path::Path::new(&dir).join(file);
        if let Ok(contents) = std::fs::read_to_string(&path) {
            return contents;
        }
    }
    embedded.to_string()
}

/// WhatsApp / WhatsApp-Business driver JS (honors `$BB_DRIVER_DIR` in dev).
pub fn whatsapp() -> String {
    load_or("whatsapp.js", VORCARO_WHATSAPP_DRIVER)
}

/// Telegram driver JS (honors `$BB_DRIVER_DIR` in dev).
pub fn telegram() -> String {
    load_or("telegram.js", VORCARO_TELEGRAM_DRIVER)
}
