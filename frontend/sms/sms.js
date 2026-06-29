// SPDX-License-Identifier: AGPL-3.0-or-later
// Phone SMS pane — talks to the native KDE Connect peer over Tauri IPC.
// Commands: sms_devices, sms_list_conversations, sms_load_thread, sms_send,
//           sms_request_pair, sms_accept_pair, sms_unpair.
// Events:   sms-conversations, sms-thread, sms-received, sms-device,
//           sms-pairing-request.

const invoke = (...a) => window.__TAURI__.core.invoke(...a);
const listen = (...a) => window.__TAURI__.event.listen(...a);

// ── State ────────────────────────────────────────────────
const conversations = new Map(); // threadId -> Conversation
const threads = new Map();        // threadId -> [SmsMessage]
const devices = new Map();        // deviceId -> PairedDevice
let activeThread = null;
let pendingPairDevice = null;
let searchQuery = '';

// ── Elements ─────────────────────────────────────────────
const el = (id) => document.getElementById(id);
const convoListEl = el('convo-list');
const messagesEl = el('messages');
const composerEl = el('composer');
const composeInput = el('compose-input');
const threadHeaderEl = el('thread-header');
const threadEmptyEl = el('thread-empty');
const pairingEl = el('pairing');
const deviceListEl = el('device-list');
const devicePillEl = el('device-pill');

// ── Contacts (name + photo resolution from KDE Connect vCards) ──
const contactsByNumber = new Map(); // normalized number -> { name, photo }

function normNumber(s) {
  const d = String(s || '').replace(/\D/g, '');
  return d.length > 9 ? d.slice(-9) : d; // last 9 digits, format-agnostic
}

async function loadContacts() {
  try {
    const list = await invoke('sms_contacts');
    contactsByNumber.clear();
    for (const c of list) {
      for (const num of c.numbers || []) {
        const key = normNumber(num);
        if (key) contactsByNumber.set(key, { name: c.name, photo: c.photo || null });
      }
    }
  } catch (_) {}
}

function resolveContact(address) {
  const k = normNumber(address);
  return k ? contactsByNumber.get(k) || null : null;
}

// ── Helpers ──────────────────────────────────────────────
function addrLabel(addresses) {
  if (!addresses || !addresses.length) return 'Unknown';
  return addresses
    .map((a) => {
      const c = resolveContact(a.address);
      return (c && c.name) || a.displayName || a.address || 'Unknown';
    })
    .join(', ');
}

// Photo of the conversation's first contact, if any.
function addrPhoto(addresses) {
  if (!addresses) return null;
  for (const a of addresses) {
    const c = resolveContact(a.address);
    if (c && c.photo) return c.photo;
  }
  return null;
}

function initials(label) {
  const t = (label || '?').trim();
  const m = t.match(/[A-Za-z0-9]/);
  return m ? m[0].toUpperCase() : '#';
}

function fmtTime(ms) {
  if (!ms) return '';
  const d = new Date(ms);
  const now = new Date();
  const sameDay = d.toDateString() === now.toDateString();
  return sameDay
    ? d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })
    : d.toLocaleDateString([], { month: 'short', day: 'numeric' });
}

function pairedReachableDevice() {
  for (const d of devices.values()) if (d.paired && d.reachable) return d;
  return null;
}

// ── Rendering ────────────────────────────────────────────
function renderDevicesGate() {
  const active = pairedReachableDevice();
  const anyPaired = [...devices.values()].find((d) => d.paired);
  if (active) {
    // Connected.
    pairingEl.classList.add('hidden');
    devicePillEl.textContent = active.name || 'Phone';
    devicePillEl.classList.remove('offline');
    renderConvoList();
  } else if (anyPaired) {
    // Paired but offline (phone asleep / off Wi-Fi). Keep the messages view with
    // whatever conversations are cached — don't dump the user back to the
    // install/pair screen. The pill + empty state show it's reconnecting.
    pairingEl.classList.add('hidden');
    devicePillEl.textContent = `${anyPaired.name || 'Phone'} · offline`;
    devicePillEl.classList.add('offline');
    renderConvoList();
  } else {
    // Nothing paired — show the install/pair overlay.
    pairingEl.classList.remove('hidden');
    renderPairList();
    devicePillEl.textContent = '';
  }
}

function renderPairList() {
  const list = [...devices.values()];
  if (!list.length) {
    deviceListEl.innerHTML =
      '<p class="muted searching">Searching for phones on your network…</p>';
    return;
  }
  deviceListEl.innerHTML = '';
  for (const d of list) {
    const row = document.createElement('div');
    row.className = 'device-row';
    const state = d.paired
      ? d.reachable ? 'Paired' : 'Paired (offline)'
      : d.reachable ? 'Available' : 'Offline';
    row.innerHTML =
      `<div><div class="name"></div><div class="state muted"></div></div>`;
    row.querySelector('.name').textContent = d.name || d.deviceId;
    row.querySelector('.state').textContent = state;

    const btn = document.createElement('button');
    btn.className = 'btn primary';
    if (d.paired) {
      btn.textContent = 'Unpair';
      btn.onclick = () => invoke('sms_unpair', { deviceId: d.deviceId });
    } else {
      btn.textContent = 'Pair';
      btn.disabled = !d.reachable;
      btn.onclick = () => {
        btn.disabled = true;
        btn.textContent = 'Accept on phone…';
        invoke('sms_request_pair', { deviceId: d.deviceId });
      };
    }
    row.appendChild(btn);
    deviceListEl.appendChild(row);
  }
}

function renderConvoList() {
  let list = [...conversations.values()].sort((a, b) => b.date - a.date);
  const q = searchQuery.trim().toLowerCase();
  if (q) {
    list = list.filter((c) => {
      const label = addrLabel(c.addresses).toLowerCase();
      const nums = (c.addresses || []).map((a) => a.address).join(' ').toLowerCase();
      const snip = (c.snippet || '').toLowerCase();
      return label.includes(q) || nums.includes(q) || snip.includes(q);
    });
  }
  convoListEl.innerHTML = '';
  if (list.length === 0) {
    const offline = !pairedReachableDevice() && [...devices.values()].some((d) => d.paired);
    const msg = q
      ? 'No matches'
      : offline
        ? '📴 Phone offline — reconnecting…'
        : 'Loading conversations…';
    convoListEl.innerHTML = `<p class="muted empty-note">${msg}</p>`;
    return;
  }
  for (const c of list) {
    const label = addrLabel(c.addresses);
    const item = document.createElement('div');
    item.className = 'convo-item' + (c.threadId === activeThread ? ' active' : '') +
      (!c.read && !c.fromMe ? ' unread' : '');
    item.onclick = () => openThread(c.threadId);

    const avatar = document.createElement('div');
    avatar.className = 'avatar';
    const photo = addrPhoto(c.addresses);
    if (photo) {
      avatar.classList.add('photo');
      avatar.style.backgroundImage = `url("${photo}")`;
    } else {
      avatar.textContent = initials(label);
    }

    const body = document.createElement('div');
    body.className = 'convo-body';
    const snippet = (c.fromMe ? 'You: ' : '') + (c.snippet || '');
    body.innerHTML =
      `<div class="convo-top"><span class="convo-name"></span><span class="convo-time"></span></div>` +
      `<div class="convo-snippet"></div>`;
    body.querySelector('.convo-name').textContent = label;
    body.querySelector('.convo-time').textContent = fmtTime(c.date);
    body.querySelector('.convo-snippet').textContent = snippet;

    item.append(avatar, body);
    convoListEl.appendChild(item);
  }
}

function renderThread() {
  if (activeThread == null) {
    threadHeaderEl.classList.add('hidden');
    messagesEl.classList.add('hidden');
    composerEl.classList.add('hidden');
    threadEmptyEl.classList.remove('hidden');
    return;
  }
  threadEmptyEl.classList.add('hidden');
  threadHeaderEl.classList.remove('hidden');
  messagesEl.classList.remove('hidden');
  composerEl.classList.remove('hidden');

  const conv = conversations.get(activeThread);
  const label = conv ? addrLabel(conv.addresses) : 'Conversation';
  el('thread-name').textContent = label;
  el('thread-addr').textContent =
    conv && conv.addresses && conv.addresses[0] ? conv.addresses[0].address : '';

  const msgs = (threads.get(activeThread) || []).slice().sort((a, b) => a.date - b.date);
  messagesEl.innerHTML = '';
  for (const m of msgs) {
    const b = document.createElement('div');
    b.className = 'bubble ' + (m.fromMe ? 'me' : 'them');
    const text = document.createTextNode(m.body || '');
    b.appendChild(text);
    if (m.attachmentCount > 0) {
      const a = document.createElement('span');
      a.className = 'attach';
      a.textContent = `📎 ${m.attachmentCount} attachment${m.attachmentCount > 1 ? 's' : ''}`;
      b.appendChild(a);
    }
    const meta = document.createElement('span');
    meta.className = 'meta';
    meta.textContent = fmtTime(m.date);
    b.appendChild(meta);
    messagesEl.appendChild(b);
  }
  messagesEl.scrollTop = messagesEl.scrollHeight;
}

// ── Actions ──────────────────────────────────────────────
function openThread(threadId) {
  activeThread = threadId;
  const conv = conversations.get(threadId);
  if (conv) conv.read = true;
  renderConvoList();
  renderThread();
  invoke('sms_load_thread', { threadId });
}

function sendCurrent() {
  const body = composeInput.value.trim();
  if (!body || activeThread == null) return;
  const conv = conversations.get(activeThread);
  const addresses = conv ? conv.addresses.map((a) => a.address) : [];
  if (!addresses.length) return;

  invoke('sms_send', { addresses, body });

  // Optimistic local echo.
  const msg = {
    threadId: activeThread,
    fromMe: true,
    body,
    date: Date.now(),
    read: true,
    addresses: conv.addresses,
    attachmentCount: 0,
  };
  const arr = threads.get(activeThread) || [];
  arr.push(msg);
  threads.set(activeThread, arr);
  if (conv) { conv.snippet = body; conv.date = msg.date; conv.fromMe = true; }
  composeInput.value = '';
  composeInput.style.height = 'auto';
  renderThread();
  renderConvoList();
}

// ── Event wiring ─────────────────────────────────────────
function mergeConversations(list) {
  for (const c of list) conversations.set(c.threadId, c);
  renderConvoList();
}

function setThread(threadId, messages) {
  threads.set(threadId, messages);
  if (threadId === activeThread) renderThread();
}

// Draggable divider to resize the conversation list (persisted).
function initResizer() {
  const resizer = el('resizer');
  const pane = document.querySelector('.convo-pane');
  if (!resizer || !pane) return;
  const saved = parseInt(localStorage.getItem('sms-convo-width') || '', 10);
  if (saved >= 240 && saved <= 620) pane.style.width = saved + 'px';

  let dragging = false;
  resizer.addEventListener('mousedown', (e) => {
    dragging = true;
    resizer.classList.add('dragging');
    document.body.style.userSelect = 'none';
    e.preventDefault();
  });
  window.addEventListener('mousemove', (e) => {
    if (!dragging) return;
    const w = Math.min(Math.max(e.clientX, 240), 620);
    pane.style.width = w + 'px';
  });
  window.addEventListener('mouseup', () => {
    if (!dragging) return;
    dragging = false;
    resizer.classList.remove('dragging');
    document.body.style.userSelect = '';
    localStorage.setItem('sms-convo-width', String(parseInt(pane.style.width, 10)));
  });
}

async function init() {
  initResizer();
  const searchEl = el('convo-search');
  if (searchEl) {
    searchEl.addEventListener('input', () => {
      searchQuery = searchEl.value;
      renderConvoList();
    });
  }
  el('send-btn').onclick = sendCurrent;
  el('back-btn').onclick = () => { activeThread = null; renderConvoList(); renderThread(); };

  composeInput.addEventListener('keydown', (e) => {
    if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); sendCurrent(); }
  });
  composeInput.addEventListener('input', () => {
    composeInput.style.height = 'auto';
    composeInput.style.height = Math.min(composeInput.scrollHeight, 120) + 'px';
  });

  el('pair-accept').onclick = () => {
    if (pendingPairDevice) invoke('sms_accept_pair', { deviceId: pendingPairDevice });
    el('pair-request').classList.add('hidden');
  };
  el('pair-dismiss').onclick = () => el('pair-request').classList.add('hidden');

  await listen('sms-device', ({ payload }) => {
    devices.set(payload.deviceId, payload);
    renderDevicesGate();
    if (payload.paired && payload.reachable) invoke('sms_list_conversations');
  });
  await listen('sms-conversations', ({ payload }) => mergeConversations(payload));
  await listen('sms-thread', ({ payload }) => setThread(payload.threadId, payload.messages));
  await listen('sms-received', ({ payload }) => {
    const arr = threads.get(payload.threadId) || [];
    arr.push(payload);
    threads.set(payload.threadId, arr);
    // Refresh the list snippet/order from the source of truth.
    invoke('sms_list_conversations');
    if (payload.threadId === activeThread) renderThread();
  });
  await listen('sms-pairing-request', ({ payload }) => {
    pendingPairDevice = payload.deviceId;
    el('pair-request-text').textContent =
      `${payload.name || 'A phone'} wants to pair with BigBox.`;
    el('pair-request').classList.remove('hidden');
  });

  // Initial pull.
  await loadContacts();
  try {
    const list = await invoke('sms_devices');
    for (const d of list) devices.set(d.deviceId, d);
  } catch (_) {}
  renderDevicesGate();
  invoke('sms_list_conversations');

  // Contacts sync asynchronously after the phone grants the Contacts
  // permission — reload periodically and re-render names/photos.
  setInterval(async () => {
    await loadContacts();
    renderConvoList();
  }, 15000);

  // While disconnected, keep polling for the device so the pane recovers on its
  // own when KDE Connect reachability flaps (no sms-device event may arrive).
  setInterval(async () => {
    if (pairedReachableDevice()) return;
    try {
      const list = await invoke('sms_devices');
      for (const d of list) devices.set(d.deviceId, d);
      renderDevicesGate();
      if (pairedReachableDevice()) invoke('sms_list_conversations');
    } catch (_) {}
  }, 3000);
}

init();
