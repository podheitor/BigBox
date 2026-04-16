// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Heitor Faria

//! Tauri IPC commands — config CRUD + embedded service WebView management

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use tauri::menu::{Menu, MenuItemBuilder, PredefinedMenuItem};
use tauri::{AppHandle, Emitter, Manager, State, WebviewBuilder, WebviewUrl};
use tauri::{LogicalPosition, LogicalSize};

use crate::config::{self, AppConfig, UserService};
use crate::services::{self, ServiceDef};

// ── Shared state ─────────────────────────────────────────────────

#[derive(Default)]
pub struct AppState {
    /// Labels of all service WebViews that have been created
    pub created_views: Mutex<HashSet<String>>,
    /// Label of the currently visible service WebView (if any)
    pub active_view:   Mutex<Option<String>>,
    /// Unread badge counts keyed by WebView label (e.g. "svc-discord")
    pub badges:        Mutex<HashMap<String, u32>>,
}

// ── Config commands ──────────────────────────────────────────────

#[tauri::command]
pub fn get_config() -> AppConfig {
    config::load()
}

#[tauri::command]
pub fn get_catalog() -> Vec<ServiceDef> {
    services::load_catalog()
}

#[tauri::command]
pub fn add_service(service_type: String, display_name: String) -> AppConfig {
    let mut cfg = config::load();

    let count = cfg.services.iter()
        .filter(|s| s.service_type == service_type)
        .count();

    let id = if count == 0 {
        service_type.clone()
    } else {
        format!("{service_type}_{}", count + 1)
    };

    cfg.services.push(UserService { id, service_type, display_name, enabled: true });
    config::save(&cfg);
    cfg
}

#[tauri::command]
pub fn remove_service(
    app:   AppHandle,
    state: State<'_, AppState>,
    id:    String,
) -> AppConfig {
    let label = svc_label(&id);

    if let Some(wv) = app.get_webview(&label) {
        let _ = wv.close();
    }

    state.created_views.lock().unwrap().remove(&label);

    let mut active = state.active_view.lock().unwrap();
    if active.as_deref() == Some(&label) {
        *active = None;
    }
    drop(active);

    let mut cfg = config::load();
    cfg.services.retain(|s| s.id != id);
    config::save(&cfg);
    cfg
}

#[tauri::command]
pub fn reorder_services(ids: Vec<String>) -> AppConfig {
    let mut cfg = config::load();

    let mut ordered: Vec<UserService> = ids.iter()
        .filter_map(|id| cfg.services.iter().find(|s| &s.id == id).cloned())
        .collect();

    for svc in &cfg.services {
        if !ordered.iter().any(|s| s.id == svc.id) {
            ordered.push(svc.clone());
        }
    }

    cfg.services = ordered;
    config::save(&cfg);
    cfg
}


/// JS injected into every service WebView.
/// Watches document.title for "(N)" / "[N]" prefix and calls update_badge.
const BADGE_MONITOR_SCRIPT: &str = r#"
(function(){
  const label = '__BADGE_LABEL__';
  function parse(t){
    var m = t.match(/^\((\d+)\)/) || t.match(/^\[(\d+)\]/) || t.match(/^(\d+) /);
    return m ? parseInt(m[1], 10) : 0;
  }
  var last = -1;
  function check(){
    var c = parse(document.title);
    if(c !== last){
      last = c;
      try{
        window.__TAURI__.core.invoke('update_badge', {label: label, count: c});
      }catch(_){
        try{
          window.__TAURI_INTERNALS__.invoke('update_badge', {label: label, count: c});
        }catch(__){ }
      }
    }
  }
  new MutationObserver(check)
    .observe(document.documentElement, {subtree:true, childList:true, characterData:true});
  setInterval(check, 3000);
  check();
})();
"#;

/// Override Notification.permission so WebKitGTK reports "granted" on page load.
/// WebKitGTK does not persist Notification API permission across sessions.
const NOTIFICATION_GRANT_SCRIPT: &str = r#"
(function(){
  if(typeof Notification !== 'undefined'){
    Object.defineProperty(Notification, 'permission', {
      get: function(){ return 'granted'; },
      configurable: true
    });
    var origReq = Notification.requestPermission;
    Notification.requestPermission = function(cb){
      if(cb) cb('granted');
      return Promise.resolve('granted');
    };
  }
})();
"#;


// ── WebView commands ─────────────────────────────────────────────

/// Open (or show) the embedded WebView for a service.
/// On Linux, collapses shell to 64px sidebar so service fills remaining width.
/// MUST be sync (not async) — GTK calls require the main thread.
#[tauri::command]
/// Apply correct position+size to a service WebView on Linux.
/// Service views must be offset by sidebar+titlebar to not block shell input.
#[cfg(target_os = "linux")]
pub fn apply_svc_bounds(app: &AppHandle, wv: &tauri::Webview<tauri::Wry>) {
    use crate::{SIDEBAR_W, TITLEBAR_H};
    let Some(win) = app.get_window("main") else { return };
    let scale = win.scale_factor().unwrap_or(1.0);
    let phys  = win.inner_size().unwrap_or_default();
    let lw = phys.width  as f64 / scale;
    let lh = phys.height as f64 / scale;
    let x  = SIDEBAR_W as f64;
    let y  = TITLEBAR_H as f64;
    let _  = wv.set_bounds(tauri::Rect {
        position: tauri::Position::Logical(tauri::LogicalPosition::new(x, y)),
        size:     tauri::Size::Logical(tauri::LogicalSize::new(
            (lw - x).max(1.0),
            (lh - y).max(1.0),
        )),
    });
}

#[tauri::command]
pub fn open_service(
    app:        AppHandle,
    state:      State<'_, AppState>,
    service_id: String,
    url:        String,
    user_agent: Option<String>,
) -> Result<(), String> {
    let label  = svc_label(&service_id);

    // Collapse shell to sidebar-only (Linux GtkBox packing)
    #[cfg(target_os = "linux")]
    crate::collapse_shell_impl(&app);

    // Hide all visible service WebViews
    {
        let created = state.created_views.lock().unwrap();
        for lbl in created.iter() {
            if let Some(wv) = app.get_webview(lbl) {
                let _ = wv.hide();
            }
        }
    }

    ensure_service_webview_created(
        &app,
        &state,
        &service_id,
        &label,
        &url,
        user_agent.as_deref(),
    )?;

    if let Some(wv) = app.get_webview(&label) {
        wv.show().map_err(|e| e.to_string())?;
        #[cfg(target_os = "linux")]
        apply_svc_bounds(&app, &wv);
    }

    *state.active_view.lock().unwrap() = Some(label);
    Ok(())
}

/// Pre-create a service WebView in background to speed up first open.
#[tauri::command]
pub fn preload_service(
    app:        AppHandle,
    state:      State<'_, AppState>,
    service_id: String,
    url:        String,
    user_agent: Option<String>,
) -> Result<(), String> {
    let label = svc_label(&service_id);

    ensure_service_webview_created(
        &app,
        &state,
        &service_id,
        &label,
        &url,
        user_agent.as_deref(),
    )?;

    if let Some(wv) = app.get_webview(&label) {
        let _ = wv.hide();
        #[cfg(target_os = "linux")]
        apply_svc_bounds(&app, &wv);
    }

    Ok(())
}


/// Auto-grant notification/media permissions for service WebViews.
/// Desktop messaging aggregator = user explicitly added the service.
#[cfg(target_os = "linux")]
fn setup_webview_permissions(wv: &tauri::Webview) {
    let _ = wv.with_webview(|platform_wv| {
        use webkit2gtk::{PermissionRequestExt, WebViewExt, PermissionRequest};
        let inner = platform_wv.inner();
        inner.connect_permission_request(|_wv, request: &PermissionRequest| {
            request.allow();
            true
        });
    });
}

#[cfg(not(target_os = "linux"))]
fn setup_webview_permissions(_wv: &tauri::Webview) {}

fn ensure_service_webview_created(
    app: &AppHandle,
    state: &State<'_, AppState>,
    service_id: &str,
    label: &str,
    url: &str,
    user_agent: Option<&str>,
) -> Result<(), String> {
    if state.created_views.lock().unwrap().contains(label) {
        return Ok(());
    }

    if app.get_webview(label).is_some() {
        state.created_views.lock().unwrap().insert(label.to_string());
        return Ok(());
    }

    let window = app.get_window("main").ok_or("main window missing")?;
    let session_dir = services::session_dir(service_id);
    std::fs::create_dir_all(&session_dir).ok();

    let parsed_url: tauri::Url = url.parse().map_err(|e| format!("{e}"))?;
    let badge_script = BADGE_MONITOR_SCRIPT.replace("__BADGE_LABEL__", label);
    let mut builder = WebviewBuilder::new(label, WebviewUrl::External(parsed_url))
        .data_directory(session_dir)
        .initialization_script(&badge_script)
        .initialization_script(NOTIFICATION_GRANT_SCRIPT);

    if let Some(ua) = user_agent {
        builder = builder.user_agent(ua);
    }

    // Initial pos/size — corrected by apply_svc_bounds after creation
    match window.add_child(
        builder,
        LogicalPosition::new(0.0, 0.0),
        LogicalSize::new(100.0, 100.0),
    ) {
        Ok(wv) => {
            state.created_views.lock().unwrap().insert(label.to_string());
              setup_webview_permissions(&wv);
            Ok(())
        }
        Err(e) => {
            if app.get_webview(label).is_some() {
                state.created_views.lock().unwrap().insert(label.to_string());
                Ok(())
            } else {
                Err(e.to_string())
            }
        }
    }
}

/// Hide the currently visible service WebView
#[tauri::command]
pub fn hide_service(
    app:   AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    if let Some(label) = state.active_view.lock().unwrap().as_ref() {
        if let Some(wv) = app.get_webview(label) {
            wv.hide().map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// Reload a service WebView (refreshes the page content)
#[tauri::command]
pub fn reload_service(
    app:   AppHandle,
    _state: State<'_, AppState>,
    id:    String,
) -> Result<(), String> {
    let label = svc_label(&id);
    if let Some(wv) = app.get_webview(&label) {
        wv.eval("window.location.reload()").map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Show a native GTK context menu for a service item.
/// On Linux/GTK, native menus always render above WebViews.
#[tauri::command]
pub fn show_service_menu(
    app:    AppHandle,
    _state: State<'_, AppState>,
    id:     String,
    x:      f64,
    y:      f64,
) -> Result<(), String> {
    let window = app.get_window("main").ok_or("main window missing")?;
    let mark_read = MenuItemBuilder::with_id(format!("mark-read-{id}"), "Mark all as read")
        .build(&app)
        .map_err(|e| e.to_string())?;
    let reload = MenuItemBuilder::with_id(format!("reload-{id}"), "Reload")
        .build(&app)
        .map_err(|e| e.to_string())?;
    let separator = PredefinedMenuItem::separator(&app).map_err(|e| e.to_string())?;
    let remove = MenuItemBuilder::with_id(format!("remove-{id}"), "Remove")
        .build(&app)
        .map_err(|e| e.to_string())?;
    let menu = Menu::with_items(&app, &[&mark_read, &reload, &separator, &remove])
        .map_err(|e| e.to_string())?;

    window
        .popup_menu_at(&menu, LogicalPosition::new(x, y))
        .map_err(|e| e.to_string())
}

/// Mute/unmute all media in every open service WebView
#[tauri::command]
pub fn set_muted(
    app:   AppHandle,
    state: State<'_, AppState>,
    muted: bool,
) -> Result<(), String> {
    let script = if muted {
        // mute existing + observe future elements
        r#"(function(){
          document.querySelectorAll('video,audio').forEach(m=>m.muted=true);
          if(window.__bigboxMuteObs){window.__bigboxMuteObs.disconnect();}
          window.__bigboxMuteObs=new MutationObserver(function(ml){
            ml.forEach(function(m){m.addedNodes.forEach(function(n){
              if(n.nodeType===1){
                if(n.matches&&n.matches('video,audio'))n.muted=true;
                n.querySelectorAll&&n.querySelectorAll('video,audio').forEach(function(e){e.muted=true;});
              }
            });});
          });
          window.__bigboxMuteObs.observe(document.body||document.documentElement,{childList:true,subtree:true});
        })();"#
    } else {
        // unmute existing + stop observer
        r#"(function(){
          document.querySelectorAll('video,audio').forEach(m=>m.muted=false);
          if(window.__bigboxMuteObs){window.__bigboxMuteObs.disconnect();window.__bigboxMuteObs=null;}
        })();"#
    };

    let created = state.created_views.lock().unwrap();
    for label in created.iter() {
        if let Some(wv) = app.get_webview(label) {
            let _ = wv.eval(script);
        }
    }
    drop(created);

    let mut cfg = config::load();
    cfg.muted = muted;
    config::save(&cfg);
    Ok(())
}

/// Expand shell to full width (for welcome screen / dialogs).
/// Call hide_service first if a service is visible.
#[tauri::command]
#[allow(unused_variables)]
pub fn expand_shell(app: AppHandle) {
    #[cfg(target_os = "linux")]
    crate::expand_shell_impl(&app);
}

/// Collapse shell to 64px sidebar (when service is active).
#[tauri::command]
#[allow(unused_variables)]
pub fn collapse_shell(app: AppHandle) {
    #[cfg(target_os = "linux")]
    crate::collapse_shell_impl(&app);
}




/// Called by service WebViews when document.title changes (badge monitoring script).
/// Stores the count and emits a "badge-update" event to the main shell.
#[tauri::command]
pub fn update_badge(
    app:   AppHandle,
    state: State<'_, AppState>,
    label: String,
    count: u32,
) -> Result<(), String> {
    state.badges.lock().unwrap().insert(label.clone(), count);
    app.emit("badge-update", serde_json::json!({ "label": label, "count": count }))
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Clear badge count for a specific service (called from context menu "Mark all as read")
#[tauri::command]
pub fn clear_badge(
    app:   AppHandle,
    state: State<'_, AppState>,
    label: String,
) -> Result<(), String> {
    // Clear stored badge
    state.badges.lock().unwrap().insert(label.clone(), 0);
    // Emit reset to shell
    app.emit("reset-badge", serde_json::json!({ "label": label }))
        .map_err(|e| e.to_string())?;
    Ok(())
}


/// Open (or focus) the About window as a small standalone WebviewWindow.
#[tauri::command]
pub fn open_about(app: AppHandle) -> Result<(), String> {
    const LABEL: &str = "about";
    if let Some(win) = app.get_window(LABEL) {
        win.set_focus().map_err(|e| e.to_string())?;
        return Ok(());
    }
    tauri::WebviewWindowBuilder::new(
        &app,
        LABEL,
        tauri::WebviewUrl::App("about.html".into()),
    )
    .title("Sobre o BigBox")
    .inner_size(360.0, 420.0)
    .min_inner_size(300.0, 380.0)
    .max_inner_size(440.0, 500.0)
    .resizable(false)
    .decorations(false)
    .center()
    .build()
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Open a URL in the system default browser (xdg-open on Linux).
#[tauri::command]
pub fn open_url(url: String) -> Result<(), String> {
    // Security: only allow http/https URLs
    if !url.starts_with("https://") && !url.starts_with("http://") {
        return Err("Only http/https URLs are allowed".into());
    }

    std::process::Command::new("xdg-open")
        .arg(&url)
        .spawn()
        .map_err(|e| format!("Failed to open URL: {e}"))?;
    Ok(())
}

// ── Helpers ──────────────────────────────────────────────────────

fn svc_label(service_id: &str) -> String {
    format!("svc-{service_id}")
}
