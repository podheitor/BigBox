// BigBox shell — service manager
// Requires: window.__TAURI__ (Tauri v2, withGlobalTauri: true)

const invoke = (...args) => window.__TAURI__.core.invoke(...args);

// ── State ──────────────────────────────────────────────────────────
let catalog = [];
let config  = { services: [], muted: false };
let activeId = null;
let dialogWasActive = null;
const badges = {};   // label -> unread count
let preloadDone = false;

const LAST_ACTIVE_SERVICE_KEY = 'bigbox.lastActiveServiceId';
const OPEN_SERVICE_METRICS_KEY = 'bigbox.openServiceMetrics';

// ── Boot ─────────────────────────────────────────────────────────
window.addEventListener('DOMContentLoaded', async () => {
    try {
        [config, catalog] = await Promise.all([
            invoke('get_config'),
            invoke('get_catalog'),
        ]);
    } catch (e) {
        console.error('Failed to init:', e);
        return;
    }

    renderSidebar();
    bindEvents();
    initTitlebar();
    initAbout();
    initBadgeListener();
    initRemovalListener();

    if (config.muted) setMuteUI(true);
    // Welcome screen stays visible — hidden only when user selects a service
});

// ── Sidebar rendering ─────────────────────────────────────────────


// ── About window ──────────────────────────────────────
function initAbout() {
  document.getElementById('btn-about').addEventListener('click', async () => {
    try {
      await invoke('open_about');
    } catch (err) {
      console.error('open_about failed:', err);
    }
  });
}





function saveLastActiveServiceId(serviceId) {
  try {
    window.localStorage.setItem(LAST_ACTIVE_SERVICE_KEY, serviceId);
  } catch (_) { }
}

function getLastActiveServiceId() {
  try {
    return window.localStorage.getItem(LAST_ACTIVE_SERVICE_KEY);
  } catch (_) {
    return null;
  }
}

function getOpenServiceMetrics() {
  try {
    const raw = window.localStorage.getItem(OPEN_SERVICE_METRICS_KEY);
    const parsed = raw ? JSON.parse(raw) : {};
    if (!parsed || typeof parsed !== 'object') return {};
    return parsed;
  } catch (_) {
    return {};
  }
}

function clearLastActiveServiceIfMatches(serviceId) {
  if (getLastActiveServiceId() !== serviceId) return;
  try {
    window.localStorage.removeItem(LAST_ACTIVE_SERVICE_KEY);
  } catch (_) { }
}

function trackOpenServiceDuration(serviceId, elapsedMs) {
  try {
    const parsed = getOpenServiceMetrics();
    const prev = parsed[serviceId] || { count: 0, avgMs: 0 };
    const count = prev.count + 1;
    const avgMs = Math.round(((prev.avgMs * prev.count) + elapsedMs) / count);
    parsed[serviceId] = { count, avgMs };
    window.localStorage.setItem(OPEN_SERVICE_METRICS_KEY, JSON.stringify(parsed));
  } catch (_) { }
}


// Sequential preload: after first service opened, preload remaining in background
const PRELOAD_INTER_STEP_MS = 2000;

async function preloadRemainingServices(excludeId) {
  if (preloadDone) return;
  preloadDone = true;

  const remaining = config.services.filter(s => s.id !== excludeId);
  if (remaining.length === 0) return;

  // Small delay before starting preload to let first service settle
  await new Promise(r => setTimeout(r, 10000));

  for (const svc of remaining) {
    const def = catalog.find(c => c.id === svc.service_type);
    if (!def) continue;

    try {
      await invoke('preload_service', {
        serviceId: svc.id,
        url: def.url,
        userAgent: def.user_agent_override || null,
      });
    } catch (e) {
      console.warn('preload_service skip:', svc.id, e);
    }

    await new Promise(r => setTimeout(r, PRELOAD_INTER_STEP_MS));
  }
  console.info('preload complete — all services warm');
}

// ── Badge updates from service WebViews ──────────────
async function initBadgeListener() {
  const { listen } = window.__TAURI__.event;
  await listen('badge-update', ({ payload }) => {
    badges[payload.label] = payload.count;
    const badgeEl = document.querySelector(`.badge[data-label="${payload.label}"]`);
    if (!badgeEl) return;
    if (payload.count > 0) {
      badgeEl.textContent = payload.count > 99 ? '99+' : payload.count;
      badgeEl.classList.add('visible');
    } else {
      badgeEl.classList.remove('visible');
    }
  });

  await listen('reset-badge', ({ payload }) => {
    badges[payload.label] = 0;
    const badgeEl = document.querySelector(`.badge[data-label="${payload.label}"]`);
    if (badgeEl) badgeEl.classList.remove('visible');
  });
}

// ── Right-click → native GTK menu (always on top of WebViews) ──
let ctxMenuPending = false;

async function showCtxMenu(e, svcId) {
  e.preventDefault();
  e.stopPropagation();
  if (ctxMenuPending) return;
  ctxMenuPending = true;
  setTimeout(() => { ctxMenuPending = false; }, 300);

  // Invoke native GTK context menu
  try {
    await invoke('show_service_menu', { id: svcId, x: e.clientX, y: e.clientY });
  } catch (err) {
    console.error('show_service_menu error:', err);
  }
}

// Listen for service removal from native menu
async function initRemovalListener() {
  const { listen } = window.__TAURI__.event;
  await listen('service-removed', ({ payload }) => {
    if (payload.id === activeId) {
      activeId = null;
    }
    clearLastActiveServiceIfMatches(payload.id);
    // Refresh config and sidebar
    invoke('get_config').then(newConfig => {
      config = newConfig;
      renderSidebar();
    }).catch(console.error);
  });
}

// ── Titlebar window controls ──────────────────────────
async function initTitlebar() {
  const { getCurrentWindow } = window.__TAURI__.window;
  const win = getCurrentWindow();

  document.getElementById('btn-minimize').addEventListener('click', () => win.minimize());
  document.getElementById('btn-maximize').addEventListener('click', () => win.toggleMaximize());
  document.getElementById('btn-close').addEventListener('click', () => win.close());
}

function renderSidebar() {
    const list = document.getElementById('service-list');
    list.innerHTML = '';
    config.services.forEach(svc => {
        const def   = catalog.find(c => c.id === svc.service_type) || {};
        const color = def.color || '#6c7086';
        const init  = (svc.display_name[0] || '?').toUpperCase();

        const li = document.createElement('li');
        li.className = 'service-item';
        li.dataset.id = svc.id;
        if (svc.id === activeId) li.classList.add('active');

        // Icon-only: avatar circle, no service name text
        const label = `svc-${svc.id}`;
        const badgeCount = badges[label] || 0;
        li.innerHTML = `
          <div class="service-btn" role="button" tabindex="0" title="${svc.display_name}">
            <span class="avatar" style="background:${color}">${init}</span>
            <span class="badge${badgeCount > 0 ? ' visible' : ''}" data-label="${label}">
              ${badgeCount > 99 ? '99+' : badgeCount}
            </span>
          </div>`;

        const btn = li.querySelector('.service-btn');
        // click/drag: down per-item (move/up delegated to document)
        li.addEventListener('pointerdown', onPtrDown);
        // backup: contextmenu fires after pointerdown on right-click
        li.addEventListener('contextmenu', (e) => {
            e.preventDefault();
            e.stopPropagation();
            showCtxMenu(e, li.dataset.id);
        });

        list.appendChild(li);
    });
}

// ── Service selection ─────────────────────────────────────────────
async function selectService(id) {
    if (id === activeId) return;

    const svc = config.services.find(s => s.id === id);
    if (!svc) return;
    const def = catalog.find(c => c.id === svc.service_type);
    if (!def) return;

    activeId = id;
    document.getElementById('welcome').classList.add('hidden');

    document.querySelectorAll('.service-item').forEach(el => {
        el.classList.toggle('active', el.dataset.id === id);
    });

    const startedAt = performance.now();
    try {
        await invoke('open_service', {
            serviceId: id,
            url: def.url,
            userAgent: def.user_agent_override || null,
        });
        saveLastActiveServiceId(id);
        // After first service fully loaded, preload remaining in background
        preloadRemainingServices(id).catch(e =>
            console.warn('background preload error:', e));
    } catch (e) {
        console.error('open_service error:', e);
    } finally {
        trackOpenServiceDuration(id, performance.now() - startedAt);
    }
}

// ── Event bindings ────────────────────────────────────────────────
function bindEvents() {
    document.getElementById('btn-mute').addEventListener('click', async () => {
        const next = !config.muted;
        config.muted = next;
        setMuteUI(next);
        await invoke('set_muted', { muted: next }).catch(console.error);
    });

    document.getElementById('btn-add').addEventListener('click', openDialog);
    document.getElementById('btn-welcome-add').addEventListener('click', openDialog);

    document.getElementById('btn-dialog-close').addEventListener('click', closeDialog);
    const dlg = document.getElementById('dialog-add');
    dlg.addEventListener('click', e => { if (e.target === dlg) closeDialog(); });
}

// ── Mute UI ───────────────────────────────────────────────────────
function setMuteUI(muted) {
    const btn = document.getElementById('btn-mute');
    btn.dataset.muted = String(muted);
    document.getElementById('icon-sound').style.display = muted ? 'none' : '';
    document.getElementById('icon-muted').style.display = muted ? ''     : 'none';
}

// ── Add / Manage dialog ───────────────────────────────────────────
async function openDialog() {
    dialogWasActive = activeId;

    if (activeId) {
        await invoke('hide_service').catch(console.error);
    }

    // Expand shell to full width so dialog renders at proper viewport size
    await invoke('expand_shell').catch(console.error);
    // Brief wait for GTK layout to propagate to WebKit viewport
    await new Promise(r => setTimeout(r, 60));

    renderCatalog();
    renderActiveList();

    const secActive = document.getElementById('section-active');
    secActive.style.display = config.services.length > 0 ? '' : 'none';

    document.getElementById('dialog-add').showModal();
}

async function closeDialog() {
    document.getElementById('dialog-add').close();

    if (dialogWasActive) {
        const id = dialogWasActive;
        dialogWasActive = null;
        const svc = config.services.find(s => s.id === id);
        const def = svc && catalog.find(c => c.id === svc.service_type);
        if (def) {
            try {
                // open_service internally collapses shell + shows service
                await invoke('open_service', {
                    serviceId: id,
                    url: def.url,
                    userAgent: def.user_agent_override || null,
                });
                saveLastActiveServiceId(id);
            } catch (e) {
                console.error('re-open service error:', e);
            }
        }
    }
    // If no active service to restore, shell stays expanded (welcome visible)
}

function renderCatalog() {
    const grid = document.getElementById('catalog-grid');
    grid.innerHTML = '';
    catalog.forEach(def => {
        const btn = document.createElement('button');
        btn.className = 'catalog-item';
        const initial = def.name ? def.name[0] : '?';
        btn.innerHTML = `
          <span class="catalog-avatar" style="background:${def.color}">${initial}</span>
          <span>${def.name}</span>`;
        btn.addEventListener('click', () => addService(def));
        grid.appendChild(btn);
    });
}

function renderActiveList() {
    const list = document.getElementById('active-list');
    list.innerHTML = '';
    config.services.forEach(svc => {
        const def   = catalog.find(c => c.id === svc.service_type) || {};
        const color = def.color || '#6c7086';
        const init  = (svc.display_name[0] || '?').toUpperCase();

        const li = document.createElement('li');
        li.className = 'active-item';
        li.innerHTML = `
          <span class="avatar" style="background:${color}">${init}</span>
          <span class="svc-name">${svc.display_name}</span>
          <button class="remove-btn" title="Remove">
            <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor">
              <path d="M19 6.41L17.59 5 12 10.59 6.41 5 5 6.41 10.59 12 5 17.59 6.41 19 12 13.41 17.59 19 19 17.59 13.41 12z"/>
            </svg>
          </button>`;
        li.querySelector('.remove-btn').addEventListener('click', () => removeService(svc.id));
        list.appendChild(li);
    });
}

async function addService(def) {
    try {
        config = await invoke('add_service', {
            serviceType: def.id,
            displayName: def.name,
        });
        renderSidebar();
        renderActiveList();
        document.getElementById('section-active').style.display = '';
    } catch (e) {
        console.error('add_service error:', e);
    }
}

async function removeService(id) {
    if (id === activeId) {
        activeId = null;
        dialogWasActive = null;
    }
    try {
        config = await invoke('remove_service', { id });
        clearLastActiveServiceIfMatches(id);
        renderSidebar();
        renderActiveList();
        const secActive = document.getElementById('section-active');
        secActive.style.display = config.services.length > 0 ? '' : 'none';
        if (config.services.length === 0) {
            document.getElementById('welcome').classList.remove('hidden');
        }
    } catch (e) {
        console.error('remove_service error:', e);
    }
}

// ── Pointer-based reorder (Wayland-safe; no setPointerCapture) ──────────────
let pDrag = null;

function onPtrDown(e) {
  if (e.button !== 0) return; // ignore right-click here (handled by contextmenu)
  e.preventDefault(); // prevent text selection + default drag gestures
  pDrag = { srcId: e.currentTarget.dataset.id, startY: e.clientY, moved: false };
  document.addEventListener('pointermove', onPtrMove);
  document.addEventListener('pointerup',   onPtrUp);
  document.addEventListener('pointercancel', onPtrCancel);
}

function onPtrMove(e) {
  if (!pDrag) return;
  if (!pDrag.moved && Math.abs(e.clientY - pDrag.startY) > 8) pDrag.moved = true;
  if (pDrag.moved) {
    document.querySelectorAll('.service-item').forEach(l =>
      l.classList.remove('drag-over', 'dragging', 'drop-above', 'drop-below'));
    document.querySelector(`.service-item[data-id="${pDrag.srcId}"]`)?.classList.add('dragging');
    const over = document.elementFromPoint(32, e.clientY)?.closest('.service-item');
    if (over && over.dataset.id !== pDrag.srcId) {
      const rect = over.getBoundingClientRect();
      const mid = rect.top + rect.height / 2;
      over.classList.add(e.clientY < mid ? 'drop-above' : 'drop-below');
    }
  }
}

function onPtrUp(e) {
  document.removeEventListener('pointermove', onPtrMove);
  document.removeEventListener('pointerup',   onPtrUp);
  document.removeEventListener('pointercancel', onPtrCancel);
  if (!pDrag) return;
  const { srcId, moved } = pDrag;
  pDrag = null;
  const over = document.elementFromPoint(32, e.clientY)?.closest('.service-item');
  const insertBefore = over?.classList.contains('drop-above');
  document.querySelectorAll('.service-item').forEach(l =>
    l.classList.remove('dragging', 'drag-over', 'drop-above', 'drop-below'));
  if (moved) {
    if (over && over.dataset.id !== srcId)
      reorderServices(srcId, over.dataset.id, insertBefore);
  } else {
    selectService(srcId);
  }
}

function onPtrCancel() {
  document.removeEventListener('pointermove', onPtrMove);
  document.removeEventListener('pointerup',   onPtrUp);
  document.removeEventListener('pointercancel', onPtrCancel);
  pDrag = null;
  document.querySelectorAll('.service-item').forEach(l =>
    l.classList.remove('dragging', 'drag-over', 'drop-above', 'drop-below'));
}


async function reorderServices(srcId, tgtId, insertBefore = false) {
    const items = Array.from(document.querySelectorAll('.service-item'));
    // Capture first positions (FLIP: First)
    const firstPositions = new Map();
    items.forEach(item => {
        firstPositions.set(item.dataset.id, item.getBoundingClientRect().top);
    });

    const ids = config.services.map(s => s.id);
    const si  = ids.indexOf(srcId);
    let ti    = ids.indexOf(tgtId);
    if (si < 0 || ti < 0) return;
    ids.splice(si, 1);
    // Recalc ti after removal; adjust for insert-above vs insert-below
    ti = ids.indexOf(tgtId);
    ids.splice(insertBefore ? ti : ti + 1, 0, srcId);
    try {
        config = await invoke('reorder_services', { ids });
        renderSidebar();
        
        // FLIP: Last - get new positions, then Invert and Play
        const lastPositions = new Map();
        const newItems = Array.from(document.querySelectorAll('.service-item'));
        newItems.forEach(item => {
            lastPositions.set(item.dataset.id, item.getBoundingClientRect().top);
        });

        // Calculate deltas and animate
        const animations = [];
        newItems.forEach(item => {
            const id = item.dataset.id;
            const first = firstPositions.get(id);
            const last = lastPositions.get(id);
            if (first !== undefined && last !== undefined && first !== last) {
                const delta = first - last;
                // Invert: move item back to its original position
                item.style.transform = `translateY(${delta}px)`;
                item.style.transition = 'none';
                
                // Play: animate to final position
                animations.push(() => {
                    requestAnimationFrame(() => {
                        item.style.transition = 'transform 0.25s cubic-bezier(0.2, 0, 0, 1)';
                        item.style.transform = '';
                    });
                });
            }
        });

        // Trigger animations in next frame
        requestAnimationFrame(() => {
            animations.forEach(fn => fn());
        });

        // Clean up after animation completes
        setTimeout(() => {
            newItems.forEach(item => {
                item.style.transition = '';
                item.style.transform = '';
            });
        }, 300);
    } catch (e) {
        console.error('reorder_services error:', e);
    }
}
