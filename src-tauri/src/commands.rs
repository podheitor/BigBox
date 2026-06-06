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
    /// Per-service zoom level (1.0 = 100%). Persists for the session.
    pub zoom_levels:   Mutex<HashMap<String, f64>>,
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

/// Keep the page's WebAudio context suspended while nothing is actually
/// playing. WhatsApp/Telegram Web prime an AudioContext at load and never let
/// it go idle, so WebKitGTK's GStreamer pulsesink holds an *uncorked* PipeWire
/// playback stream open forever. KDE's task manager treats any uncorked stream
/// from the window's PID as "playing audio" and overlays a speaker indicator on
/// the taskbar button — which sits on top of the notification-count badge.
///
/// We auto-suspend each AudioContext shortly after creation and after playback
/// ends, and resume it the instant real playback starts (a media element plays,
/// or an AudioScheduledSourceNode / Audio() is started). Suspended contexts let
/// WebKit cork/close the PipeWire stream, so the speaker indicator disappears
/// while notification sounds still play on demand.
const AUDIO_IDLE_SCRIPT: &str = r#"
(function(){
  var IDLE_MS = 1500;
  var ctxs = new Set();
  function anyPlaying(){
    var els = document.querySelectorAll('audio,video');
    for(var i=0;i<els.length;i++){
      var e = els[i];
      if(!e.paused && !e.ended && e.currentTime > 0 && e.readyState > 2) return true;
    }
    return false;
  }
  function maybeSuspend(ctx){
    if(ctx.__bbActive) return;
    if(anyPlaying()) return;
    if(ctx.state === 'running'){ try{ ctx.suspend(); }catch(_){} }
  }
  function scheduleIdle(ctx){
    clearTimeout(ctx.__bbTimer);
    ctx.__bbTimer = setTimeout(function(){ maybeSuspend(ctx); }, IDLE_MS);
  }
  function resume(ctx){
    if(ctx.state === 'suspended'){ try{ ctx.resume(); }catch(_){} }
  }
  function wrap(Orig){
    if(!Orig) return Orig;
    function Wrapped(){
      var ctx = new Orig(arguments[0]);
      ctxs.add(ctx);
      // Resume + (re)arm idle timer whenever a source node starts.
      var oc = ctx.createBufferSource;
      if(oc){ ctx.createBufferSource = function(){
        var n = oc.apply(ctx, arguments);
        var os = n.start;
        n.start = function(){ ctx.__bbActive = true; resume(ctx); var r = os.apply(n, arguments);
          n.addEventListener('ended', function(){ ctx.__bbActive = false; scheduleIdle(ctx); });
          return r; };
        return n;
      }; }
      scheduleIdle(ctx);
      return ctx;
    }
    Wrapped.prototype = Orig.prototype;
    return Wrapped;
  }
  try{ window.AudioContext = wrap(window.AudioContext); }catch(_){}
  try{ window.webkitAudioContext = wrap(window.webkitAudioContext); }catch(_){}

  // Media elements: resume any suspended ctx on play; re-arm idle on pause/end.
  function onPlay(){ ctxs.forEach(resume); }
  function onStop(){ ctxs.forEach(scheduleIdle); }
  document.addEventListener('play', onPlay, true);
  document.addEventListener('playing', onPlay, true);
  document.addEventListener('pause', onStop, true);
  document.addEventListener('ended', onStop, true);
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

// WebKitGTK doesn't synthesize image File entries on the DOM `paste` event
// from GTK clipboard image targets — WhatsApp/Telegram see e.clipboardData
// with no items and silently drop the paste. We listen for paste events;
// when no image is present, we read the async Clipboard API (enabled via
// javascript_can_access_clipboard), wrap any image blob as a File, and
// re-dispatch a synthetic paste with a populated DataTransfer.
const CLIPBOARD_IMAGE_SHIM: &str = r#"
(function(){
  var REENTRY = '__bigbox_paste_reentry__';
  document.addEventListener('paste', function(ev){
    if(ev[REENTRY]) return;
    var cd = ev.clipboardData;
    if(cd){
      if(cd.files && cd.files.length) return;
      if(cd.items){
        for(var i=0;i<cd.items.length;i++){
          if(cd.items[i].type && cd.items[i].type.indexOf('image/') === 0) return;
        }
      }
    }
    if(!navigator.clipboard || !navigator.clipboard.read) return;
    var target = ev.target;
    navigator.clipboard.read().then(function(items){
      var jobs = [];
      items.forEach(function(item){
        item.types.forEach(function(t){
          if(t.indexOf('image/') === 0){
            jobs.push(item.getType(t).then(function(blob){
              return new File([blob], 'pasted-image.' + (t.split('/')[1]||'png'), {type: t});
            }));
          }
        });
      });
      if(!jobs.length) return;
      Promise.all(jobs).then(function(files){
        if(!files.length) return;
        var dt = new DataTransfer();
        files.forEach(function(f){ dt.items.add(f); });
        var synth = new ClipboardEvent('paste', {
          bubbles: true,
          cancelable: true,
          clipboardData: dt
        });
        synth[REENTRY] = true;
        (target || document.activeElement || document.body).dispatchEvent(synth);
      });
    }).catch(function(){});
  }, true);
})();
"#;


// WhatsApp Web detects MSE codec support via MediaSource.isTypeSupported and,
// when the engine reports HE-AAC capability, ships videos as HE-AAC v2 + PS
// inside fragmented MP4. WebKitGTK 4.1's GStreamer pipeline (avdec_aac /
// fdkaacdec / faad) all mishandle that codec_data and either silence audio
// or abort the whole video pipeline. By rejecting the HE-AAC family in
// isTypeSupported we force WhatsApp to fall back to plain AAC-LC
// (mp4a.40.2), which the default decoder handles correctly.
const CODEC_FILTER_SCRIPT: &str = r#"
(function(){
  if(typeof MediaSource === 'undefined') return;
  var BLOCKED = /mp4a\.40\.(5|29|39)/i;
  function patch(obj){
    if(!obj || !obj.isTypeSupported) return;
    var orig = obj.isTypeSupported.bind(obj);
    obj.isTypeSupported = function(type){
      if(typeof type === 'string' && BLOCKED.test(type)) return false;
      return orig(type);
    };
  }
  patch(window.MediaSource);
  if(typeof window.ManagedMediaSource !== 'undefined'){
    patch(window.ManagedMediaSource);
  }
})();
"#;

// WebKitGTK 4.1 has a bug in the `<video src=blob:...>` (non-MSE) path: the
// video stream decodes but audio is silently dropped — confirmed against an
// AAC-LC fmp4 sample where both http:// and MSE+SourceBuffer routes work
// fine. WhatsApp Web sometimes serves clips that way (service worker hands
// the page a Blob, the page does `video.src = URL.createObjectURL(blob)`),
// which is why audio drops there even after the HE-AAC filter.
//
// Workaround: when a same-origin blob: URL is assigned to a <video>, swap
// it for a synthetic MediaSource fed with the blob's bytes. That routes
// playback through `WebKitMediaSourceGStreamer` which handles audio
// correctly. We track blobs created via URL.createObjectURL so we can map
// the blob: URL back to its underlying Blob.
const BLOB_VIDEO_PATCH: &str = r#"
(function(){
  if(typeof MediaSource === 'undefined') return;

  function log(line){
    if(!window.__BB_DEBUG_CODEC) return;
    try {
      if(window.__TAURI__ && window.__TAURI__.core && window.__TAURI__.core.invoke){
        window.__TAURI__.core.invoke('bb_log', { line: '[' + new Date().toISOString() + '] ' + line });
      }
    } catch(_){}
  }

  // Probe the actual avc1 profile/level/compat from the avcC box so we can
  // hand addSourceBuffer the matching codec string. WebKit MSE rejects
  // mismatched profile claims even when the family is supported.
  function probeAvc(buf){
    var v = new Uint8Array(buf);
    var limit = Math.min(v.length - 8, 65536);
    for(var i = 0; i < limit; i++){
      if(v[i] === 0x61 && v[i+1] === 0x76 && v[i+2] === 0x63 && v[i+3] === 0x43){
        var profile = v[i+5];
        var profileCompat = v[i+6];
        var level = v[i+7];
        function hex(n){ return (n < 16 ? '0' : '') + n.toString(16); }
        return 'avc1.' + hex(profile) + hex(profileCompat) + hex(level);
      }
    }
    return null;
  }

  // Track which video elements we've already redirected so re-assignments
  // of the same blob don't loop. WeakMap key=video, value=string of last
  // redirected blob URL.
  var redirected = new WeakMap();
  // URLs that came from URL.createObjectURL(MediaSource) — never redirect those,
  // they're already MSE-backed and trying to fetch them fails ("Load failed").
  var skipUrls = new Set();
  var origCreate = URL.createObjectURL.bind(URL);
  URL.createObjectURL = function(obj){
    var url = origCreate(obj);
    if(obj instanceof MediaSource || (typeof ManagedMediaSource !== 'undefined' && obj instanceof ManagedMediaSource)){
      skipUrls.add(url);
    }
    return url;
  };
  var origRevoke = URL.revokeObjectURL.bind(URL);
  URL.revokeObjectURL = function(url){ skipUrls.delete(url); return origRevoke(url); };

  // URLs we've tried to fetch and failed — don't retry, prevents log spam
  // and CPU thrash from the polling loop hitting MSE-internal currentSrc
  // URLs that aren't fetchable.
  var failedUrls = new Set();

  function feedFromUrl(video, blobUrl){
    if(redirected.get(video) === blobUrl) return;
    if(failedUrls.has(blobUrl)) return;
    redirected.set(video, blobUrl);
    log('blob-patch: feedFromUrl ' + blobUrl);

    fetch(blobUrl).then(function(r){ return r.arrayBuffer(); }).then(function(buf){
      var avc = probeAvc(buf) || 'avc1.42E01E';
      var codec = 'video/mp4; codecs="' + avc + ',mp4a.40.2"';
      log('blob-patch: fetched bytes=' + buf.byteLength + ' avc=' + avc);

      // Stash the largest blob seen so devtools can extract it via
      // `await window.bbLastVideoB64()` for offline ffprobe inspection.
      if(buf.byteLength > 1000000){
        window.bbLastVideo = buf;
        window.bbLastVideoB64 = function(){
          var u8 = new Uint8Array(buf);
          var s = '';
          for(var i = 0; i < u8.length; i++) s += String.fromCharCode(u8[i]);
          return btoa(s);
        };
      }

      var ms = new MediaSource();
      var msUrl = origCreate(ms);
      // Mark our synthetic MediaSource URL so the poll/setter intercepts
      // ignore it — origCreate bypasses the URL.createObjectURL wrapper
      // that would normally add it to skipUrls.
      skipUrls.add(msUrl);
      // Use the underlying setter to skip our own intercept loop.
      origSetSrc.call(video, msUrl);

      var tag = 'sz=' + buf.byteLength;
      ms.addEventListener('sourceended', function(){ log('blob-patch: sourceended ' + tag); });
      ms.addEventListener('sourceclose', function(){ log('blob-patch: sourceclose ' + tag); });
      video.addEventListener('error', function(){
        var err = video.error;
        log('blob-patch: video error code=' + (err ? err.code : '?') + ' msg=' + (err ? err.message : '') + ' ' + tag);
      });
      video.addEventListener('stalled', function(){ log('blob-patch: video stalled ' + tag); });
      video.addEventListener('canplay', function(){ log('blob-patch: video canplay ' + tag); });
      video.addEventListener('playing', function(){ log('blob-patch: video PLAYING ' + tag); });

      ms.addEventListener('sourceopen', function once(){
        ms.removeEventListener('sourceopen', once);
        log('blob-patch: sourceopen ' + tag);
        var sb;
        try { sb = ms.addSourceBuffer(codec); }
        catch(e){
          log('blob-patch: addSourceBuffer FAIL codec=' + codec + ' err=' + e.message);
          try { sb = ms.addSourceBuffer('video/mp4; codecs="avc1.42E01E,mp4a.40.2"'); }
          catch(e2){ log('blob-patch: fallback addSourceBuffer FAIL ' + e2.message); return; }
        }
        sb.addEventListener('error', function(){ log('blob-patch: sb error ' + tag); });
        sb.addEventListener('abort', function(){ log('blob-patch: sb abort ' + tag); });

        // Chunked appendBuffer: WebKit MSE rejects/aborts a single huge
        // append (~11 MB observed). 1 MB chunks process reliably and let
        // the demuxer emit progress events between calls.
        var CHUNK = 1024 * 1024;
        var off = 0;
        var total = buf.byteLength;
        function appendNext(){
          if(off >= total){
            var br = (sb.buffered && sb.buffered.length)
              ? sb.buffered.start(0) + '-' + sb.buffered.end(0) : 'empty';
            log('blob-patch: append complete buffered=' + br + ' ' + tag);
            try { ms.endOfStream(); log('blob-patch: endOfStream OK ' + tag); }
            catch(e){ log('blob-patch: endOfStream FAIL ' + e.message + ' ' + tag); }
            return;
          }
          var end = Math.min(off + CHUNK, total);
          var slice = buf.slice(off, end);
          off = end;
          try { sb.appendBuffer(slice); }
          catch(e){ log('blob-patch: appendBuffer FAIL off=' + off + ' ' + e.message + ' ' + tag); }
        }
        sb.addEventListener('updateend', appendNext);
        appendNext();
      });
    }).catch(function(e){
      log('blob-patch: fetch FAIL ' + blobUrl + ' err=' + e.message);
      failedUrls.add(blobUrl);
      redirected.delete(video);
    });
  }

  // Capture the native <video>.src setter before anyone else can.
  var proto = HTMLMediaElement.prototype;
  var desc = Object.getOwnPropertyDescriptor(proto, 'src');
  if(!desc || !desc.set){ log('blob-patch: cannot capture src setter'); return; }
  var origSetSrc = desc.set;
  var origGetSrc = desc.get;

  function shouldIntercept(value){
    if(typeof value !== 'string' || !value.startsWith('blob:')) return false;
    // Skip MediaSource-backed blob URLs — already on the working pipeline.
    if(skipUrls.has(value)) return false;
    // Same-origin only — cross-origin blobs can't be fetched.
    return value.indexOf(location.origin) >= 0 || value.startsWith('blob:null/');
  }

  Object.defineProperty(proto, 'src', {
    configurable: true,
    enumerable: desc.enumerable,
    get: origGetSrc ? function(){ return origGetSrc.call(this); } : undefined,
    set: function(v){
      if(shouldIntercept(v) && (this.tagName === 'VIDEO')){
        feedFromUrl(this, v);
        return;
      }
      origSetSrc.call(this, v);
    }
  });

  // setAttribute('src', ...) bypasses the prototype setter — patch too.
  var origSetAttr = Element.prototype.setAttribute;
  Element.prototype.setAttribute = function(name, value){
    if(this.tagName === 'VIDEO' && typeof name === 'string' && name.toLowerCase() === 'src'
       && shouldIntercept(value)){
      feedFromUrl(this, value);
      return;
    }
    return origSetAttr.call(this, name, value);
  };

  // Service-worker-served blob: URLs (e.g. WhatsApp) never pass through the
  // page's URL.createObjectURL nor through the property setters above —
  // the URL arrives already attached to a <video> via React's reconciler.
  // To catch those: poll every <video> in the DOM and, whenever a blob:
  // URL is on currentSrc/src that we haven't redirected yet, take over.
  function scanVideos(){
    var vids = document.querySelectorAll('video');
    for(var i = 0; i < vids.length; i++){
      var v = vids[i];
      // Use .src (the explicitly-set value), not .currentSrc — currentSrc
      // can reflect a MSE-internal URL that we can't fetch and don't need
      // to redirect.
      var s = v.src || '';
      if(shouldIntercept(s) && redirected.get(v) !== s && !failedUrls.has(s)){
        log('blob-patch: poll detected ' + s);
        feedFromUrl(v, s);
      }
    }
  }
  setInterval(scanVideos, 250);
  // Also rescan on any DOM mutation so we catch newly-added videos quickly.
  if(document.body){
    new MutationObserver(scanVideos).observe(document.body, { childList: true, subtree: true, attributes: true, attributeFilter: ['src'] });
  } else {
    document.addEventListener('DOMContentLoaded', function(){
      new MutationObserver(scanVideos).observe(document.body, { childList: true, subtree: true, attributes: true, attributeFilter: ['src'] });
    });
  }
})();
"#;


// Diagnostic-only: dumps every codec capability query, every MSE source-buffer
// addition/error, and every <video> error to /tmp/bigbox-codec-probe.log via
// the bb_log Tauri command. Used to chase down WhatsApp regressions when no
// DevTools is available. Safe to leave installed but produces a lot of log
// output for media-heavy sites.
const CODEC_PROBE_SCRIPT: &str = r#"
(function(){
  function log(line){
    if(!window.__BB_DEBUG_CODEC) return;
    try {
      if(window.__TAURI__ && window.__TAURI__.core && window.__TAURI__.core.invoke){
        window.__TAURI__.core.invoke('bb_log', { line: '[' + new Date().toISOString() + '] ' + line });
      }
    } catch(_){}
  }
  log('probe-init url=' + location.href);

  // Capture every URL.createObjectURL(blob) so we can see what mime type
  // WhatsApp/etc tag video blobs with — needed to decide BLOB_VIDEO_PATCH filter.
  if(typeof URL !== 'undefined' && URL.createObjectURL){
    var origCreate = URL.createObjectURL.bind(URL);
    URL.createObjectURL = function(obj){
      var url = origCreate(obj);
      try {
        if(obj instanceof Blob){
          log('createObjectURL blob type="' + (obj.type || '') + '" size=' + obj.size + ' url=' + url);
        } else if(obj && obj.constructor){
          log('createObjectURL object kind=' + obj.constructor.name + ' url=' + url);
        }
      } catch(_){}
      return url;
    };
  }

  if(typeof MediaSource !== 'undefined'){
    var origIs = MediaSource.isTypeSupported.bind(MediaSource);
    MediaSource.isTypeSupported = function(t){
      var r = origIs(t);
      log('isTypeSupported(' + t + ')=' + r);
      return r;
    };
    var origAdd = MediaSource.prototype.addSourceBuffer;
    MediaSource.prototype.addSourceBuffer = function(t){
      log('addSourceBuffer(' + t + ')');
      try { var sb = origAdd.call(this, t); log('addSourceBuffer OK ' + t); return sb; }
      catch(e){ log('addSourceBuffer FAIL ' + t + ' :: ' + e.message); throw e; }
    };
  }

  function watchVideo(v){
    if(v.__bbWatched) return;
    v.__bbWatched = true;
    v.addEventListener('error', function(){
      var e = v.error || {};
      log('video error code=' + e.code + ' msg=' + e.message + ' src=' + (v.currentSrc || v.src));
    });
    v.addEventListener('loadedmetadata', function(){
      log('video loadedmetadata duration=' + v.duration + ' src=' + (v.currentSrc || v.src));
    });
    v.addEventListener('stalled', function(){
      log('video stalled src=' + (v.currentSrc || v.src));
    });
    v.addEventListener('canplay', function(){ log('video canplay'); });
    v.addEventListener('playing', function(){ log('video playing'); });
  }
  // Watch all existing + future videos
  document.querySelectorAll('video').forEach(watchVideo);
  var mo = new MutationObserver(function(muts){
    muts.forEach(function(m){
      m.addedNodes && m.addedNodes.forEach(function(n){
        if(n.nodeType === 1){
          if(n.tagName === 'VIDEO') watchVideo(n);
          n.querySelectorAll && n.querySelectorAll('video').forEach(watchVideo);
        }
      });
    });
  });
  if(document.body){ mo.observe(document.body, { childList: true, subtree: true }); }
  else { document.addEventListener('DOMContentLoaded', function(){ mo.observe(document.body, { childList: true, subtree: true }); }); }
})();
"#;


// Captures Ctrl++ / Ctrl+- / Ctrl+0 keyboard shortcuts and Ctrl+wheel
// in the service WebView and forwards them to the `zoom_service` Tauri
// command. The native side mutates WebKit's zoom_level and remembers the
// value per-service. We intercept early (capture phase) so the page
// itself can't swallow the events, and call preventDefault so the
// browser's own zoom-on-wheel doesn't double-apply.
const ZOOM_SHORTCUT_SCRIPT: &str = r#"
(function(){
  function invoke(delta){
    try{
      window.__TAURI__.core.invoke('zoom_service', {delta: delta});
    }catch(_){
      try{
        window.__TAURI_INTERNALS__.invoke('zoom_service', {delta: delta});
      }catch(__){ }
    }
  }
  document.addEventListener('keydown', function(ev){
    if(!(ev.ctrlKey || ev.metaKey)) return;
    var k = ev.key;
    if(k === '+' || k === '=' || k === 'Add'){
      ev.preventDefault(); ev.stopPropagation(); invoke(0.1);
    } else if(k === '-' || k === '_' || k === 'Subtract'){
      ev.preventDefault(); ev.stopPropagation(); invoke(-0.1);
    } else if(k === '0'){
      ev.preventDefault(); ev.stopPropagation(); invoke(0.0);
    }
  }, true);
  document.addEventListener('wheel', function(ev){
    if(!(ev.ctrlKey || ev.metaKey)) return;
    ev.preventDefault(); ev.stopPropagation();
    invoke(ev.deltaY < 0 ? 0.1 : -0.1);
  }, {capture: true, passive: false});
})();
"#;

// ── WebView commands ─────────────────────────────────────────────

/// Open (or show) the embedded WebView for a service.
/// On Linux, collapses shell to 64px sidebar so service fills remaining width.
/// MUST be sync (not async) — GTK calls require the main thread.
#[tauri::command]
/// Apply correct position+size to a service WebView on Linux.
/// Position the overlaid service webview over the content area (right of the
/// sidebar, below the titlebar) so it doesn't block shell input. Linux/other
/// only: on Windows each service is its own borderless WebviewWindow placed at
/// the window level (see `position_service_window`), so this is a no-op there.
pub fn apply_svc_bounds(app: &AppHandle, wv: &tauri::Webview<tauri::Wry>) {
    #[cfg(not(target_os = "windows"))]
    {
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
    #[cfg(target_os = "windows")]
    let _ = (app, wv);
}

/// Windows: content-area placement (logical px) for a service window — right of
/// the sidebar, below the titlebar — derived from the main window's size.
#[cfg(target_os = "windows")]
fn content_area(app: &AppHandle) -> (f64, f64, f64, f64) {
    let x = crate::SIDEBAR_W as f64;
    let y = crate::TITLEBAR_H as f64;
    if let Some(window) = app.get_window("main") {
        let scale = window.scale_factor().unwrap_or(1.0);
        let phys  = window.inner_size().unwrap_or_default();
        let lw = phys.width  as f64 / scale;
        let lh = phys.height as f64 / scale;
        (x, y, (lw - x).max(1.0), (lh - y).max(1.0))
    } else {
        (x, y, 800.0, 600.0)
    }
}

/// Windows: build one hidden, borderless, content-area-sized service
/// `WebviewWindow` owned by the main window. WebView2 controllers only paint
/// when their webview is created on the UI thread at startup, so this is called
/// from precreate_service_windows in setup(); open_service then reveals/raises.
#[cfg(target_os = "windows")]
fn build_service_window_win(
    app: &AppHandle,
    service_id: &str,
    label: &str,
    url: &str,
    user_agent: Option<&str>,
) -> Result<(), String> {
    if app.get_webview_window(label).is_some() {
        return Ok(());
    }
    let parsed: tauri::Url = url.parse().map_err(|e| format!("{e}"))?;
    let session_dir = services::session_dir(service_id);
    std::fs::create_dir_all(&session_dir).ok();
    let (x, y, cw, ch) = content_area(app);
    let badge = BADGE_MONITOR_SCRIPT.replace("__BADGE_LABEL__", label);
    // Created hidden (visible(false)) at the content area. Created-hidden is
    // what lets a later show() from a command handler actually reveal it (a
    // window hidden after creation via the event loop won't re-show from a
    // command); occlusion is disabled (see run()) so it paints once shown.
    let wb = tauri::WebviewWindowBuilder::new(app, label.to_string(), WebviewUrl::External(parsed))
        .data_directory(session_dir)
        .initialization_script(badge)
        .initialization_script(ZOOM_SHORTCUT_SCRIPT)
        .initialization_script(AUDIO_IDLE_SCRIPT)
        .decorations(false)
        .skip_taskbar(true)
        .shadow(false)
        .visible(false)
        .inner_size(cw, ch)
        .position(x, y);
    // Own it to the main window (stays above it, closes with it). parent()
    // consumes the builder, so handle the error path explicitly.
    let mut wb = match app.get_webview_window("main") {
        Some(mw) => wb.parent(&mw).map_err(|e| e.to_string())?,
        None => wb,
    };
    if is_whatsapp_service(service_id) {
        wb = wb.initialization_script(crate::vorcaro::drivers::VORCARO_WHATSAPP_DRIVER);
    } else if is_telegram_service(service_id) {
        wb = wb.initialization_script(crate::vorcaro::drivers::VORCARO_TELEGRAM_DRIVER);
    }
    if let Some(ua) = user_agent {
        wb = wb.user_agent(ua);
    }
    wb.build().map_err(|e| e.to_string())?;
    Ok(())
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

    // Linux/other: hide the other in-window service webviews. (Windows raises
    // the active window instead — see below.)
    #[cfg(not(target_os = "windows"))]
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

    // Linux/other: show the in-window webview and bound it to the content area.
    #[cfg(not(target_os = "windows"))]
    if let Some(wv) = app.get_webview(&label) {
        wv.show().map_err(|e| e.to_string())?;
        apply_svc_bounds(&app, &wv);
    }

    #[cfg(target_os = "windows")]
    let prev_active = state.active_view.lock().unwrap().clone();

    *state.active_view.lock().unwrap() = Some(label.clone());

    // Windows: hide the previously-active service window (it has already
    // rendered, so it re-shows fine later) and show()+set_focus() the selected
    // one. show()/hide() are the only window ops that take effect from a command
    // handler, and hiding the previous window is what reliably reveals the new
    // one (set_focus alone doesn't reorder sibling owned windows).
    #[cfg(target_os = "windows")]
    {
        if let Some(prev) = prev_active {
            if prev != label {
                if let Some(pw) = app.get_webview_window(&prev) {
                    let _ = pw.hide();
                }
            }
        }
        if let Some(ww) = app.get_webview_window(&label) {
            let _ = ww.show();
            let _ = ww.set_focus();
        }
        // Showing the window isn't enough: the inner webview stays hidden
        // (SetIsVisible(false) from creation), so the rendered content doesn't
        // composite. Show the webview itself too.
        if let Some(wv) = app.get_webview(&label) {
            let _ = wv.show();
        }
    }

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
        apply_svc_bounds(&app, &wv);
    }

    Ok(())
}


/// Auto-grant notification/media permissions for service WebViews,
/// enable clipboard + HTML5 media settings, and route external links
/// (target="_blank" / window.open) to the system browser via xdg-open.
#[cfg(target_os = "linux")]
fn setup_webview_permissions(wv: &tauri::Webview) {
    let _ = wv.with_webview(|platform_wv| {
        use webkit2gtk::{
            NavigationPolicyDecision, NavigationPolicyDecisionExt, PermissionRequest,
            PermissionRequestExt, PolicyDecisionExt, PolicyDecisionType, SettingsExt,
            URIRequestExt, WebViewExt,
        };
        let inner = platform_wv.inner();

        // 1. Auto-grant all permission prompts (notifications, mic, camera, clipboard…)
        inner.connect_permission_request(|_wv, request: &PermissionRequest| {
            request.allow();
            true
        });

        // 2. Tune WebKit settings so WhatsApp/Telegram behave like a desktop browser:
        //    - JS clipboard access  → image paste in chat
        //    - HTML5 media + MediaSource + hardware accel → video playback
        if let Some(settings) = inner.settings() {
            settings.set_javascript_can_access_clipboard(true);
            settings.set_javascript_can_open_windows_automatically(true);
            settings.set_enable_media(true);
            settings.set_enable_mediasource(true);
            settings.set_enable_media_capabilities(true);
            settings.set_enable_media_stream(true);
            settings.set_enable_encrypted_media(true);
            settings.set_enable_webaudio(true);
            settings.set_enable_webgl(true);
            settings.set_enable_html5_database(true);
            settings.set_enable_html5_local_storage(true);
            settings.set_enable_smooth_scrolling(true);
            // NOTE: do not change hardware_acceleration_policy here — main.rs
            // disables DMABUF/compositing to avoid WebKitGTK rendering bugs;
            // forcing Always would re-trigger them.
        }

        // 3. target="_blank" / window.open → ignore inside the embedded view
        //    and hand the URL to the system browser.
        inner.connect_decide_policy(|_wv, decision, dtype| {
            use webkit2gtk::glib::Cast;
            if dtype != PolicyDecisionType::NewWindowAction {
                return false;
            }
            let Some(nav) = decision.dynamic_cast_ref::<NavigationPolicyDecision>() else {
                return false;
            };
            let Some(action) = nav.navigation_action() else {
                return false;
            };
            let url = action
                .request()
                .and_then(|r| r.uri())
                .map(|s| s.to_string())
                .unwrap_or_default();
            if url.starts_with("http://") || url.starts_with("https://") {
                let _ = std::process::Command::new("xdg-open").arg(&url).spawn();
            }
            decision.ignore();
            true
        });

        // 4. window.open() that bypasses decide-policy (rare) → also xdg-open.
        inner.connect_create(|_wv, action| {
            let url = action
                .request()
                .and_then(|r| r.uri())
                .map(|s| s.to_string())
                .unwrap_or_default();
            if url.starts_with("http://") || url.starts_with("https://") {
                let _ = std::process::Command::new("xdg-open").arg(&url).spawn();
            }
            None
        });

        // 5. Downloads. Telegram/WhatsApp deliver attachments via blob: URLs
        //    triggered by <a download> or window.open(blob:). decide_policy
        //    only handles http(s); blob: navigations fall through to
        //    WebContext::download-started, where we must pick a destination
        //    (default destination is empty → download fails silently).
        if let Some(ctx) = inner.context() {
            use webkit2gtk::{DownloadExt, WebContextExt};
            ctx.connect_download_started(|_ctx, download| {
                download.connect_decide_destination(|dl, suggested| {
                    let dir = dirs::download_dir()
                        .or_else(dirs::home_dir)
                        .unwrap_or_else(|| std::path::PathBuf::from("."));
                    let _ = std::fs::create_dir_all(&dir);

                    let safe = suggested.rsplit('/').next().unwrap_or("download");
                    let safe = if safe.is_empty() { "download" } else { safe };

                    let mut target = dir.join(safe);
                    if target.exists() {
                        let stem = target.file_stem()
                            .map(|s| s.to_string_lossy().into_owned())
                            .unwrap_or_else(|| "download".into());
                        let ext = target.extension()
                            .map(|s| format!(".{}", s.to_string_lossy()))
                            .unwrap_or_default();
                        for n in 1..10_000 {
                            let candidate = dir.join(format!("{stem} ({n}){ext}"));
                            if !candidate.exists() { target = candidate; break; }
                        }
                    }

                    let uri = format!("file://{}", target.to_string_lossy());
                    dl.set_allow_overwrite(false);
                    dl.set_destination(&uri);
                    true
                });
            });
        }
    });
}

#[cfg(not(target_os = "linux"))]
fn setup_webview_permissions(_wv: &tauri::Webview) {}

/// Windows: pre-create every configured service as a hidden, borderless,
/// content-area-sized WebviewWindow at boot. WebView2 controllers only
/// initialize and paint when their webview is created on the UI thread at
/// startup; a webview created later from a command leaves the controller 0x0
/// (gray/black). open_service then just shows/raises the matching window.
#[cfg(target_os = "windows")]
pub fn precreate_service_windows(app: &AppHandle) {
    let cfg = config::load();
    let catalog = services::load_catalog();
    let state: State<'_, AppState> = app.state();
    for us in cfg.services.iter().filter(|s| s.enabled) {
        // Vorcaro's local panel is created separately; skip it here.
        if is_vorcaro_panel(&us.id) { continue; }
        let Some(def) = catalog.iter().find(|d| d.id == us.service_type) else { continue };
        let label = svc_label(&us.id);
        if build_service_window_win(app, &us.id, &label, &def.url, def.user_agent.as_deref()).is_ok() {
            state.created_views.lock().unwrap().insert(label);
        }
    }
    // No boot park needed: every window is created hidden, so the shell's
    // welcome screen shows until the user opens a service.
}

/// Windows: size/position every service window onto the main window's content
/// area. Only effective on the event-loop thread, so it's called from the
/// Moved/Resized handler (set_position/set_size apply there) to keep the
/// borderless service windows pinned to the content area as the main window
/// moves and resizes. Visibility is handled separately by place_service_windows.
#[cfg(target_os = "windows")]
pub fn reposition_service_windows(app: &AppHandle) {
    let Some(main) = app.get_webview_window("main") else { return };
    let scale  = main.scale_factor().unwrap_or(1.0);
    let origin = main.outer_position().unwrap_or_default();
    let size   = main.inner_size().unwrap_or_default();
    let off_x = (crate::SIDEBAR_W as f64 * scale).round() as i32;
    let off_y = (crate::TITLEBAR_H as f64 * scale).round() as i32;
    let w = (size.width  as i32 - off_x).max(1) as u32;
    let h = (size.height as i32 - off_y).max(1) as u32;
    let pos = tauri::PhysicalPosition::new(origin.x + off_x, origin.y + off_y);
    let sz  = tauri::PhysicalSize::new(w, h);
    let state: State<'_, AppState> = app.state();
    let labels: Vec<String> = state.created_views.lock().unwrap().iter().cloned().collect();
    for lbl in labels {
        if let Some(ww) = app.get_webview_window(&lbl) {
            let _ = ww.set_position(pos);
            let _ = ww.set_size(sz);
        }
    }
}

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

    // Vorcaro's Studio is a local HTML panel — no external URL, no chat-service
    // injection scripts, no UA override. Treated like any other sidebar service
    // for sizing/visibility purposes but loaded from frontend/vorcaro/index.html.
    if is_vorcaro_panel(service_id) {
        return create_vorcaro_panel(app, state, &window, label, session_dir);
    }

    // ── Windows: pre-created at boot; this is the rare runtime fallback ─────
    // (e.g. a service added without an app restart). Build the same kind of
    // hidden borderless window; it won't paint until the next launch (runtime-
    // created WebView2 controllers stay 0x0), but it's tracked and correct.
    #[cfg(target_os = "windows")]
    {
        build_service_window_win(app, service_id, label, url, user_agent)?;
        state.created_views.lock().unwrap().insert(label.to_string());
        return Ok(());
    }

    // ── Linux / other: overlaid child webview inside the main window ────────
    #[cfg(not(target_os = "windows"))]
    {
        let parsed_url: tauri::Url = url.parse().map_err(|e| format!("{e}"))?;
        let badge_script = BADGE_MONITOR_SCRIPT.replace("__BADGE_LABEL__", label);
        let mut builder = WebviewBuilder::new(label, WebviewUrl::External(parsed_url))
            .data_directory(session_dir)
            .initialization_script(&badge_script)
            .initialization_script(ZOOM_SHORTCUT_SCRIPT);

        // WebKitGTK-only shims (notification-permission persistence, GTK
        // clipboard image paste, HE-AAC decode, blob:<video> audio drop) — work
        // around WebKitGTK 4.1 bugs; dead weight / harmful elsewhere. Linux-only.
        #[cfg(target_os = "linux")]
        {
            builder = builder
                .initialization_script(NOTIFICATION_GRANT_SCRIPT)
                .initialization_script(CLIPBOARD_IMAGE_SHIM)
                .initialization_script(CODEC_FILTER_SCRIPT)
                .initialization_script(BLOB_VIDEO_PATCH)
                .initialization_script(AUDIO_IDLE_SCRIPT);
        }

        // Vorcaro driver — only for the chat services we can actually drive.
        if is_whatsapp_service(service_id) {
            builder = builder.initialization_script(crate::vorcaro::drivers::VORCARO_WHATSAPP_DRIVER);
        } else if is_telegram_service(service_id) {
            builder = builder.initialization_script(crate::vorcaro::drivers::VORCARO_TELEGRAM_DRIVER);
        }

        if std::env::var("BB_DEBUG_CODEC").is_ok() {
            builder = builder
                .initialization_script("window.__BB_DEBUG_CODEC = true;")
                .initialization_script(CODEC_PROBE_SCRIPT);
        }

        // UA override is a WebKitGTK accommodation; WebView2 uses its native UA.
        #[cfg(target_os = "linux")]
        if let Some(ua) = user_agent {
            builder = builder.user_agent(ua);
        }
        #[cfg(not(target_os = "linux"))]
        let _ = user_agent;

        let (init_pos, init_size) = {
            let scale = window.scale_factor().unwrap_or(1.0);
            let phys  = window.inner_size().unwrap_or_default();
            let lw = phys.width  as f64 / scale;
            let lh = phys.height as f64 / scale;
            let x  = crate::SIDEBAR_W as f64;
            let y  = crate::TITLEBAR_H as f64;
            (
                LogicalPosition::new(x, y),
                LogicalSize::new((lw - x).max(1.0), (lh - y).max(1.0)),
            )
        };
        match window.add_child(builder, init_pos, init_size) {
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

/// Adjust zoom for the active service WebView.
/// `delta` semantics: > 0 zoom-in, < 0 zoom-out, 0.0 reset to 100%.
/// Returns the new zoom level as a percentage (e.g. 110 for 110%).
#[tauri::command]
#[allow(unused_variables)]
pub fn zoom_service(
    app:   AppHandle,
    state: State<'_, AppState>,
    delta: f64,
) -> Result<u32, String> {
    let label = state
        .active_view
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(|| "no active service".to_string())?;

    let mut levels = state.zoom_levels.lock().unwrap();
    let current = *levels.get(&label).unwrap_or(&1.0);
    let next = if delta == 0.0 {
        1.0
    } else {
        (current + delta).clamp(0.5, 3.0)
    };
    levels.insert(label.clone(), next);
    drop(levels);

    #[cfg(target_os = "linux")]
    if let Some(wv) = app.get_webview(&label) {
        let _ = wv.with_webview(move |platform_wv| {
            use webkit2gtk::WebViewExt;
            platform_wv.inner().set_zoom_level(next);
        });
    }

    Ok((next * 100.0).round() as u32)
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
    {
        let mut badges = state.badges.lock().unwrap();
        badges.insert(label.clone(), count);
        let has_any = badges.values().any(|&v| v > 0);
        refresh_tray_icon(&app, has_any);
    }
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
    {
        let mut badges = state.badges.lock().unwrap();
        badges.insert(label.clone(), 0);
        let has_any = badges.values().any(|&v| v > 0);
        refresh_tray_icon(&app, has_any);
    }
    app.emit("reset-badge", serde_json::json!({ "label": label }))
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn refresh_tray_icon(_app: &AppHandle, has_notifications: bool) {
    #[cfg(target_os = "linux")]
    {
        // KDE only honors the Unity LauncherEntry API for taskbar-button badges
        // (it ignores live _NET_WM_ICON changes for associated launchers). So we
        // emit that signal; KDE renders the count badge on the BigBox button.
        std::thread::spawn(move || {
            if let Ok(rt) = tokio::runtime::Builder::new_current_thread().enable_all().build() {
                let _ = rt.block_on(set_launcher_badge(has_notifications));
            }
        });
    }
    #[cfg(not(target_os = "linux"))]
    let _ = has_notifications;
}

#[cfg(target_os = "linux")]
async fn set_launcher_badge(show: bool) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use zbus::Connection;
    use zbus::zvariant::{OwnedValue, Value};
    use std::collections::HashMap;

    let conn = Connection::session().await?;

    // KDE SmartLauncherBackend listens for this signal on any path/sender.
    // Must be emitted (not called) with signature sa{sv}.
    let mut props: HashMap<String, OwnedValue> = HashMap::new();
    props.insert("count-visible".into(), Value::new(show).try_into()?);
    props.insert("count".into(), Value::new(1i64).try_into()?);

    conn.emit_signal(
        None::<&str>,
        "/com/canonical/unity/launcherentry/1",
        "com.canonical.Unity.LauncherEntry",
        "Update",
        &("application://bigbox.desktop", props),
    ).await?;

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

/// Diagnostic-only sink: lets injected JS append a line to a fixed log file
/// when the page can't expose console output. Used to capture the codec
/// detection sequence on services where DevTools isn't available.
#[tauri::command]
pub fn bb_log(line: String) {
    use std::io::Write;
    let path = "/tmp/bigbox-codec-probe.log";
    let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) else { return };
    let _ = writeln!(f, "{line}");
}

// ── Helpers ──────────────────────────────────────────────────────

fn svc_label(service_id: &str) -> String {
    format!("svc-{service_id}")
}

/// True for the built-in Vorcaro's Studio panel and any future "local://"
/// pseudo-services. The frontend catalog uses `id = "vorcaro"`.
fn is_vorcaro_panel(service_id: &str) -> bool {
    service_id == "vorcaro" || service_id.starts_with("vorcaro_")
}

/// Service ids that host a WhatsApp Web view we can scrape from.
/// Covers personal + Business plus user-renamed multi-instances.
pub(crate) fn is_whatsapp_service(service_id: &str) -> bool {
    service_id == "whatsapp"
        || service_id == "whatsapp_business"
        || service_id.starts_with("whatsapp_")
}

pub(crate) fn is_telegram_service(service_id: &str) -> bool {
    service_id == "telegram" || service_id.starts_with("telegram_")
}

/// Create Vorcaro's Studio as a local-asset WebView (frontend/vorcaro/index.html).
/// No chat-service injection scripts, no UA override, no badge monitor.
fn create_vorcaro_panel(
    app: &AppHandle,
    state: &State<'_, AppState>,
    window: &tauri::Window,
    label: &str,
    session_dir: std::path::PathBuf,
) -> Result<(), String> {
    let builder = WebviewBuilder::new(
        label,
        WebviewUrl::App("vorcaro/index.html".into()),
    )
    .data_directory(session_dir);

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
