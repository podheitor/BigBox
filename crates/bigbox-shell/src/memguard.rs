// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Heitor Faria

//! Memory guard for leaking service webviews.
//!
//! Some services (WhatsApp Web above all, Gmail to a lesser degree) leak
//! inside their web process: after a day or two a single WebKitWebProcess
//! holds several GB, mostly pushed to swap, and the whole desktop crawls.
//! Hibernation is opt-in and idle-based, so it never catches a leak in a
//! service the user keeps around.
//!
//! The guard runs every [`SWEEP_INTERVAL`]: it lists our WebKitWebProcess
//! children from `/proc`, attributes each new one to the service whose
//! webview was created or reloaded just before the process started, and
//! recycles (terminate + reload) a hidden service whose process exceeds
//! [`PROCESS_LIMIT_BYTES`], at most once per [`RECYCLE_COOLDOWN`]. The visible
//! service is never touched. A process WebKit spawns later on its own (e.g.
//! a process swap on navigation) has no spawn mark; it is attributed through
//! the `sessions/<id>` cache it maps when that shows up, and otherwise only
//! logged once when over the limit.

use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager};

use crate::commands::AppState;

/// Resident + swapped size above which a hidden service is recycled.
/// Gmail alone sits around 1.2 GB right after loading, so stay above that.
pub const PROCESS_LIMIT_BYTES: u64 = 1536 * 1024 * 1024;

/// Minimum time between two recycles of the same service.
const RECYCLE_COOLDOWN: Duration = Duration::from_secs(6 * 60 * 60);

/// A service must have been hidden at least this long before a recycle.
const IDLE_BEFORE_RECYCLE: Duration = Duration::from_secs(5 * 60);

/// Max delay between a webview spawn mark and its process start.
const SPAWN_WINDOW_SECS: f64 = 20.0;

/// How often the guard runs.
const SWEEP_INTERVAL: Duration = Duration::from_secs(60);

/// Linux exposes process start times in USER_HZ ticks, fixed at 100.
const CLOCK_TICKS_PER_SEC: f64 = 100.0;

/// One WebKitWebProcess child of this app.
#[derive(Debug, Clone, PartialEq)]
pub struct WebProcess {
    pub pid: u32,
    /// Start time in seconds since boot.
    pub started_at: f64,
    /// VmRSS + VmSwap in bytes.
    pub bytes: u64,
}

/// Record that `label` is about to spawn a web process (creation or reload).
pub fn note_spawn(state: &AppState, label: &str) {
    if let Some(now) = uptime_secs() {
        state.spawn_marks.lock().unwrap().push((label.to_string(), now));
    }
}

/// Spawn the background memory guard.
pub fn start_memory_guard(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut tick = tokio::time::interval(SWEEP_INTERVAL);
        loop {
            tick.tick().await;
            sweep_leaking_services(&app);
        }
    });
}

fn sweep_leaking_services(app: &AppHandle) {
    let state: tauri::State<'_, AppState> = app.state();
    let procs = child_web_processes(std::process::id());
    attribute_new_processes(&state, &procs, uptime_secs().unwrap_or(0.0));
    let unattributed = unattributed_over_limit(&state, &procs);
    adopt_by_session(&state, &unattributed);
    report_unattributed(&state, &procs);
    let now = Instant::now();
    let Some((label, bytes)) = pick_recycle_target(&state, &procs, now) else {
        return;
    };
    let Some(wv) = app.get_webview(&label) else { return };
    eprintln!("[memguard] recycling {label}: web process at {} MB", bytes / (1024 * 1024));
    state.web_pids.lock().unwrap().remove(&label);
    state.recycled_at.lock().unwrap().insert(label.clone(), now);
    if crate::hibernate::terminate_web_process(&wv) {
        note_spawn(&state, &label);
        crate::hibernate::reload_webview(&wv);
    }
}

/// Bind unattributed processes to the closest earlier spawn mark.
fn attribute_new_processes(state: &AppState, procs: &[WebProcess], now: f64) {
    let mut pids = state.web_pids.lock().unwrap();
    let mut marks = state.spawn_marks.lock().unwrap();
    pids.retain(|_, pid| procs.iter().any(|p| p.pid == *pid));
    let mut fresh: Vec<&WebProcess> =
        procs.iter().filter(|p| !pids.values().any(|pid| *pid == p.pid)).collect();
    fresh.sort_by(|a, b| a.started_at.total_cmp(&b.started_at));
    for proc in fresh {
        if let Some(idx) = closest_mark(&marks, proc.started_at) {
            let (label, _) = marks.remove(idx);
            pids.insert(label, proc.pid);
        }
    }
    marks.retain(|(_, t)| now - t <= SPAWN_WINDOW_SECS);
}

/// PIDs over the limit that no service owns yet.
fn unattributed_over_limit(state: &AppState, procs: &[WebProcess]) -> Vec<u32> {
    let pids = state.web_pids.lock().unwrap();
    procs
        .iter()
        .filter(|p| p.bytes > PROCESS_LIMIT_BYTES && !pids.values().any(|pid| *pid == p.pid))
        .map(|p| p.pid)
        .collect()
}

/// Attribute `candidates` through the `sessions/<id>` path in their maps,
/// unless that service already owns a live process.
fn adopt_by_session(state: &AppState, candidates: &[u32]) {
    for pid in candidates {
        let Ok(maps) = std::fs::read_to_string(format!("/proc/{pid}/maps")) else { continue };
        let Some(id) = session_id_from_maps(&maps) else { continue };
        state.web_pids.lock().unwrap().entry(format!("svc-{id}")).or_insert(*pid);
    }
}

/// First `…/sessions/<id>/…` service id mentioned in a maps body.
fn session_id_from_maps(maps: &str) -> Option<&str> {
    let rest = &maps[maps.find("/sessions/")? + "/sessions/".len()..];
    let id = &rest[..rest.find(['/', '\n'])?];
    (!id.is_empty()).then_some(id)
}

/// Log, once per PID, processes over the limit that stay unattributed.
fn report_unattributed(state: &AppState, procs: &[WebProcess]) {
    let mut reported = state.reported_pids.lock().unwrap();
    reported.retain(|pid| procs.iter().any(|p| p.pid == *pid));
    for pid in unattributed_over_limit(state, procs) {
        if reported.insert(pid) {
            let mb = procs.iter().find(|p| p.pid == pid).map_or(0, |p| p.bytes / (1024 * 1024));
            eprintln!("[memguard] unattributed web process {pid} at {mb} MB (not recycled)");
        }
    }
}

/// Index of the latest mark at or before `started_at` within the window.
/// One tick of slack covers the coarse process start clock.
fn closest_mark(marks: &[(String, f64)], started_at: f64) -> Option<usize> {
    let slack = 1.0 / CLOCK_TICKS_PER_SEC;
    marks
        .iter()
        .enumerate()
        .filter(|(_, (_, t))| *t <= started_at + slack && started_at - t <= SPAWN_WINDOW_SECS)
        .max_by(|a, b| a.1 .1.total_cmp(&b.1 .1))
        .map(|(i, _)| i)
}

/// The largest hidden, idle, attributed service over the limit.
fn pick_recycle_target(
    state: &AppState,
    procs: &[WebProcess],
    now: Instant,
) -> Option<(String, u64)> {
    let active = state.active_view.lock().unwrap().clone();
    let hibernated = state.hibernated.lock().unwrap();
    let last_used = state.last_used.lock().unwrap();
    let recycled_at = state.recycled_at.lock().unwrap();
    let pids = state.web_pids.lock().unwrap();
    let idle = |l: &str| {
        last_used.get(l).is_none_or(|t| now.duration_since(*t) >= IDLE_BEFORE_RECYCLE)
    };
    let cooled = |l: &str| {
        recycled_at.get(l).is_none_or(|t| now.duration_since(*t) >= RECYCLE_COOLDOWN)
    };
    pids.iter()
        .filter(|(l, _)| active.as_deref() != Some(l.as_str()) && !hibernated.contains(*l))
        .filter(|(l, _)| idle(l) && cooled(l))
        .filter_map(|(l, pid)| procs.iter().find(|p| p.pid == *pid).map(|p| (l.clone(), p.bytes)))
        .filter(|(_, bytes)| *bytes > PROCESS_LIMIT_BYTES)
        .max_by_key(|(_, bytes)| *bytes)
}

/// WebKitWebProcess children of `parent`, read from `/proc`.
fn child_web_processes(parent: u32) -> Vec<WebProcess> {
    let Ok(entries) = std::fs::read_dir("/proc") else { return Vec::new() };
    entries
        .filter_map(|e| e.ok()?.file_name().to_str()?.parse::<u32>().ok())
        .filter_map(|pid| read_web_process(pid, parent))
        .collect()
}

fn read_web_process(pid: u32, parent: u32) -> Option<WebProcess> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let (comm, ppid, start_ticks) = parse_stat(&stat)?;
    if ppid != parent || !comm.starts_with("WebKitWebProces") {
        return None;
    }
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    Some(WebProcess {
        pid,
        started_at: start_ticks as f64 / CLOCK_TICKS_PER_SEC,
        bytes: parse_status_bytes(&status),
    })
}

/// `(comm, ppid, starttime)` from a `/proc/<pid>/stat` line.
fn parse_stat(stat: &str) -> Option<(String, u32, u64)> {
    let open = stat.find('(')?;
    let close = stat.rfind(')')?;
    let comm = stat.get(open + 1..close)?.to_string();
    let fields: Vec<&str> = stat.get(close + 2..)?.split_whitespace().collect();
    // After "(comm) ": state=0, ppid=1, …, starttime=19 (field 22 overall).
    Some((comm, fields.get(1)?.parse().ok()?, fields.get(19)?.parse().ok()?))
}

/// VmRSS + VmSwap of a `/proc/<pid>/status` body, in bytes.
fn parse_status_bytes(status: &str) -> u64 {
    let kb = |key: &str| -> u64 {
        status
            .lines()
            .find_map(|l| l.strip_prefix(key))
            .and_then(|v| v.split_whitespace().next()?.parse().ok())
            .unwrap_or(0)
    };
    (kb("VmRSS:") + kb("VmSwap:")) * 1024
}

fn uptime_secs() -> Option<f64> {
    let text = std::fs::read_to_string("/proc/uptime").ok()?;
    text.split_whitespace().next()?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const GIB: u64 = 1024 * 1024 * 1024;

    fn proc(pid: u32, started_at: f64, bytes: u64) -> WebProcess {
        WebProcess { pid, started_at, bytes }
    }

    fn hidden_since(state: &AppState, label: &str, at: Instant) {
        state.last_used.lock().unwrap().insert(label.to_string(), at);
    }

    #[test]
    fn stat_line_with_spaces_in_comm_parses() {
        let line = "5734 (WebKitWebProces) S 4663 4663 4663 0 -1 4194560 1 0 0 0 \
                    10 5 0 0 20 0 30 0 123456 999 888";
        assert_eq!(parse_stat(line), Some(("WebKitWebProces".into(), 4663, 123_456)));
        let odd = "7 (a (b) c) R 1 1 1 0 -1 0 0 0 0 0 0 0 0 0 20 0 1 0 42 0 0";
        assert_eq!(parse_stat(odd), Some(("a (b) c".into(), 1, 42)));
    }

    #[test]
    fn status_bytes_sum_rss_and_swap() {
        let body = "Name:\tWebKitWebProces\nVmRSS:\t   100 kB\nVmSwap:\t  2400 kB\n";
        assert_eq!(parse_status_bytes(body), 2500 * 1024);
    }

    #[test]
    fn processes_bind_to_the_preceding_spawn_mark() {
        let state = AppState::default();
        state.spawn_marks.lock().unwrap().extend([
            ("svc-whatsapp".to_string(), 100.0),
            ("svc-gmail".to_string(), 110.0),
        ]);
        let procs = [proc(1, 50.0, 0), proc(2, 100.4, 0), proc(3, 110.2, 0)];
        attribute_new_processes(&state, &procs, 115.0);
        let pids = state.web_pids.lock().unwrap();
        assert_eq!(pids.get("svc-whatsapp"), Some(&2));
        assert_eq!(pids.get("svc-gmail"), Some(&3));
        assert_eq!(pids.len(), 2, "the main UI process stays unattributed");
    }

    #[test]
    fn dead_pids_are_forgotten_and_stale_marks_pruned() {
        let state = AppState::default();
        state.web_pids.lock().unwrap().insert("svc-a".into(), 9);
        state.spawn_marks.lock().unwrap().push(("svc-b".into(), 10.0));
        attribute_new_processes(&state, &[], 100.0);
        assert!(state.web_pids.lock().unwrap().is_empty());
        assert!(state.spawn_marks.lock().unwrap().is_empty());
    }

    #[test]
    fn largest_hidden_idle_service_over_limit_is_picked() {
        let state = AppState::default();
        let t0 = Instant::now();
        let later = t0 + IDLE_BEFORE_RECYCLE;
        for (label, pid) in [("svc-wa", 1), ("svc-gmail", 2), ("svc-tg", 3)] {
            state.web_pids.lock().unwrap().insert(label.into(), pid);
            hidden_since(&state, label, t0);
        }
        let procs = [proc(1, 0.0, 3 * GIB), proc(2, 0.0, 2 * GIB), proc(3, 0.0, GIB)];
        assert_eq!(pick_recycle_target(&state, &procs, later), Some(("svc-wa".into(), 3 * GIB)));
    }

    #[test]
    fn recently_recycled_service_waits_for_the_cooldown() {
        let state = AppState::default();
        let t0 = Instant::now();
        state.web_pids.lock().unwrap().insert("svc-gmail".into(), 1);
        hidden_since(&state, "svc-gmail", t0);
        state.recycled_at.lock().unwrap().insert("svc-gmail".into(), t0);
        let procs = [proc(1, 0.0, 2 * GIB)];
        let soon = t0 + RECYCLE_COOLDOWN - Duration::from_secs(1);
        assert_eq!(pick_recycle_target(&state, &procs, soon), None);
        let later = t0 + RECYCLE_COOLDOWN;
        assert_eq!(pick_recycle_target(&state, &procs, later), Some(("svc-gmail".into(), 2 * GIB)));
    }

    #[test]
    fn session_id_is_read_from_maps() {
        let maps = "7f00-7f01 r--s 0 00:1f 1 /usr/lib/libc.so\n\
                    7f02-7f03 r--s 0 00:1f 2 /home/u/.local/share/bigbox/sessions/whatsapp/WebKitCache/x\n";
        assert_eq!(session_id_from_maps(maps), Some("whatsapp"));
        assert_eq!(session_id_from_maps("7f00-7f01 r--p 0 00:1f 1 /usr/lib/libc.so\n"), None);
    }

    #[test]
    fn active_recent_and_small_services_are_spared() {
        let state = AppState::default();
        let t0 = Instant::now();
        for (label, pid) in [("svc-wa", 1), ("svc-gmail", 2), ("svc-tg", 3)] {
            state.web_pids.lock().unwrap().insert(label.into(), pid);
            hidden_since(&state, label, t0);
        }
        *state.active_view.lock().unwrap() = Some("svc-wa".into());
        hidden_since(&state, "svc-gmail", t0 + IDLE_BEFORE_RECYCLE);
        let procs = [proc(1, 0.0, 3 * GIB), proc(2, 0.0, 2 * GIB), proc(3, 0.0, GIB / 2)];
        let later = t0 + IDLE_BEFORE_RECYCLE + Duration::from_secs(1);
        assert_eq!(pick_recycle_target(&state, &procs, later), None);
    }
}
