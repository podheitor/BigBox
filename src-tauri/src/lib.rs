// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Heitor Faria

//! BigBox — Tauri v2 entry point

pub mod commands;
pub mod config;
pub mod services;

use commands::AppState;
use tauri::{Emitter, Manager};

/// Titlebar height (must match CSS `#titlebar { height }`)
pub const TITLEBAR_H: i32 = 30;
/// Sidebar width (must match CSS `--sidebar-w`)
pub const SIDEBAR_W: i32 = 64;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            commands::get_config,
            commands::get_catalog,
            commands::add_service,
            commands::remove_service,
            commands::reorder_services,
            commands::open_service,
            commands::preload_service,
            commands::hide_service,
            commands::reload_service,
            commands::set_muted,
            commands::expand_shell,
            commands::collapse_shell,
            commands::open_url,
            commands::open_about,
            commands::update_badge,
            commands::clear_badge,
            commands::show_service_menu,
        ])
        .setup(|app| {
            // Handle context-menu click events emitted by show_service_menu
            app.on_menu_event(|app, event| {
                let eid = event.id().as_ref();
                if let Some(svc_id) = eid.strip_prefix("mark-read-") {
                    let label = format!("svc-{}", svc_id);
                    let state: tauri::State<'_, commands::AppState> = app.state();
                    state.badges.lock().unwrap().insert(label.clone(), 0);
                    let _ = app.emit("reset-badge", serde_json::json!({ "label": label }));
                    return;
                }

                if let Some(svc_id) = eid.strip_prefix("reload-") {
                    let label = format!("svc-{}", svc_id);
                    if let Some(wv) = app.get_webview(&label) {
                        let _ = wv.eval("window.location.reload()");
                    }
                    return;
                }

                if let Some(svc_id) = eid.strip_prefix("remove-") {
                    let state: tauri::State<'_, commands::AppState> = app.state();
                    let _ = commands::remove_service(app.clone(), state, svc_id.to_string());
                    let _ = app.emit("service-removed", serde_json::json!({ "id": svc_id }));
                }
            });
            #[cfg(target_os = "linux")]
            setup_gtk_layout(app)?;

            // Reposition all service WebViews when window is resized
            #[cfg(target_os = "linux")]
            {
                let window = app.get_window("main").ok_or("main window missing")?;
                let app_h = app.handle().clone();
                window.on_window_event(move |event| {
                    if let tauri::WindowEvent::Resized(_) = event {
                        let state: tauri::State<'_, commands::AppState> = app_h.state();
                        let views: Vec<String> = state.created_views.lock().unwrap().iter().cloned().collect();
                        for lbl in views {
                            if let Some(wv) = app_h.get_webview(&lbl) {
                                commands::apply_svc_bounds(&app_h, &wv);
                            }
                        }
                    }
                });
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running bigbox");
}

#[cfg(target_os = "linux")]
use std::cell::RefCell;

#[cfg(target_os = "linux")]
thread_local! {
    static CACHED_VBOX: RefCell<Option<gtk::Box>> = const { RefCell::new(None) };
}

/// Setup GTK horizontal box layout with overlay-style service view positioning.
///
/// Layout strategy:
///   child[0] (shell WebView) → always full window (titlebar + sidebar visible)
///   child[1..] (service views) → overlaid at (SIDEBAR_W, TITLEBAR_H) offset
///
/// WebKitWebView natural size is huge → GtkBox packing cannot shrink it.
/// Only size_allocate override on the GtkBox reliably sets bounds on Wayland.
#[cfg(target_os = "linux")]
fn setup_gtk_layout(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    use gtk::prelude::*;

    let window = app.get_webview_window("main").ok_or("main window not found")?;
    let vbox: gtk::Box = window.default_vbox()?;
    vbox.set_orientation(gtk::Orientation::Horizontal);
    vbox.set_spacing(0);

    // GTK CSS: dark background matching app theme → fixes edge strips
    let css = gtk::CssProvider::new();
    css.load_from_data(b"window, box { background-color: transparent; }").ok();
    if let Some(screen) = gtk::gdk::Screen::default() {
        gtk::StyleContext::add_provider_for_screen(
            &screen, &css, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }

    // Override allocations: shell always full, services overlaid at content offset.
    vbox.connect_size_allocate(|bx, alloc| {
        let children = bx.children();
        if children.is_empty() { return; }

        let x0 = alloc.x();
        let y0 = alloc.y();
        let w  = alloc.width();
        let h  = alloc.height();

        // Shell: full window (titlebar + sidebar always visible)
        children[0].size_allocate(&gtk::Allocation::new(x0, y0, w, h));

        if children.len() < 2 { return; }

        // Services: overlaid on content area (skips sidebar width + titlebar height)
        let svc_x = x0 + SIDEBAR_W;
        let svc_y = y0 + TITLEBAR_H;
        let svc_w = (w - SIDEBAR_W).max(1);
        let svc_h = (h - TITLEBAR_H).max(1);
        for child in &children[1..] {
            child.size_allocate(&gtk::Allocation::new(svc_x, svc_y, svc_w, svc_h));
        }
    });

    CACHED_VBOX.with(|cell| *cell.borrow_mut() = Some(vbox));
    Ok(())
}

#[cfg(target_os = "linux")]
pub fn with_vbox(f: impl FnOnce(&gtk::Box)) {
    CACHED_VBOX.with(|cell| {
        if let Some(ref vbox) = *cell.borrow() {
            f(vbox);
        }
    });
}

/// Shell is always full size now — only trigger resize for GTK to re-apply allocations.
#[cfg(target_os = "linux")]
pub fn collapse_shell_impl(_app: &tauri::AppHandle) {
    use gtk::prelude::*;
    with_vbox(|vbox| vbox.queue_resize());
}

/// Same as collapse — shell stays full size, resize ensures correct child allocations.
#[cfg(target_os = "linux")]
pub fn expand_shell_impl(_app: &tauri::AppHandle) {
    use gtk::prelude::*;
    with_vbox(|vbox| vbox.queue_resize());
}
