// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Heitor Faria

//! Phase C: campaign send orchestrator.
//!
//! One `tokio` task per running campaign. The task:
//!   1. Resolves recipients from `TargetSpec`.
//!   2. For each recipient: fires `__vorcaro.sendTo(...)` into the matching
//!      chat-service WebView (via the `DriverTransport` port), awaits a
//!      `oneshot` reply from the driver's `vorcaro_send_result` IPC call.
//!   3. Persists per-attempt progress and emits campaign progress (via the
//!      `ProgressSink` port).
//!   4. Sleeps a randomized delay between sends.
//!   5. Auto-pauses after N consecutive failures.
//!   6. Honors daily-cap, pause/resume, and abort signals.
//!
//! This module names **no** `tauri` symbols — everything that would touch the
//! UI or a webview goes out through `bigbox_contract` ports, which the Tauri
//! layer implements.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use chrono::Utc;
use tokio::sync::{oneshot, Mutex as AsyncMutex, Notify};
use uuid::Uuid;

use bigbox_contract::{DriverTransport, ProgressSink};
use bigbox_core::vorcaro::{
    Campaign, CampaignStatus, Contact, Platform, SendAttempt, SendStatus, TargetSpec,
    TemplateUsage, VorcaroState,
};

// ── Public types ────────────────────────────────────────────────

// SendOutcome moved to bigbox-core (shared with bigbox-cloud); re-exported so
// `orchestrator::SendOutcome` paths keep working.
pub use bigbox_core::vorcaro::SendOutcome;

/// Shared in-memory store handle. The engine persists through `bigbox-config`.
pub type SharedStore = Arc<Mutex<Option<VorcaroState>>>;

/// Per-running-campaign control handle, stored in `CampaignRegistry`.
pub struct CampaignControl {
    pub status: Arc<AsyncMutex<CampaignStatus>>,
    /// Signaled when the user resumes from pause OR aborts (to unblock the wait).
    pub wake: Arc<Notify>,
}

/// Pending sendTo attempts, awaiting a result from the driver inside the WebView.
pub type AttemptRegistry = Arc<AsyncMutex<HashMap<Uuid, oneshot::Sender<SendOutcome>>>>;

#[derive(Default)]
pub struct OrchestratorState {
    pub attempts: AttemptRegistry,
    pub campaigns: Arc<AsyncMutex<HashMap<Uuid, CampaignControl>>>,
}

// ── Entrypoint ──────────────────────────────────────────────────

/// Spawn the send loop for a campaign. Idempotent: if a campaign is already
/// running with the same id, returns Err.
///
/// `transport` and `progress` are the injected ports — the Tauri layer builds
/// them over `AppHandle` and hands them in here.
pub async fn start(
    orch: &OrchestratorState,
    store: SharedStore,
    transport: Arc<dyn DriverTransport>,
    progress: Arc<dyn ProgressSink>,
    campaign_id: Uuid,
) -> Result<(), String> {
    {
        let map = orch.campaigns.lock().await;
        if map.contains_key(&campaign_id) {
            return Err("campanha já está rodando".into());
        }
    }

    let control = CampaignControl {
        status: Arc::new(AsyncMutex::new(CampaignStatus::Running)),
        wake: Arc::new(Notify::new()),
    };
    let status_h = control.status.clone();
    let wake_h = control.wake.clone();

    orch.campaigns.lock().await.insert(campaign_id, control);

    let attempts = orch.attempts.clone();
    let campaigns_map = orch.campaigns.clone();

    tokio::spawn(async move {
        let outcome = run_campaign(
            transport,
            progress.clone(),
            store,
            attempts,
            campaign_id,
            status_h.clone(),
            wake_h,
        )
        .await;

        // Final status: leave as Aborted/Done in store. Always remove from the
        // live registry so the campaign can be restarted.
        campaigns_map.lock().await.remove(&campaign_id);

        let final_status = match outcome {
            Ok(()) => *status_h.lock().await,
            Err(_) => CampaignStatus::Aborted,
        };
        emit_progress(
            &*progress,
            campaign_id,
            "campaign-finished",
            serde_json::json!({ "status": format!("{:?}", final_status).to_lowercase() }),
        );
    });

    Ok(())
}

pub async fn pause(orch: &OrchestratorState, campaign_id: Uuid) -> Result<(), String> {
    let map = orch.campaigns.lock().await;
    let Some(ctrl) = map.get(&campaign_id) else {
        return Err("campanha não está rodando".into());
    };
    *ctrl.status.lock().await = CampaignStatus::Paused;
    Ok(())
}

pub async fn resume(orch: &OrchestratorState, campaign_id: Uuid) -> Result<(), String> {
    let map = orch.campaigns.lock().await;
    let Some(ctrl) = map.get(&campaign_id) else {
        return Err("campanha não está rodando".into());
    };
    *ctrl.status.lock().await = CampaignStatus::Running;
    ctrl.wake.notify_one();
    Ok(())
}

pub async fn abort(orch: &OrchestratorState, campaign_id: Uuid) -> Result<(), String> {
    let map = orch.campaigns.lock().await;
    let Some(ctrl) = map.get(&campaign_id) else {
        return Err("campanha não está rodando".into());
    };
    *ctrl.status.lock().await = CampaignStatus::Aborted;
    ctrl.wake.notify_one();
    Ok(())
}

/// Driver inside the WebView calls this via IPC; we route the outcome back to
/// the oneshot waiting in `run_campaign`.
pub async fn route_send_result(
    orch: &OrchestratorState,
    attempt_id: Uuid,
    outcome: SendOutcome,
) -> Result<(), String> {
    let mut reg = orch.attempts.lock().await;
    if let Some(tx) = reg.remove(&attempt_id) {
        let _ = tx.send(outcome);
        Ok(())
    } else {
        // No matching attempt — likely timed out already. Not an error.
        Ok(())
    }
}

// ── Core loop ──────────────────────────────────────────────────

async fn run_campaign(
    transport: Arc<dyn DriverTransport>,
    progress: Arc<dyn ProgressSink>,
    store: SharedStore,
    attempts: AttemptRegistry,
    campaign_id: Uuid,
    status: Arc<AsyncMutex<CampaignStatus>>,
    wake: Arc<Notify>,
) -> Result<(), String> {
    // Snapshot what we need from the store under the sync mutex.
    let (campaign, recipients, settings, platform) = {
        let mut guard = store.lock().unwrap();
        let s = guard.as_mut().ok_or("store not loaded")?;
        let Some(c) = s.campaigns.iter().find(|c| c.id == campaign_id).cloned() else {
            return Err("campanha não encontrada".into());
        };
        let recipients = resolve_targets(s, &c.targets);
        (c.clone(), recipients, s.settings.clone(), c.platform)
    };

    // Read attachments once up front — every recipient gets the same payload.
    // base64 in memory is fine for the typical 1-10 MB campaign attachment;
    // we'll need streaming if we ever ship 100 MB+ media.
    let attachments_payload: Vec<serde_json::Value> = campaign
        .attachments
        .iter()
        .filter_map(|p| match crate::attachments::read_as_base64(p) {
            Ok((name, mime, b64)) => Some(serde_json::json!({
                "name": name, "mime": mime, "b64": b64,
            })),
            Err(e) => {
                emit_progress(&*progress, campaign_id, "attachment-error", serde_json::json!({
                    "path": p.to_string_lossy(), "error": e,
                }));
                None
            }
        })
        .collect();

    // ── Scheduled-start wait ───────────────────────────────────
    if let Some(when) = campaign.scheduled_at {
        let now = Utc::now();
        if when > now {
            let wait_dur = (when - now).to_std().unwrap_or(std::time::Duration::ZERO);
            update_campaign(&store, campaign_id, |c| c.status = CampaignStatus::Scheduled);
            persist(&store)?;
            emit_progress(&*progress, campaign_id, "scheduled", serde_json::json!({
                "scheduled_at": when.to_rfc3339(),
            }));

            // Interruptible sleep — break early on abort.
            let mut remaining = wait_dur;
            while remaining > std::time::Duration::ZERO {
                if matches!(*status.lock().await, CampaignStatus::Aborted) {
                    finalize(&store, campaign_id, CampaignStatus::Aborted);
                    return Ok(());
                }
                let step = remaining.min(std::time::Duration::from_secs(30));
                tokio::time::sleep(step).await;
                remaining = remaining.saturating_sub(step);
            }
        }
    }

    // Mark Running + started_at in the persisted record.
    update_campaign(&store, campaign_id, |c| {
        c.status = CampaignStatus::Running;
        if c.started_at.is_none() {
            c.started_at = Some(Utc::now());
        }
    });
    persist(&store)?;

    // Build a quick lookup of already-attempted contacts so we can skip /
    // honor the retry budget during resume.
    let progress_so_far = campaign.progress.clone();

    let mut consecutive_failures: u32 = 0;

    for contact in recipients.iter() {
        // ── Resume-aware skip: terminal-success / over-budget? ─
        let prior: Vec<&SendAttempt> = progress_so_far
            .iter()
            .filter(|a| a.contact_id == contact.id)
            .collect();
        let already_terminal = prior.iter().any(|a| matches!(
            a.status,
            SendStatus::Sent | SendStatus::InvalidNumber | SendStatus::Skipped
        ));
        if already_terminal {
            continue;
        }
        let prior_failures = prior
            .iter()
            .filter(|a| matches!(a.status, SendStatus::Failed))
            .count() as u32;
        if prior_failures > settings.max_retries_per_recipient {
            // Already burned through the retry budget — leave as Failed, move on.
            continue;
        }

        // ── Control: paused or aborted? ────────────────────────
        loop {
            let st = *status.lock().await;
            match st {
                CampaignStatus::Aborted => {
                    hide_overlay_for(&*transport, platform, campaign.workspace_id.as_deref());
                    finalize(&store, campaign_id, CampaignStatus::Aborted);
                    return Ok(());
                }
                CampaignStatus::Paused => {
                    emit_progress(&*progress, campaign_id, "paused", serde_json::json!({}));
                    wake.notified().await;
                    continue;
                }
                _ => break,
            }
        }

        // ── Daily cap check ────────────────────────────────────
        let cap_ok = {
            let mut guard = store.lock().unwrap();
            let s = guard.as_mut().unwrap();
            let used = s.daily_cap.count(platform);
            used < s.settings.daily_cap_per_platform
        };
        if !cap_ok {
            emit_progress(&*progress, campaign_id, "daily-cap-reached", serde_json::json!({
                "platform": format!("{:?}", platform),
            }));
            hide_overlay_for(&*transport, platform, campaign.workspace_id.as_deref());
            finalize(&store, campaign_id, CampaignStatus::Paused);
            return Ok(());
        }

        // ── Send ───────────────────────────────────────────────
        // Lock the chat-service tab JUST during the send. Between sends the
        // user can use WA freely; the overlay returns for the next recipient.
        if !matches!(platform, Platform::WhatsAppCloudApi) {
            let label = format!("svc-{}",
                campaign.workspace_id.as_deref().unwrap_or(platform.service_id()));
            let _ = transport.eval(&label, "if(window.__vorcaro && window.__vorcaro.showCampaignOverlay) \
                window.__vorcaro.showCampaignOverlay();");
        }

        let attempt_id = Uuid::new_v4();
        let outcome = if matches!(platform, Platform::WhatsAppCloudApi) {
            perform_send_cloud_api(contact, &campaign.body, campaign.template.as_ref()).await
        } else {
            perform_send(
                &*transport,
                &attempts,
                platform,
                campaign.workspace_id.as_deref(),
                contact,
                &campaign.body,
                &attachments_payload,
                attempt_id,
            )
            .await
        };

        // Send done — unlock the tab for the delay window.
        hide_overlay_for(&*transport, platform, campaign.workspace_id.as_deref());

        // ── Record + emit ──────────────────────────────────────
        let attempt = SendAttempt {
            contact_id: contact.id,
            status: outcome.status,
            error: outcome.error.clone(),
            at: Utc::now(),
        };
        update_campaign(&store, campaign_id, |c| c.progress.push(attempt.clone()));
        if matches!(outcome.status, SendStatus::Sent) {
            let mut guard = store.lock().unwrap();
            guard.as_mut().unwrap().daily_cap.increment(platform);
        }
        persist(&store)?;
        emit_progress(&*progress, campaign_id, "attempt", serde_json::to_value(&attempt).unwrap());

        // ── Auto-pause after N consecutive failures ────────────
        match outcome.status {
            SendStatus::Sent => consecutive_failures = 0,
            _ => consecutive_failures += 1,
        }
        if consecutive_failures >= settings.auto_pause_after_consecutive_failures.max(1) {
            emit_progress(&*progress, campaign_id, "auto-paused", serde_json::json!({
                "consecutive_failures": consecutive_failures,
            }));
            hide_overlay_for(&*transport, platform, campaign.workspace_id.as_deref());
            finalize(&store, campaign_id, CampaignStatus::Paused);
            return Ok(());
        }

        // ── Randomized delay ───────────────────────────────────
        let (lo, hi) = (
            settings.min_delay_secs.min(settings.max_delay_secs),
            settings.max_delay_secs.max(settings.min_delay_secs),
        );
        let delay_secs = if hi == lo { lo } else {
            use rand::Rng;
            rand::thread_rng().gen_range(lo..=hi)
        };
        if delay_secs > 0 {
            // Sleep in 250ms slices so abort can interrupt mid-wait.
            let total_ms = (delay_secs as u64) * 1000;
            let mut slept = 0u64;
            while slept < total_ms {
                if matches!(*status.lock().await, CampaignStatus::Aborted) {
                    hide_overlay_for(&*transport, platform, campaign.workspace_id.as_deref());
                    finalize(&store, campaign_id, CampaignStatus::Aborted);
                    return Ok(());
                }
                let step = total_ms.saturating_sub(slept).min(250);
                tokio::time::sleep(std::time::Duration::from_millis(step)).await;
                slept += step;
            }
        }
    }

    hide_overlay_for(&*transport, platform, campaign.workspace_id.as_deref());
    finalize(&store, campaign_id, CampaignStatus::Done);
    Ok(())
}

async fn perform_send(
    transport: &dyn DriverTransport,
    attempts: &AttemptRegistry,
    platform: Platform,
    workspace_id: Option<&str>,
    contact: &Contact,
    body: &str,
    attachments: &[serde_json::Value],
    attempt_id: Uuid,
) -> SendOutcome {
    // Pick the matching WebView label. `workspace_id` (e.g. "whatsapp_2")
    // overrides the canonical service id for users with multiple slots.
    let label = match workspace_id {
        Some(id) => format!("svc-{id}"),
        None => format!("svc-{}", platform.service_id()),
    };
    if !transport.webview_exists(&label) {
        return SendOutcome {
            status: SendStatus::Failed,
            error: Some(format!("{} não está aberto no BigBox", platform.service_id())),
        };
    }

    // Pick the right handle. Cloud API never reaches this function — the
    // run_campaign loop dispatches to perform_send_cloud_api instead.
    // For WA platforms, fall back to the other WA field when the preferred
    // one is empty — most users keep the same number under both labels.
    let handle_opt = match platform {
        Platform::WhatsAppWeb => contact.whatsapp.clone().or_else(|| contact.whatsapp_business.clone()),
        Platform::WhatsAppBusinessWeb => contact.whatsapp_business.clone().or_else(|| contact.whatsapp.clone()),
        Platform::Telegram => contact.telegram.clone(),
        Platform::WhatsAppCloudApi => unreachable!("WebView send path can't be Cloud API"),
    };
    let Some(handle) = handle_opt else {
        return SendOutcome {
            status: SendStatus::Skipped,
            error: Some("contato sem identificador para a plataforma".into()),
        };
    };

    // Variable substitution
    let body = substitute_vars(body, contact);

    // Register oneshot first, then dispatch.
    let (tx, rx) = oneshot::channel();
    attempts.lock().await.insert(attempt_id, tx);

    let attachments_json = serde_json::to_string(attachments).unwrap_or_else(|_| "[]".into());
    let js = format!(
        "(window.__vorcaro && window.__vorcaro.sendTo) \
            ? window.__vorcaro.sendTo({phone}, {text}, {aid}, {atts}, {expected}) \
            : window.__TAURI__.core.invoke('vorcaro_send_result', \
                {{ attempt_id: {aid}, status: 'failed', error: 'driver não carregado' }});",
        phone = serde_json::to_string(&handle).unwrap(),
        text = serde_json::to_string(&body).unwrap(),
        aid = serde_json::to_string(&attempt_id.to_string()).unwrap(),
        atts = attachments_json,
        expected = serde_json::to_string(&contact.display_name).unwrap(),
    );
    if let Err(e) = transport.eval(&label, &js) {
        attempts.lock().await.remove(&attempt_id);
        return SendOutcome {
            status: SendStatus::Failed,
            error: Some(format!("wv.eval falhou: {e}")),
        };
    }

    // Wait for driver result (max 90s — composer wait + tick wait + slack).
    let timeout = std::time::Duration::from_secs(90);
    match tokio::time::timeout(timeout, rx).await {
        Ok(Ok(outcome)) => outcome,
        Ok(Err(_)) => {
            attempts.lock().await.remove(&attempt_id);
            SendOutcome {
                status: SendStatus::Failed,
                error: Some("driver channel cancelado".into()),
            }
        }
        Err(_) => {
            attempts.lock().await.remove(&attempt_id);
            SendOutcome {
                status: SendStatus::Failed,
                error: Some("timeout aguardando driver (90s)".into()),
            }
        }
    }
}

/// Send via WhatsApp Cloud API (no WebView). Branches on whether the campaign
/// has a `TemplateUsage` configured: template send for cold outreach, free-form
/// text otherwise (only works for recipients with an open 24h window).
async fn perform_send_cloud_api(
    contact: &Contact,
    body: &str,
    template: Option<&TemplateUsage>,
) -> SendOutcome {
    // Cloud API addresses recipients by E.164-without-+. Cloud API IS Business,
    // so prefer the WA-Business field then fall back to personal.
    let Some(handle) = contact.whatsapp_business.as_deref().or(contact.whatsapp.as_deref()) else {
        return SendOutcome {
            status: SendStatus::Skipped,
            error: Some("contato sem número WhatsApp".into()),
        };
    };

    let cfg = bigbox_cloud::load_config();
    if !cfg.is_complete() {
        return SendOutcome {
            status: SendStatus::Failed,
            error: Some("credenciais Cloud API não configuradas".into()),
        };
    }

    let body_substituted = substitute_vars(body, contact);

    if let Some(t) = template {
        // Substitute each param per recipient.
        let params: Vec<String> = t
            .body_params
            .iter()
            .map(|p| substitute_vars(p, contact))
            .collect();
        bigbox_cloud::send_template(&cfg, handle, &t.name, &t.language, &params).await
    } else {
        bigbox_cloud::send_text(&cfg, handle, &body_substituted).await
    }
}

/// Replace `{nome}`, `{name}`, `{firstname}`, `{whatsapp}`, `{telegram}`,
/// `{tag}` (first tag), `{notes}` with values from the contact.
fn substitute_vars(body: &str, c: &Contact) -> String {
    let firstname = c.display_name.split_whitespace().next().unwrap_or("").to_string();
    let first_tag = c.tags.first().cloned().unwrap_or_default();
    body.replace("{nome}", &c.display_name)
        .replace("{name}", &c.display_name)
        .replace("{firstname}", &firstname)
        .replace("{primeironome}", &firstname)
        .replace("{whatsapp}", c.whatsapp.as_deref().unwrap_or(""))
        .replace("{telegram}", c.telegram.as_deref().unwrap_or(""))
        .replace("{tag}", &first_tag)
        .replace("{notes}", c.notes.as_deref().unwrap_or(""))
}

// ── Helpers ─────────────────────────────────────────────────────

pub fn resolve_targets(s: &VorcaroState, spec: &TargetSpec) -> Vec<Contact> {
    match spec {
        TargetSpec::List(list_id) => s
            .lists
            .iter()
            .find(|l| l.id == *list_id)
            .map(|l| {
                l.contact_ids
                    .iter()
                    .filter_map(|cid| s.contacts.iter().find(|c| &c.id == cid).cloned())
                    .collect()
            })
            .unwrap_or_default(),
        TargetSpec::Tag(tag) => s
            .contacts
            .iter()
            .filter(|c| c.tags.iter().any(|t| t == tag))
            .cloned()
            .collect(),
        TargetSpec::AdHoc(ids) => ids
            .iter()
            .filter_map(|cid| s.contacts.iter().find(|c| &c.id == cid).cloned())
            .collect(),
    }
}

fn update_campaign<F: FnOnce(&mut Campaign)>(store: &SharedStore, campaign_id: Uuid, f: F) {
    let mut guard = store.lock().unwrap();
    if let Some(s) = guard.as_mut() {
        if let Some(c) = s.campaigns.iter_mut().find(|c| c.id == campaign_id) {
            f(c);
        }
    }
}

fn finalize(store: &SharedStore, campaign_id: Uuid, final_status: CampaignStatus) {
    update_campaign(store, campaign_id, |c| {
        c.status = final_status;
        if matches!(final_status, CampaignStatus::Done | CampaignStatus::Aborted) {
            c.finished_at = Some(Utc::now());
        }
    });
    let _ = persist(store);
}

/// Hide the chat-tab lockout overlay. Safe to call multiple times / when not present.
fn hide_overlay_for(transport: &dyn DriverTransport, platform: Platform, workspace_id: Option<&str>) {
    if matches!(platform, Platform::WhatsAppCloudApi) {
        return;
    }
    let label = format!("svc-{}", workspace_id.unwrap_or(platform.service_id()));
    let _ = transport.eval(
        &label,
        "if(window.__vorcaro && window.__vorcaro.hideCampaignOverlay) \
            window.__vorcaro.hideCampaignOverlay();",
    );
}

/// Persist the in-memory store to disk through `bigbox-config`.
fn persist(store: &SharedStore) -> Result<(), String> {
    let guard = store.lock().unwrap();
    if let Some(s) = guard.as_ref() {
        bigbox_config::store::save(s)
    } else {
        Ok(())
    }
}

fn emit_progress(progress: &dyn ProgressSink, campaign_id: Uuid, kind: &str, payload: serde_json::Value) {
    progress.emit(campaign_id, kind, payload);
}
