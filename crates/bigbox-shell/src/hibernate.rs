// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Heitor Faria

//! Memory footprint of the service webviews.
//!
//! Every service runs in its own WebKit context (one `data_directory` each),
//! so each one owns a full WebKitWebProcess that is never released while the
//! view is merely hidden. Two levers:
//!
//! * a leaner cache model applied to every service context, and
//! * opt-in hibernation: a service with `hibernate = true` that stays hidden
//!   for [`IDLE_BEFORE_HIBERNATE`] gets its web process terminated; the next
//!   `open_service` reloads it. A hibernated service raises no notifications.

use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager};

use bigbox_config::config;

use crate::commands::AppState;

/// How long a hidden service must stay unused before it is hibernated.
pub const IDLE_BEFORE_HIBERNATE: Duration = Duration::from_secs(20 * 60);

/// How often the hibernation sweep runs.
const SWEEP_INTERVAL: Duration = Duration::from_secs(60);

/// Apply the lean cache model to a freshly created service webview.
/// `DocumentBrowser` keeps the disk cache (fast wake-up after hibernation)
/// but shrinks the in-memory caches that `WebBrowser` sizes for a browser.
#[cfg(target_os = "linux")]
pub fn apply_lean_cache_model(wv: &tauri::Webview) {
    let _ = wv.with_webview(|platform_wv| {
        use webkit2gtk::{CacheModel, WebContextExt, WebViewExt};
        if let Some(ctx) = platform_wv.inner().context() {
            ctx.set_cache_model(CacheModel::DocumentBrowser);
        }
    });
}

#[cfg(not(target_os = "linux"))]
pub fn apply_lean_cache_model(_wv: &tauri::Webview) {}

/// Record that `label` was visible until now (called when it gains or loses
/// the active slot, and at creation for preloaded views).
pub fn mark_used(state: &AppState, label: &str) {
    state.last_used.lock().unwrap().insert(label.to_string(), Instant::now());
}

/// If `label` was hibernated, reload it so a new web process is spawned.
/// Returns true when a wake-up reload was issued.
pub fn wake_if_hibernated(app: &AppHandle, state: &AppState, label: &str) -> bool {
    if !state.hibernated.lock().unwrap().remove(label) {
        return false;
    }
    if let Some(wv) = app.get_webview(label) {
        reload_webview(&wv);
    }
    true
}

#[cfg(target_os = "linux")]
fn reload_webview(wv: &tauri::Webview) {
    let _ = wv.with_webview(|platform_wv| {
        use webkit2gtk::WebViewExt;
        platform_wv.inner().reload();
    });
}

#[cfg(not(target_os = "linux"))]
fn reload_webview(wv: &tauri::Webview) {
    let _ = wv.eval("window.location.reload()");
}

/// Spawn the background sweep that hibernates idle opt-in services.
pub fn start_hibernation_sweep(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut tick = tokio::time::interval(SWEEP_INTERVAL);
        loop {
            tick.tick().await;
            sweep_idle_services(&app);
        }
    });
}

fn sweep_idle_services(app: &AppHandle) {
    let state: tauri::State<'_, AppState> = app.state();
    for label in idle_hibernation_candidates(&state) {
        let Some(wv) = app.get_webview(&label) else { continue };
        if terminate_web_process(&wv) {
            state.hibernated.lock().unwrap().insert(label);
        }
    }
}

/// Labels of opt-in services that are created, not active, not yet
/// hibernated, and unused for longer than [`IDLE_BEFORE_HIBERNATE`].
fn idle_hibernation_candidates(state: &AppState) -> Vec<String> {
    let opted_in: Vec<String> = config::load()
        .services
        .iter()
        .filter(|s| s.hibernate)
        .map(|s| format!("svc-{}", s.id))
        .collect();
    filter_idle_labels(state, opted_in, Instant::now())
}

/// Pure part of the sweep: keep the `opted_in` labels that are eligible at `now`.
fn filter_idle_labels(state: &AppState, opted_in: Vec<String>, now: Instant) -> Vec<String> {
    let active = state.active_view.lock().unwrap().clone();
    let created = state.created_views.lock().unwrap();
    let hibernated = state.hibernated.lock().unwrap();
    let last_used = state.last_used.lock().unwrap();
    let idle_long_enough = |l: &String| {
        last_used.get(l).is_some_and(|t| now.duration_since(*t) >= IDLE_BEFORE_HIBERNATE)
    };
    opted_in
        .into_iter()
        .filter(|l| created.contains(l) && !hibernated.contains(l))
        .filter(|l| active.as_deref() != Some(l.as_str()))
        .filter(idle_long_enough)
        .collect()
}

#[cfg(target_os = "linux")]
fn terminate_web_process(wv: &tauri::Webview) -> bool {
    wv.with_webview(|platform_wv| {
        use webkit2gtk::WebViewExt;
        platform_wv.inner().terminate_web_process();
    })
    .is_ok()
}

/// WebView2 has no per-view process kill; hibernation is Linux-only.
#[cfg(not(target_os = "linux"))]
fn terminate_web_process(_wv: &tauri::Webview) -> bool {
    false
}

/// Flip a service's `hibernate` flag in the config. A service turned off
/// while hibernated is woken so it resumes notifying.
pub fn toggle_hibernate(app: &AppHandle, service_id: &str) {
    let mut cfg = config::load();
    let Some(svc) = cfg.services.iter_mut().find(|s| s.id == service_id) else {
        return;
    };
    svc.hibernate = !svc.hibernate;
    let enabled = svc.hibernate;
    config::save(&cfg);
    if !enabled {
        let state: tauri::State<'_, AppState> = app.state();
        wake_if_hibernated(app, &state, &format!("svc-{service_id}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state_with(labels: &[&str], used_at: Instant) -> AppState {
        let state = AppState::default();
        for l in labels {
            state.created_views.lock().unwrap().insert((*l).to_string());
            state.last_used.lock().unwrap().insert((*l).to_string(), used_at);
        }
        state
    }

    fn labels(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn idle_hidden_opt_in_service_is_selected() {
        let t0 = Instant::now();
        let state = state_with(&["svc-gmail"], t0);
        let later = t0 + IDLE_BEFORE_HIBERNATE;
        assert_eq!(filter_idle_labels(&state, labels(&["svc-gmail"]), later), labels(&["svc-gmail"]));
    }

    #[test]
    fn recently_used_service_is_kept() {
        let t0 = Instant::now();
        let state = state_with(&["svc-gmail"], t0);
        let soon = t0 + IDLE_BEFORE_HIBERNATE - Duration::from_secs(1);
        assert!(filter_idle_labels(&state, labels(&["svc-gmail"]), soon).is_empty());
    }

    #[test]
    fn active_hibernated_and_uncreated_services_are_skipped() {
        let t0 = Instant::now();
        let state = state_with(&["svc-a", "svc-b"], t0);
        *state.active_view.lock().unwrap() = Some("svc-a".to_string());
        state.hibernated.lock().unwrap().insert("svc-b".to_string());
        let later = t0 + IDLE_BEFORE_HIBERNATE * 2;
        let picked = filter_idle_labels(&state, labels(&["svc-a", "svc-b", "svc-c"]), later);
        assert!(picked.is_empty());
    }
}
