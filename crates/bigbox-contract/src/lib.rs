// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Heitor Faria

//! `bigbox-contract` — the ports the campaign engine (`bigbox-orchestrator`)
//! talks *out* through, so the engine never names `tauri`.
//!
//! The Tauri layer (`bigbox-vorcaro`) provides the concrete implementations
//! (over `AppHandle` / `Emitter` / `get_webview`) and injects them as
//! `Arc<dyn …>` when starting a campaign. Both ports are intentionally
//! **synchronous, fire-and-forget**: `wv.eval` and `app.emit` return
//! immediately, and the engine awaits the driver's reply via its own
//! `oneshot` channel — so no `async_trait` is needed here.

use uuid::Uuid;

/// Port 1: how the engine pushes JS into a live chat-service WebView.
///
/// The engine identifies a webview by its BigBox label (`svc-<id>`); the
/// adapter resolves that to the actual webview and runs the script.
pub trait DriverTransport: Send + Sync {
    /// Whether a service WebView with this label currently exists / is open.
    fn webview_exists(&self, service_label: &str) -> bool;

    /// Evaluate `js` in the named WebView. Fire-and-forget from the engine's
    /// point of view (results arrive later via an IPC callback). Returns `Err`
    /// if the webview isn't open or the eval call itself failed.
    fn eval(&self, service_label: &str, js: &str) -> Result<(), String>;
}

/// Port 2: how the engine emits campaign progress to the UI.
///
/// The adapter is responsible for wrapping `(campaign_id, kind, payload)` into
/// whatever envelope/event-name the frontend listens for.
pub trait ProgressSink: Send + Sync {
    fn emit(&self, campaign_id: Uuid, kind: &str, payload: serde_json::Value);
}
