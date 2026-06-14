// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Heitor Faria

//! BigBox — Tauri v2 entry point. App-composition crate: the only place that
//! wires every layer together (Builder, IPC handler registration, window/GTK
//! setup). Domain logic lives in the inner crates; the GTK overlay layout lives
//! in `bigbox-shell`.

use bigbox_shell as commands;
use bigbox_vorcaro as vorcaro;

use commands::AppState;
use tauri::{Emitter, Manager};
use vorcaro::orchestrator::OrchestratorState;
use vorcaro::VorcaroStore;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState::default())
        .manage(VorcaroStore::default())
        .manage(OrchestratorState::default())
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
            commands::zoom_service,
            commands::expand_shell,
            commands::collapse_shell,
            commands::open_url,
            commands::open_about,
            commands::update_badge,
            commands::clear_badge,
            commands::show_service_menu,
            commands::bb_log,
            vorcaro::vorcaro_get_state,
            vorcaro::vorcaro_save_contact,
            vorcaro::vorcaro_delete_contact,
            vorcaro::vorcaro_import_csv,
            vorcaro::vorcaro_save_list,
            vorcaro::vorcaro_delete_list,
            vorcaro::vorcaro_apply_tag,
            vorcaro::vorcaro_remove_tag,
            vorcaro::vorcaro_rename_tag,
            vorcaro::vorcaro_update_settings,
            vorcaro::vorcaro_add_contact_to_list,
            vorcaro::vorcaro_remove_contact_from_list,
            vorcaro::vorcaro_scrape_chats,
            vorcaro::vorcaro_scrape_result,
            vorcaro::vorcaro_import_scraped,
            vorcaro::vorcaro_preview_campaign,
            vorcaro::vorcaro_start_campaign,
            vorcaro::vorcaro_pause_campaign,
            vorcaro::vorcaro_resume_campaign,
            vorcaro::vorcaro_abort_campaign,
            vorcaro::vorcaro_send_result,
            vorcaro::vorcaro_stage_attachment,
            vorcaro::vorcaro_get_cloud_config,
            vorcaro::vorcaro_save_cloud_config,
            vorcaro::vorcaro_verify_cloud_connection,
            vorcaro::vorcaro_list_cloud_templates,
            vorcaro::vorcaro_list_workspaces,
            vorcaro::vorcaro_scrape_workspace,
            vorcaro::vorcaro_list_wa_labels,
            vorcaro::vorcaro_wa_labels_result,
            vorcaro::vorcaro_debug_chat_pane,
            vorcaro::vorcaro_debug_dom_result,
            vorcaro::vorcaro_scrape_progress,
        ])
        .setup(|app| {
            // Handle context-menu click events emitted by show_service_menu
            app.on_menu_event(|app, event| {
                let eid = event.id().as_ref();
                if let Some(svc_id) = eid.strip_prefix("mark-read-") {
                    let label = format!("svc-{}", svc_id);
                    let state: tauri::State<'_, commands::AppState> = app.state();
                    {
                        let mut badges = state.badges.lock().unwrap();
                        badges.insert(label.clone(), 0);
                        let has_any = badges.values().any(|&v| v > 0);
                        commands::refresh_tray_icon(app, has_any);
                    }
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
                    return;
                }
            });
            // Respawn orchestrators for campaigns that were Scheduled / Running
            // when the app last quit. Idempotent; safe to call once at boot.
            vorcaro::rehydrate_on_boot(app.handle().clone());

            #[cfg(target_os = "linux")]
            commands::setup_gtk_layout(app)?;

            // Keep service views aligned to the content area as the main window
            // moves/resizes. Linux: re-bound the in-window child webviews on
            // resize. Windows: re-place the borderless per-service windows on
            // both move and resize (this handler runs on the UI thread, so the
            // window ops apply).
            {
                let window = app.get_window("main").ok_or("main window missing")?;
                let app_h = app.handle().clone();
                window.on_window_event(move |event| {
                    let track = matches!(
                        event,
                        tauri::WindowEvent::Resized(_) | tauri::WindowEvent::Moved(_)
                    );
                    if !track {
                        return;
                    }
                    #[cfg(target_os = "windows")]
                    commands::reposition_service_windows(&app_h);
                    #[cfg(not(target_os = "windows"))]
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

            // Windows: WebView2 controllers only initialize/paint when their
            // webview is created at boot on the UI thread (runtime creation from
            // a command leaves the controller 0x0 / gray). So pre-create every
            // configured service window here, hidden; open_service then just
            // shows/raises the right one.
            #[cfg(target_os = "windows")]
            commands::precreate_service_windows(app.handle());

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running bigbox");
}
