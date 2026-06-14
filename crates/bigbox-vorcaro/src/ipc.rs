// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Heitor Faria

//! Vorcaro's Studio IPC commands + the engine port adapters. The crate-root
//! submodules (`model`, `orchestrator`, `cloud_api`, …) are declared in
//! `lib.rs`; this module references them through `crate::`.

use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Emitter, Manager};
use uuid::Uuid;

use crate::{attachments, cloud_api, csv_io, orchestrator, store};
use crate::model::{
    Campaign, CampaignStatus, Contact, ContactList, ContactSource, Platform, ScrapedChat,
    SendStatus, Settings, TargetSpec, TemplateUsage, VorcaroState,
};
use crate::orchestrator::OrchestratorState;
use crate::cloud_api::{TemplateInfo, WhatsAppCloudConfig};

/// In-memory cache of the persisted state. Cloneable via `inner_clone()` so the
/// orchestrator can hold a long-lived handle without going through Tauri state.
#[derive(Default)]
pub struct VorcaroStore {
    inner: Arc<Mutex<Option<VorcaroState>>>,
}

impl VorcaroStore {
    fn with<R>(&self, f: impl FnOnce(&mut VorcaroState) -> R) -> R {
        let mut guard = self.inner.lock().unwrap();
        if guard.is_none() {
            *guard = Some(store::load());
        }
        f(guard.as_mut().unwrap())
    }

    fn snapshot(&self) -> VorcaroState {
        let mut guard = self.inner.lock().unwrap();
        if guard.is_none() {
            *guard = Some(store::load());
        }
        guard.as_ref().unwrap().clone()
    }

    /// Hand the inner Arc to the orchestrator (so the task can outlive the
    /// IPC handler that spawned it).
    pub fn inner_clone(&self) -> Arc<Mutex<Option<VorcaroState>>> {
        self.inner.clone()
    }
}

fn persist(state: &VorcaroState) -> Result<(), String> {
    store::save(state)
}

/// Same as `persist`, but takes the locked-inner Arc directly. Used by the
/// orchestrator, which doesn't hold a `&VorcaroStore`.
pub fn persist_state(
    inner: &Arc<Mutex<Option<VorcaroState>>>,
) -> Result<(), String> {
    let guard = inner.lock().unwrap();
    if let Some(s) = guard.as_ref() {
        store::save(s)
    } else {
        Ok(())
    }
}

// ── Port adapters: bridge the Tauri-free engine to the live AppHandle ─────
// These are the concrete implementations of the `bigbox-contract` ports. The
// engine (bigbox-orchestrator) holds them as `Arc<dyn …>` and never names tauri.

struct TauriTransport {
    app: AppHandle,
}

impl bigbox_contract::DriverTransport for TauriTransport {
    fn webview_exists(&self, label: &str) -> bool {
        self.app.get_webview(label).is_some()
    }

    fn eval(&self, label: &str, js: &str) -> Result<(), String> {
        match self.app.get_webview(label) {
            Some(wv) => wv.eval(js).map_err(|e| e.to_string()),
            None => Err(format!("'{label}' não está aberto")),
        }
    }
}

struct TauriProgress {
    app: AppHandle,
}

impl bigbox_contract::ProgressSink for TauriProgress {
    fn emit(&self, campaign_id: Uuid, kind: &str, payload: serde_json::Value) {
        let _ = self.app.emit(
            "vorcaro://campaign-progress",
            serde_json::json!({
                "campaign_id": campaign_id.to_string(),
                "kind": kind,
                "payload": payload,
            }),
        );
    }
}

/// Build the engine ports over a live `AppHandle`.
fn make_ports(
    app: &AppHandle,
) -> (
    Arc<dyn bigbox_contract::DriverTransport>,
    Arc<dyn bigbox_contract::ProgressSink>,
) {
    (
        Arc::new(TauriTransport { app: app.clone() }),
        Arc::new(TauriProgress { app: app.clone() }),
    )
}

// ── IPC commands ────────────────────────────────────────────────

#[tauri::command]
pub fn vorcaro_get_state(state: tauri::State<'_, VorcaroStore>) -> VorcaroState {
    state.snapshot()
}

#[tauri::command]
pub fn vorcaro_save_contact(
    state: tauri::State<'_, VorcaroStore>,
    contact: Contact,
) -> Result<Contact, String> {
    state.with(|s| {
        let mut c = contact;
        if c.id.is_nil() { c.id = Uuid::new_v4(); }
        // Normalize handles a bit so dedup works regardless of how the user typed them.
        if let Some(wa) = c.whatsapp.as_deref() { c.whatsapp = Some(strip_phone(wa)); }
        if let Some(wab) = c.whatsapp_business.as_deref() { c.whatsapp_business = Some(strip_phone(wab)); }
        if let Some(tg) = c.telegram.as_deref() { c.telegram = Some(strip_telegram(tg)); }
        if c.display_name.trim().is_empty() {
            c.display_name = c.whatsapp.clone()
                .or_else(|| c.telegram.clone())
                .unwrap_or_else(|| "(unnamed)".into());
        }
        if let Some(pos) = s.contacts.iter().position(|x| x.id == c.id) {
            s.contacts[pos] = c.clone();
        } else {
            s.contacts.push(c.clone());
        }
        persist(s)?;
        Ok(c)
    })
}

#[tauri::command]
pub fn vorcaro_delete_contact(
    state: tauri::State<'_, VorcaroStore>,
    id: Uuid,
) -> Result<(), String> {
    state.with(|s| {
        s.contacts.retain(|c| c.id != id);
        for l in s.lists.iter_mut() {
            l.contact_ids.retain(|cid| *cid != id);
        }
        persist(s)
    })
}

#[tauri::command]
pub fn vorcaro_import_csv(
    state: tauri::State<'_, VorcaroStore>,
    content: String,
) -> Result<csv_io::ImportReportSerde, String> {
    state.with(|s| {
        let report = csv_io::import_csv(content.as_bytes(), &mut s.contacts)?;
        persist(s)?;
        Ok(report)
    })
}

#[tauri::command]
pub fn vorcaro_save_list(
    state: tauri::State<'_, VorcaroStore>,
    list: ContactList,
) -> Result<ContactList, String> {
    state.with(|s| {
        let mut l = list;
        if l.id.is_nil() { l.id = Uuid::new_v4(); }
        if l.name.trim().is_empty() {
            return Err("list name cannot be empty".to_string());
        }
        if let Some(pos) = s.lists.iter().position(|x| x.id == l.id) {
            s.lists[pos] = l.clone();
        } else {
            s.lists.push(l.clone());
        }
        persist(s)?;
        Ok(l)
    })
}

#[tauri::command]
pub fn vorcaro_delete_list(
    state: tauri::State<'_, VorcaroStore>,
    id: Uuid,
) -> Result<(), String> {
    state.with(|s| {
        s.lists.retain(|l| l.id != id);
        persist(s)
    })
}

#[tauri::command]
pub fn vorcaro_apply_tag(
    state: tauri::State<'_, VorcaroStore>,
    contact_ids: Vec<Uuid>,
    tag: String,
) -> Result<(), String> {
    let tag = tag.trim().to_string();
    if tag.is_empty() { return Err("tag cannot be empty".into()); }
    state.with(|s| {
        for c in s.contacts.iter_mut() {
            if contact_ids.contains(&c.id) && !c.tags.contains(&tag) {
                c.tags.push(tag.clone());
            }
        }
        persist(s)
    })
}

#[tauri::command]
pub fn vorcaro_remove_tag(
    state: tauri::State<'_, VorcaroStore>,
    contact_ids: Vec<Uuid>,
    tag: String,
) -> Result<(), String> {
    state.with(|s| {
        for c in s.contacts.iter_mut() {
            if contact_ids.contains(&c.id) {
                c.tags.retain(|t| t != &tag);
            }
        }
        persist(s)
    })
}

#[tauri::command]
pub fn vorcaro_rename_tag(
    state: tauri::State<'_, VorcaroStore>,
    old: String,
    new: String,
) -> Result<(), String> {
    let new = new.trim().to_string();
    if new.is_empty() { return Err("new tag cannot be empty".into()); }
    state.with(|s| {
        for c in s.contacts.iter_mut() {
            for t in c.tags.iter_mut() {
                if *t == old { *t = new.clone(); }
            }
            // dedup after rename
            let mut seen = std::collections::HashSet::new();
            c.tags.retain(|t| seen.insert(t.clone()));
        }
        persist(s)
    })
}

#[tauri::command]
pub fn vorcaro_update_settings(
    state: tauri::State<'_, VorcaroStore>,
    settings: Settings,
) -> Result<Settings, String> {
    state.with(|s| {
        s.settings = settings.clone();
        persist(s)?;
        Ok(settings)
    })
}

#[tauri::command]
pub fn vorcaro_add_contact_to_list(
    state: tauri::State<'_, VorcaroStore>,
    list_id: Uuid,
    contact_id: Uuid,
) -> Result<(), String> {
    state.with(|s| {
        let Some(list) = s.lists.iter_mut().find(|l| l.id == list_id) else {
            return Err("list not found".into());
        };
        if !list.contact_ids.contains(&contact_id) {
            list.contact_ids.push(contact_id);
        }
        persist(s)
    })
}

#[tauri::command]
pub fn vorcaro_remove_contact_from_list(
    state: tauri::State<'_, VorcaroStore>,
    list_id: Uuid,
    contact_id: Uuid,
) -> Result<(), String> {
    state.with(|s| {
        let Some(list) = s.lists.iter_mut().find(|l| l.id == list_id) else {
            return Err("list not found".into());
        };
        list.contact_ids.retain(|c| *c != contact_id);
        persist(s)
    })
}

// ── Phase B: scraping ───────────────────────────────────────────

/// Trigger a scrape on the matching chat-service WebView. Fire-and-forget:
/// the driver eventually calls `vorcaro_scrape_result`, which re-emits an
/// event the studio panel listens for.
#[tauri::command]
pub fn vorcaro_scrape_chats(app: AppHandle, platform: Platform) -> Result<(), String> {
    // Cloud API has no WebView — there's nothing to scrape.
    if matches!(platform, Platform::WhatsAppCloudApi) {
        return Err("Cloud API não tem WebView; raspe via WhatsApp Web ou Business.".into());
    }
    let service_id = platform.service_id();
    let label = format!("svc-{service_id}");
    let Some(wv) = app.get_webview(&label) else {
        return Err(format!(
            "{} não está aberto. Adicione e abra o serviço no BigBox primeiro.",
            service_id
        ));
    };
    let platform_tag = match platform {
        Platform::WhatsAppWeb => "whatsapp_web",
        Platform::WhatsAppBusinessWeb => "whatsapp_business_web",
        Platform::Telegram => "telegram",
        Platform::WhatsAppCloudApi => unreachable!(), // gated above
    };
    let js = format!(
        "(window.__vorcaro && window.__vorcaro.scrapeChats)
            ? window.__vorcaro.scrapeChats('{platform_tag}')
            : (window.__TAURI__.core.invoke('vorcaro_scrape_result', \
                {{ platform: '{platform_tag}', rows: [], error: 'driver não carregado — recarregue a aba' }}));"
    );
    wv.eval(&js).map_err(|e| e.to_string())
}

/// Receives scraped rows from the driver inside the chat WebView and re-emits
/// them as a `vorcaro://scrape-result` event for the studio panel.
#[tauri::command]
pub fn vorcaro_scrape_result(
    app: AppHandle,
    platform: String,
    rows: Vec<ScrapedChat>,
    error: Option<String>,
) -> Result<(), String> {
    let payload = serde_json::json!({
        "platform": platform,
        "rows": rows,
        "error": error,
    });
    app.emit("vorcaro://scrape-result", payload)
        .map_err(|e| e.to_string())
}

/// Driver progress beacon during the slow click-each-chat phone extraction.
#[tauri::command]
pub fn vorcaro_scrape_progress(
    app: AppHandle,
    current: u32,
    total: u32,
) -> Result<(), String> {
    app.emit("vorcaro://scrape-progress", serde_json::json!({
        "current": current, "total": total,
    })).map_err(|e| e.to_string())
}

// ── Phase C: campaigns ──────────────────────────────────────────

#[derive(serde::Serialize)]
pub struct CampaignPreview {
    pub recipient_count: usize,
    pub recipients_with_handle: usize,
    pub recipients_missing_handle: usize,
    pub warn: bool,
    pub daily_cap_remaining: u32,
}

/// Resolve a target spec to a recipient set and surface counts + warnings.
#[tauri::command]
pub fn vorcaro_preview_campaign(
    state: tauri::State<'_, VorcaroStore>,
    targets: TargetSpec,
    platform: Platform,
) -> Result<CampaignPreview, String> {
    state.with(|s| {
        let recipients = orchestrator::resolve_targets(s, &targets);
        let with_handle = recipients
            .iter()
            .filter(|c| match platform {
                Platform::WhatsAppWeb => c.whatsapp.is_some() || c.whatsapp_business.is_some(),
                // Business platform falls back to the personal WhatsApp number
                // when the WA-Business field is empty — most users have the
                // same number under both labels.
                Platform::WhatsAppBusinessWeb => c.whatsapp_business.is_some() || c.whatsapp.is_some(),
                Platform::Telegram => c.telegram.is_some(),
                Platform::WhatsAppCloudApi => c.whatsapp_business.is_some() || c.whatsapp.is_some(),
            })
            .count();
        let cap_used = s.daily_cap.count(platform);
        let cap_total = s.settings.daily_cap_per_platform;
        Ok(CampaignPreview {
            recipient_count: recipients.len(),
            recipients_with_handle: with_handle,
            recipients_missing_handle: recipients.len() - with_handle,
            warn: recipients.len() > s.settings.warn_threshold as usize,
            daily_cap_remaining: cap_total.saturating_sub(cap_used),
        })
    })
}

#[derive(serde::Deserialize)]
pub struct CampaignSpec {
    pub name: String,
    pub body: String,
    pub targets: TargetSpec,
    pub platform: Platform,
    #[serde(default)]
    pub scheduled_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Absolute paths returned earlier by `vorcaro_stage_attachment`.
    #[serde(default)]
    pub attachments: Vec<std::path::PathBuf>,
    /// Cloud-API-only: send a template instead of free-form text.
    #[serde(default)]
    pub template: Option<TemplateUsage>,
    /// Specific BigBox service id to drive. See `Campaign.workspace_id`.
    #[serde(default)]
    pub workspace_id: Option<String>,
}

/// Receive a base64-encoded attachment from the frontend, stage it under the
/// cache dir, and return the absolute path to be stored in `Campaign.attachments`.
#[tauri::command]
pub fn vorcaro_stage_attachment(name: String, b64: String) -> Result<String, String> {
    let path = attachments::stage(&name, &b64)?;
    Ok(path.to_string_lossy().into_owned())
}

// ── Phase G: workspace selection ───────────────────────────────

/// A chat-service workspace the user has added to BigBox. The `id` is the same
/// id used in the WebView label (`svc-<id>`).
#[derive(Debug, Clone, serde::Serialize)]
pub struct Workspace {
    pub id: String,
    pub display_name: String,
    pub platform: Platform,
}

/// Lists every workspace Vorcaro can drive: WhatsApp / WhatsApp Business /
/// Telegram instances currently present in BigBox's config.toml. Pulls from
/// the live `config::load()` snapshot so it stays in sync with the sidebar.
#[tauri::command]
pub fn vorcaro_list_workspaces() -> Vec<Workspace> {
    let cfg = bigbox_config::config::load();
    cfg.services
        .into_iter()
        .filter_map(|svc| {
            let platform = match svc.service_type.as_str() {
                "whatsapp" => Platform::WhatsAppWeb,
                "whatsapp_business" => Platform::WhatsAppBusinessWeb,
                "telegram" => Platform::Telegram,
                _ => return None,
            };
            Some(Workspace {
                id: svc.id,
                display_name: svc.display_name,
                platform,
            })
        })
        .collect()
}

/// Ask the WhatsApp driver to list available filter labels (built-in + custom
/// WA Business labels). Result comes back via the `vorcaro://wa-labels-result`
/// event so the panel can populate its dropdown asynchronously.
#[tauri::command]
pub fn vorcaro_list_wa_labels(app: AppHandle, workspace_id: String) -> Result<(), String> {
    let workspaces = vorcaro_list_workspaces();
    let Some(ws) = workspaces.iter().find(|w| w.id == workspace_id) else {
        return Err(format!("workspace '{workspace_id}' não encontrado"));
    };
    if !matches!(ws.platform, Platform::WhatsAppWeb | Platform::WhatsAppBusinessWeb) {
        return Err("etiquetas só existem no WhatsApp Business".into());
    }
    let label = format!("svc-{}", ws.id);
    let Some(wv) = app.get_webview(&label) else {
        return Err(format!("'{}' não está aberto no BigBox", ws.display_name));
    };
    let js = "(window.__vorcaro && window.__vorcaro.listLabels)
        ? window.__vorcaro.listLabels()
        : window.__TAURI__.core.invoke('vorcaro_wa_labels_result', \
            { labels: [], error: 'driver não carregado — recarregue a aba' });";
    wv.eval(js).map_err(|e| e.to_string())
}

/// Driver inside the WA WebView calls this after listLabels finishes; we
/// re-emit as an event the panel listens for.
#[tauri::command]
pub fn vorcaro_wa_labels_result(
    app: AppHandle,
    labels: Vec<String>,
    error: Option<String>,
) -> Result<(), String> {
    let payload = serde_json::json!({ "labels": labels, "error": error });
    app.emit("vorcaro://wa-labels-result", payload)
        .map_err(|e| e.to_string())
}

/// Diagnostic: ask the WA driver to dump everything clickable near the top of
/// the chat pane. Used to derive correct selectors when auto-detection fails.
#[tauri::command]
pub fn vorcaro_debug_chat_pane(app: AppHandle, workspace_id: String) -> Result<(), String> {
    let workspaces = vorcaro_list_workspaces();
    let Some(ws) = workspaces.iter().find(|w| w.id == workspace_id) else {
        return Err(format!("workspace '{workspace_id}' não encontrado"));
    };
    let label = format!("svc-{}", ws.id);
    let Some(wv) = app.get_webview(&label) else {
        return Err(format!("'{}' não está aberto", ws.display_name));
    };
    let js = "(window.__vorcaro && window.__vorcaro.debugChatPane) \
        ? window.__vorcaro.debugChatPane() \
        : window.__TAURI__.core.invoke('vorcaro_debug_dom_result', \
            { dump: 'driver não carregado' });";
    wv.eval(js).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn vorcaro_debug_dom_result(app: AppHandle, dump: String) -> Result<(), String> {
    app.emit("vorcaro://debug-dom-result", serde_json::json!({ "dump": dump }))
        .map_err(|e| e.to_string())
}

/// Scrape chats from a specific workspace (e.g. `whatsapp_2`). Replaces the
/// older platform-keyed flow for users with multiple WhatsApp slots.
///
/// `label_filter`, when present and non-empty, applies WA Business's built-in
/// "filter by label" UI before scraping. Only the chats matching that label
/// come back. Ignored for Telegram (Telegram has no labels feature).
#[tauri::command]
pub fn vorcaro_scrape_workspace(
    app: AppHandle,
    workspace_id: String,
    label_filter: Option<String>,
) -> Result<(), String> {
    let workspaces = vorcaro_list_workspaces();
    let Some(ws) = workspaces.iter().find(|w| w.id == workspace_id) else {
        return Err(format!("workspace '{workspace_id}' não encontrado em BigBox"));
    };
    let label = format!("svc-{}", ws.id);
    let Some(wv) = app.get_webview(&label) else {
        return Err(format!(
            "'{}' não está aberto no BigBox. Abra a aba ao menos uma vez e faça login.",
            ws.display_name
        ));
    };
    let platform_tag = match ws.platform {
        Platform::WhatsAppWeb => "whatsapp_web",
        Platform::WhatsAppBusinessWeb => "whatsapp_business_web",
        Platform::Telegram => "telegram",
        Platform::WhatsAppCloudApi => return Err("Cloud API não suporta raspagem".into()),
    };
    let label_json = match label_filter.as_deref() {
        Some(s) if !s.trim().is_empty() => serde_json::to_string(&s).unwrap_or_else(|_| "null".into()),
        _ => "null".into(),
    };
    let js = format!(
        "(window.__vorcaro && window.__vorcaro.scrapeChats)
            ? window.__vorcaro.scrapeChats('{platform_tag}', {{ label: {label_json} }})
            : window.__TAURI__.core.invoke('vorcaro_scrape_result', \
                {{ platform: '{platform_tag}', rows: [], error: 'driver não carregado — recarregue a aba' }});"
    );
    wv.eval(&js).map_err(|e| e.to_string())
}

// ── Phase F: WhatsApp Cloud API ────────────────────────────────

/// Returns the saved Cloud API config. The access_token is redacted (only the
/// last 4 chars are returned, prefixed with "…") to avoid surfacing the
/// secret in the panel UI inadvertently.
#[tauri::command]
pub fn vorcaro_get_cloud_config() -> WhatsAppCloudConfig {
    let mut cfg = cloud_api::load_config();
    if cfg.access_token.len() > 4 {
        let tail = &cfg.access_token[cfg.access_token.len() - 4..];
        cfg.access_token = format!("…{tail}");
    } else if !cfg.access_token.is_empty() {
        cfg.access_token = "…".into();
    }
    cfg
}

/// Save Cloud API config. If the frontend sends a redacted-looking token
/// (starts with "…"), we keep the previously-stored real token to avoid
/// the user clobbering their secret by re-saving the panel.
#[tauri::command]
pub fn vorcaro_save_cloud_config(config: WhatsAppCloudConfig) -> Result<(), String> {
    let mut to_save = config;
    if to_save.access_token.starts_with('…') {
        let prev = cloud_api::load_config();
        to_save.access_token = prev.access_token;
    }
    cloud_api::save_config(&to_save)
}

#[tauri::command]
pub async fn vorcaro_verify_cloud_connection() -> Result<String, String> {
    let cfg = cloud_api::load_config();
    cloud_api::verify_connection(&cfg).await
}

#[tauri::command]
pub async fn vorcaro_list_cloud_templates() -> Result<Vec<TemplateInfo>, String> {
    let cfg = cloud_api::load_config();
    cloud_api::list_templates(&cfg).await
}

#[tauri::command]
pub async fn vorcaro_start_campaign(
    app: AppHandle,
    store: tauri::State<'_, VorcaroStore>,
    orch: tauri::State<'_, OrchestratorState>,
    spec: CampaignSpec,
) -> Result<Uuid, String> {
    let campaign = Campaign {
        id: Uuid::new_v4(),
        name: spec.name,
        body: spec.body,
        attachments: spec.attachments,
        targets: spec.targets,
        platform: spec.platform,
        status: CampaignStatus::Draft,
        created_at: chrono::Utc::now(),
        scheduled_at: spec.scheduled_at,
        started_at: None,
        finished_at: None,
        progress: vec![],
        template: spec.template,
        workspace_id: spec.workspace_id,
    };
    let id = campaign.id;
    store.with(|s| {
        s.campaigns.push(campaign);
        persist(s)
    })?;

    let (transport, progress) = make_ports(&app);
    orchestrator::start(orch.inner(), store.inner_clone(), transport, progress, id).await?;
    Ok(id)
}

/// Called once during app setup. Defensively marks any campaigns that were
/// `Running` when BigBox last quit as `Paused` — they will NOT auto-resume.
/// The user has to click Retomar explicitly. This prevents zombie campaigns
/// from continuing to send messages after a crash/restart with stale driver
/// state. Scheduled campaigns DO auto-resume (they haven't started yet).
pub fn rehydrate_on_boot(app: AppHandle) {
    let store: tauri::State<'_, VorcaroStore> = app.state();
    let to_resume: Vec<Uuid> = store.with(|s| {
        let mut scheduled = vec![];
        for c in s.campaigns.iter_mut() {
            match c.status {
                CampaignStatus::Running => {
                    // Demote — don't auto-resume mid-run after a restart.
                    c.status = CampaignStatus::Paused;
                }
                CampaignStatus::Scheduled => {
                    scheduled.push(c.id);
                }
                _ => {}
            }
        }
        let _ = persist(s);
        scheduled
    });

    for id in to_resume {
        let app2 = app.clone();
        tauri::async_runtime::spawn(async move {
            let store_state: tauri::State<'_, VorcaroStore> = app2.state();
            let orch_state: tauri::State<'_, OrchestratorState> = app2.state();
            let (transport, progress) = make_ports(&app2);
            let _ = orchestrator::start(
                orch_state.inner(),
                store_state.inner_clone(),
                transport,
                progress,
                id,
            )
            .await;
        });
    }
}

#[tauri::command]
pub async fn vorcaro_pause_campaign(
    orch: tauri::State<'_, OrchestratorState>,
    id: Uuid,
) -> Result<(), String> {
    orchestrator::pause(orch.inner(), id).await
}

#[tauri::command]
pub async fn vorcaro_resume_campaign(
    app: AppHandle,
    store: tauri::State<'_, VorcaroStore>,
    orch: tauri::State<'_, OrchestratorState>,
    id: Uuid,
) -> Result<(), String> {
    // If the live task is still running, just unpause it.
    {
        let map = orch.campaigns.lock().await;
        if map.contains_key(&id) {
            drop(map);
            return orchestrator::resume(orch.inner(), id).await;
        }
    }
    // Otherwise (e.g. auto-paused on cap/failures, or restarted), respawn the loop.
    // It will continue from where the recorded progress left off — naïvely, but
    // safe: `resolve_targets` returns the full list and the loop re-sends from
    // index 0. For Phase C we keep it simple: resume == restart from scratch
    // unless the campaign is still in-memory.
    let (transport, progress) = make_ports(&app);
    orchestrator::start(orch.inner(), store.inner_clone(), transport, progress, id).await
}

#[tauri::command]
pub async fn vorcaro_abort_campaign(
    orch: tauri::State<'_, OrchestratorState>,
    id: Uuid,
) -> Result<(), String> {
    orchestrator::abort(orch.inner(), id).await
}

#[derive(serde::Deserialize)]
pub struct SendResultPayload {
    pub attempt_id: Uuid,
    pub status: SendResultStatus,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(serde::Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum SendResultStatus {
    Sent,
    Failed,
    InvalidNumber,
    Skipped,
}

impl From<SendResultStatus> for SendStatus {
    fn from(s: SendResultStatus) -> Self {
        match s {
            SendResultStatus::Sent => SendStatus::Sent,
            SendResultStatus::Failed => SendStatus::Failed,
            SendResultStatus::InvalidNumber => SendStatus::InvalidNumber,
            SendResultStatus::Skipped => SendStatus::Skipped,
        }
    }
}

/// Driver inside the chat WebView calls this when sendTo finishes.
#[tauri::command]
pub async fn vorcaro_send_result(
    orch: tauri::State<'_, OrchestratorState>,
    attempt_id: Uuid,
    status: SendResultStatus,
    error: Option<String>,
) -> Result<(), String> {
    orchestrator::route_send_result(
        orch.inner(),
        attempt_id,
        orchestrator::SendOutcome {
            status: status.into(),
            error,
        },
    )
    .await
}

/// Merge a user-confirmed subset of scraped rows into the contacts store.
/// Dedupe is by handle (phone or peer_id+name); existing contacts get their
/// missing fields filled in rather than duplicated.
#[tauri::command]
pub fn vorcaro_import_scraped(
    state: tauri::State<'_, VorcaroStore>,
    platform: Platform,
    rows: Vec<ScrapedChat>,
) -> Result<csv_io::ImportReport, String> {
    if matches!(platform, Platform::WhatsAppCloudApi) {
        return Err("Cloud API não tem fluxo de raspagem.".into());
    }
    state.with(|s| {
        let mut report = csv_io::ImportReport::default();

        for row in rows {
            if row.name.trim().is_empty()
                && row.phone.is_none()
                && row.peer_id.is_none()
                && row.username.is_none()
            {
                report.skipped += 1;
                continue;
            }

            // Dedup key depends on the platform. (Cloud API already rejected above.)
            let existing_idx = s.contacts.iter().position(|c| match platform {
                Platform::WhatsAppWeb => {
                    row.phone.is_some() && c.whatsapp == row.phone
                        || (row.phone.is_none() && c.display_name == row.name && c.whatsapp.is_none())
                }
                Platform::WhatsAppBusinessWeb => {
                    row.phone.is_some() && c.whatsapp_business == row.phone
                        || (row.phone.is_none()
                            && c.display_name == row.name
                            && c.whatsapp_business.is_none())
                }
                Platform::Telegram => {
                    (row.username.is_some() && c.telegram == row.username)
                        || (row.peer_id.is_some()
                            && c.notes.as_deref() == Some(row.peer_id.as_deref().unwrap_or("")))
                        || (row.username.is_none()
                            && row.peer_id.is_none()
                            && c.display_name == row.name)
                }
                Platform::WhatsAppCloudApi => unreachable!(),
            });

            if let Some(i) = existing_idx {
                let c = &mut s.contacts[i];
                match platform {
                    Platform::WhatsAppWeb => {
                        if c.whatsapp.is_none() && row.phone.is_some() { c.whatsapp = row.phone.clone(); }
                    }
                    Platform::WhatsAppBusinessWeb => {
                        if c.whatsapp_business.is_none() && row.phone.is_some() {
                            c.whatsapp_business = row.phone.clone();
                        }
                    }
                    Platform::Telegram => {
                        if c.telegram.is_none() && row.username.is_some() {
                            c.telegram = row.username.clone();
                        }
                    }
                    Platform::WhatsAppCloudApi => unreachable!(),
                }
                report.merged += 1;
            } else {
                let mut new_contact = Contact {
                    id: Uuid::new_v4(),
                    display_name: row.name.clone(),
                    whatsapp: None,
                    whatsapp_business: None,
                    telegram: row.username.clone(),
                    tags: vec![],
                    source: ContactSource::Scraped,
                    notes: row.peer_id.clone(),
                };
                match platform {
                    Platform::WhatsAppWeb => { new_contact.whatsapp = row.phone.clone(); }
                    Platform::WhatsAppBusinessWeb => { new_contact.whatsapp_business = row.phone.clone(); }
                    Platform::Telegram => {}
                    Platform::WhatsAppCloudApi => unreachable!(),
                }
                s.contacts.push(new_contact);
                report.added += 1;
            }
        }

        persist(s)?;
        Ok(report)
    })
}

// ── tiny normalizers (kept private; csv_io.rs has its own copies) ─

fn strip_phone(raw: &str) -> String {
    let trimmed = raw.trim();
    let has_plus = trimmed.starts_with('+');
    let digits: String = trimmed.chars().filter(|c| c.is_ascii_digit()).collect();
    if has_plus { format!("+{digits}") } else { digits }
}

fn strip_telegram(raw: &str) -> String {
    let t = raw.trim();
    if t.starts_with('+') || t.chars().all(|c| c.is_ascii_digit()) {
        return strip_phone(t);
    }
    if let Some(s) = t.strip_prefix('@') { format!("@{s}") } else { format!("@{t}") }
}

