// Vorcaro's Studio — Phase A frontend (vanilla JS)
// Talks to the Rust backend via Tauri IPC (window.__TAURI__.core.invoke).

const invoke = (...args) => window.__TAURI__.core.invoke(...args);
const NIL_UUID = '00000000-0000-0000-0000-000000000000';

const PLATFORM_LABEL = {
  whatsapp_web:           'WhatsApp',
  whatsapp_business_web:  'WhatsApp Business',
  telegram:               'Telegram',
};

const state = {
  contacts: [],
  lists: [],
  campaigns: [],
  settings: {},
  workspaces: [],   // [{id, display_name, platform}], from vorcaro_list_workspaces
  selectedIds: new Set(),
  selectedListId: null,
  searchQuery: '',
  tagFilter: '',
  // scrape session
  scrapePlatform: null,
  scrapeRows: [],
  scrapeSelected: new Set(),
  scrapeFilter: '',
};

// ── Boot ─────────────────────────────────────────────────────────
window.addEventListener('DOMContentLoaded', async () => {
  try {
    const [snap, workspaces] = await Promise.all([
      invoke('vorcaro_get_state'),
      invoke('vorcaro_list_workspaces'),
    ]);
    Object.assign(state, {
      contacts: snap.contacts || [],
      lists: snap.lists || [],
      campaigns: snap.campaigns || [],
      settings: snap.settings || {},
      workspaces: workspaces || [],
    });
  } catch (e) {
    toast('Erro carregando estado: ' + e, 'error');
    console.error(e);
  }

  bindTabs();
  bindContactsTab();
  bindListsTab();
  bindSettingsTab();
  bindScrape();
  bindCampaign();
  bindLogs();
  bindCloudApi();
  listenCampaignProgress();

  renderContacts();
  renderListsSide();
  fillSettingsForm();
  await fillCloudApiForm();
  fillWorkspaceSelectors();
  renderCampaignForm();
  renderCampaignRecent();
  renderLogsCampaignSelect();
});

const PLATFORM_SHORT = {
  whatsapp_web: 'WA',
  whatsapp_business_web: 'WA-B',
  telegram: 'TG',
  whatsapp_cloud_api: 'Cloud',
};

function platformLabelOf(ws) {
  return PLATFORM_SHORT[ws.platform] || ws.platform;
}

function fillWorkspaceSelectors() {
  // Scrape picker (Contatos toolbar)
  const scrapeSel = document.getElementById('scrape-workspace');
  if (scrapeSel) {
    if (state.workspaces.length === 0) {
      scrapeSel.innerHTML = '<option value="">— nenhum WhatsApp/Telegram adicionado —</option>';
      document.getElementById('btn-scrape').disabled = true;
    } else {
      scrapeSel.innerHTML = state.workspaces.map(w =>
        `<option value="${esc(w.id)}">${esc(w.display_name)} (${platformLabelOf(w)})</option>`
      ).join('');
      document.getElementById('btn-scrape').disabled = false;
    }
  }

  // Campaign workspace picker (refreshed when platform changes too)
  refreshCampaignWorkspaces();
}

function refreshCampaignWorkspaces() {
  const sel = document.getElementById('camp-workspace');
  if (!sel) return;
  const platform = document.getElementById('camp-platform').value;
  // Cloud API has no workspace concept; hide the row entirely.
  const row = document.getElementById('camp-workspace-row');
  if (platform === 'whatsapp_cloud_api') {
    row.hidden = true;
    return;
  }
  row.hidden = false;

  // The Vorcaro Platform decides which contact field to use as the recipient
  // (whatsapp vs whatsapp_business vs telegram), not which BigBox slot to
  // drive. Both WhatsApp variants accept any whatsapp-flavored workspace —
  // the same WA Web DOM works either way; users often log a Business account
  // into a "whatsapp" slot.
  let matching;
  if (platform === 'whatsapp_web' || platform === 'whatsapp_business_web') {
    matching = state.workspaces.filter(w =>
      w.platform === 'whatsapp_web' || w.platform === 'whatsapp_business_web'
    );
  } else {
    matching = state.workspaces.filter(w => w.platform === platform);
  }

  if (matching.length === 0) {
    sel.innerHTML = `<option value="">— nenhum ${PLATFORM_SHORT[platform] || platform} adicionado em BigBox —</option>`;
  } else {
    sel.innerHTML = matching.map(w =>
      `<option value="${esc(w.id)}">${esc(w.display_name)}</option>`
    ).join('');
  }
}

// ── Tab navigation ───────────────────────────────────────────────
function bindTabs() {
  document.querySelectorAll('.tab').forEach(t => {
    t.addEventListener('click', () => {
      if (t.classList.contains('disabled')) return;
      document.querySelectorAll('.tab').forEach(x => x.classList.remove('active'));
      document.querySelectorAll('.panel').forEach(p => p.classList.remove('active'));
      t.classList.add('active');
      document.querySelector(`.panel[data-panel="${t.dataset.tab}"]`).classList.add('active');
    });
  });
}

// ── Toast ────────────────────────────────────────────────────────
let toastTimer = null;
function toast(msg, kind = '') {
  const el = document.getElementById('toast');
  el.textContent = msg;
  el.className = 'toast ' + kind;
  el.hidden = false;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => { el.hidden = true; }, 2400);
}

// ── CONTACTS TAB ─────────────────────────────────────────────────
function bindContactsTab() {
  document.getElementById('contact-search').addEventListener('input', e => {
    state.searchQuery = e.target.value.toLowerCase();
    renderContacts();
  });
  document.getElementById('contact-tag-filter').addEventListener('change', e => {
    state.tagFilter = e.target.value;
    renderContacts();
  });

  document.getElementById('btn-new-contact').addEventListener('click', () => openContactDialog(null));
  document.getElementById('btn-contact-cancel').addEventListener('click',
    () => document.getElementById('dlg-contact').close());
  document.getElementById('contact-form').addEventListener('submit', onContactFormSubmit);

  document.getElementById('btn-import-csv').addEventListener('click',
    () => document.getElementById('csv-file').click());
  document.getElementById('csv-file').addEventListener('change', onCsvFile);

  document.getElementById('check-all').addEventListener('change', e => {
    state.selectedIds.clear();
    if (e.target.checked) {
      filteredContacts().forEach(c => state.selectedIds.add(c.id));
    }
    renderContacts();
  });

  document.getElementById('btn-bulk-delete').addEventListener('click', onBulkDelete);
  document.getElementById('btn-bulk-tag').addEventListener('click', onBulkTag);
  document.getElementById('btn-bulk-untag').addEventListener('click', onBulkUntag);
  document.getElementById('btn-bulk-add-list').addEventListener('click', onBulkAddToList);
}

function filteredContacts() {
  const q = state.searchQuery;
  const tag = state.tagFilter;
  return state.contacts.filter(c => {
    if (tag && !(c.tags || []).includes(tag)) return false;
    if (!q) return true;
    const blob = [
      c.display_name, c.whatsapp, c.whatsapp_business, c.telegram,
      ...(c.tags || []),
    ].filter(Boolean).join(' ').toLowerCase();
    return blob.includes(q);
  });
}

function allTags() {
  const set = new Set();
  state.contacts.forEach(c => (c.tags || []).forEach(t => set.add(t)));
  return [...set].sort();
}

function renderContacts() {
  // Tag filter dropdown
  const tagSel = document.getElementById('contact-tag-filter');
  const tags = allTags();
  const current = tagSel.value;
  tagSel.innerHTML = '<option value="">Todas as tags</option>' +
    tags.map(t => `<option value="${esc(t)}">${esc(t)}</option>`).join('');
  tagSel.value = tags.includes(current) ? current : '';

  // Body
  const tbody = document.getElementById('contacts-body');
  const rows = filteredContacts();
  tbody.innerHTML = rows.map(c => {
    const checked = state.selectedIds.has(c.id) ? 'checked' : '';
    const chips = (c.tags || []).map(t => `<span class="chip">${esc(t)}</span>`).join('');
    return `
      <tr data-id="${c.id}">
        <td class="col-check"><input type="checkbox" class="row-check" ${checked}></td>
        <td>${esc(c.display_name || '')}</td>
        <td>${esc(c.whatsapp || '')}</td>
        <td>${esc(c.whatsapp_business || '')}</td>
        <td>${esc(c.telegram || '')}</td>
        <td>${chips}</td>
        <td class="col-actions">
          <div class="row-actions">
            <button class="btn btn-small btn-edit">Editar</button>
            <button class="btn btn-small btn-danger btn-del">Apagar</button>
          </div>
        </td>
      </tr>`;
  }).join('');

  tbody.querySelectorAll('tr').forEach(tr => {
    const id = tr.dataset.id;
    tr.querySelector('.row-check').addEventListener('change', e => {
      if (e.target.checked) state.selectedIds.add(id);
      else state.selectedIds.delete(id);
      updateBulkBar();
    });
    tr.querySelector('.btn-edit').addEventListener('click', () => openContactDialog(id));
    tr.querySelector('.btn-del').addEventListener('click', () => deleteContact(id));
  });

  document.getElementById('contacts-empty').hidden = state.contacts.length > 0;
  document.getElementById('check-all').checked =
    rows.length > 0 && rows.every(c => state.selectedIds.has(c.id));
  updateBulkBar();
}

function updateBulkBar() {
  const n = state.selectedIds.size;
  const bar = document.getElementById('bulk-bar');
  bar.hidden = n === 0;
  document.getElementById('bulk-count').textContent =
    `${n} selecionado${n === 1 ? '' : 's'}`;
  // Campaign tab's ad-hoc count mirrors this set.
  const adhocCount = document.getElementById('camp-adhoc-count');
  if (adhocCount) adhocCount.textContent = n;
}

// ── Contact dialog ───────────────────────────────────────────────
function openContactDialog(id) {
  const dlg = document.getElementById('dlg-contact');
  const form = document.getElementById('contact-form');
  form.reset();
  const editing = id ? state.contacts.find(c => c.id === id) : null;
  document.getElementById('dlg-contact-title').textContent =
    editing ? 'Editar contato' : 'Novo contato';
  form.dataset.id = editing ? editing.id : '';
  if (editing) {
    form.display_name.value = editing.display_name || '';
    form.whatsapp.value = editing.whatsapp || '';
    form.whatsapp_business.value = editing.whatsapp_business || '';
    form.telegram.value = editing.telegram || '';
    form.tags.value = (editing.tags || []).join(', ');
    form.notes.value = editing.notes || '';
  }
  dlg.showModal();
}

async function onContactFormSubmit(e) {
  e.preventDefault();
  const f = e.target;
  const contact = {
    id: f.dataset.id || NIL_UUID,
    display_name: f.display_name.value.trim(),
    whatsapp: f.whatsapp.value.trim() || null,
    whatsapp_business: f.whatsapp_business.value.trim() || null,
    telegram: f.telegram.value.trim() || null,
    tags: f.tags.value.split(',').map(t => t.trim()).filter(Boolean),
    source: f.dataset.id ? undefined : 'manual',
    notes: f.notes.value.trim() || null,
  };
  // Server requires `source` field present; default if empty.
  if (!contact.source) contact.source = 'manual';

  try {
    const saved = await invoke('vorcaro_save_contact', { contact });
    const idx = state.contacts.findIndex(c => c.id === saved.id);
    if (idx >= 0) state.contacts[idx] = saved;
    else state.contacts.push(saved);
    renderContacts();
    document.getElementById('dlg-contact').close();
    toast('Contato salvo', 'ok');
  } catch (err) {
    toast('Erro: ' + err, 'error');
  }
}

async function deleteContact(id) {
  if (!confirm('Apagar este contato?')) return;
  try {
    await invoke('vorcaro_delete_contact', { id });
    state.contacts = state.contacts.filter(c => c.id !== id);
    state.selectedIds.delete(id);
    // remove from lists too (server already did)
    state.lists.forEach(l => { l.contact_ids = (l.contact_ids || []).filter(x => x !== id); });
    renderContacts();
    if (state.selectedListId) renderListDetail();
    toast('Contato apagado', 'ok');
  } catch (err) {
    toast('Erro: ' + err, 'error');
  }
}

// ── CSV import ───────────────────────────────────────────────────
async function onCsvFile(e) {
  const file = e.target.files?.[0];
  e.target.value = '';
  if (!file) return;
  const content = await file.text();
  try {
    const report = await invoke('vorcaro_import_csv', { content });
    // Reload state to pick up new/merged contacts
    const snap = await invoke('vorcaro_get_state');
    state.contacts = snap.contacts || [];
    renderContacts();
    toast(`CSV: ${report.added} novos · ${report.merged} mesclados · ${report.skipped} ignorados`, 'ok');
  } catch (err) {
    toast('Erro no CSV: ' + err, 'error');
  }
}

// ── Bulk actions ────────────────────────────────────────────────
async function onBulkDelete() {
  const ids = [...state.selectedIds];
  if (ids.length === 0) return;
  if (!confirm(`Apagar ${ids.length} contato(s)?`)) return;
  for (const id of ids) {
    try { await invoke('vorcaro_delete_contact', { id }); } catch (_) {}
  }
  state.contacts = state.contacts.filter(c => !state.selectedIds.has(c.id));
  state.selectedIds.clear();
  renderContacts();
  toast(`${ids.length} apagado(s)`, 'ok');
}

async function onBulkTag() {
  const ids = [...state.selectedIds];
  if (ids.length === 0) return;
  const tag = await prompt('Aplicar tag a ' + ids.length + ' contato(s):', '');
  if (!tag) return;
  try {
    await invoke('vorcaro_apply_tag', { contactIds: ids, tag });
    state.contacts.forEach(c => {
      if (state.selectedIds.has(c.id) && !(c.tags || []).includes(tag)) {
        c.tags = [...(c.tags || []), tag];
      }
    });
    renderContacts();
    toast('Tag aplicada', 'ok');
  } catch (err) { toast('Erro: ' + err, 'error'); }
}

async function onBulkUntag() {
  const ids = [...state.selectedIds];
  if (ids.length === 0) return;
  const tag = await prompt('Remover tag de ' + ids.length + ' contato(s):', '');
  if (!tag) return;
  try {
    await invoke('vorcaro_remove_tag', { contactIds: ids, tag });
    state.contacts.forEach(c => {
      if (state.selectedIds.has(c.id)) {
        c.tags = (c.tags || []).filter(t => t !== tag);
      }
    });
    renderContacts();
    toast('Tag removida', 'ok');
  } catch (err) { toast('Erro: ' + err, 'error'); }
}

async function onBulkAddToList() {
  const ids = [...state.selectedIds];
  if (ids.length === 0) return;
  if (state.lists.length === 0) {
    toast('Crie uma lista primeiro na aba Listas', 'error');
    return;
  }
  const sel = document.getElementById('pick-list-select');
  sel.innerHTML = state.lists.map(l =>
    `<option value="${l.id}">${esc(l.name)} (${(l.contact_ids || []).length})</option>`
  ).join('');
  const dlg = document.getElementById('dlg-pick-list');
  document.getElementById('btn-pick-list-cancel').onclick = () => dlg.close();
  dlg.showModal();
  dlg.addEventListener('close', async function once() {
    dlg.removeEventListener('close', once);
    if (dlg.returnValue === 'cancel') return;
    const listId = sel.value;
    let ok = 0;
    for (const cid of ids) {
      try {
        await invoke('vorcaro_add_contact_to_list', { listId, contactId: cid });
        ok++;
      } catch (_) {}
    }
    const list = state.lists.find(l => l.id === listId);
    if (list) {
      ids.forEach(cid => {
        if (!list.contact_ids.includes(cid)) list.contact_ids.push(cid);
      });
    }
    if (state.selectedListId === listId) renderListDetail();
    toast(`${ok} adicionado(s) à lista`, 'ok');
  }, { once: true });
}

// ── LISTS TAB ────────────────────────────────────────────────────
function bindListsTab() {
  document.getElementById('btn-new-list').addEventListener('click', async () => {
    const name = await prompt('Nome da nova lista:', '');
    if (!name) return;
    try {
      const saved = await invoke('vorcaro_save_list', {
        list: { id: NIL_UUID, name, contact_ids: [] }
      });
      state.lists.push(saved);
      state.selectedListId = saved.id;
      renderListsSide();
      renderListDetail();
    } catch (err) { toast('Erro: ' + err, 'error'); }
  });

  document.getElementById('btn-rename-list').addEventListener('click', async () => {
    const list = currentList();
    if (!list) return;
    const newName = document.getElementById('list-name-input').value.trim();
    if (!newName || newName === list.name) return;
    try {
      const saved = await invoke('vorcaro_save_list', {
        list: { ...list, name: newName }
      });
      const idx = state.lists.findIndex(l => l.id === saved.id);
      if (idx >= 0) state.lists[idx] = saved;
      renderListsSide();
      toast('Lista renomeada', 'ok');
    } catch (err) { toast('Erro: ' + err, 'error'); }
  });

  document.getElementById('btn-delete-list').addEventListener('click', async () => {
    const list = currentList();
    if (!list) return;
    if (!confirm(`Apagar lista "${list.name}"? (Os contatos permanecem.)`)) return;
    try {
      await invoke('vorcaro_delete_list', { id: list.id });
      state.lists = state.lists.filter(l => l.id !== list.id);
      state.selectedListId = null;
      renderListsSide();
      document.getElementById('list-detail').hidden = true;
      toast('Lista apagada', 'ok');
    } catch (err) { toast('Erro: ' + err, 'error'); }
  });

  document.getElementById('list-add-search').addEventListener('input', renderListAddCandidates);
}

function currentList() {
  return state.lists.find(l => l.id === state.selectedListId) || null;
}

function renderListsSide() {
  const ul = document.getElementById('lists-ul');
  ul.innerHTML = state.lists.map(l => `
    <li data-id="${l.id}" class="${l.id === state.selectedListId ? 'active' : ''}">
      ${esc(l.name)}
      <span class="meta">${(l.contact_ids || []).length} contato(s)</span>
    </li>`).join('');
  ul.querySelectorAll('li').forEach(li => {
    li.addEventListener('click', () => {
      state.selectedListId = li.dataset.id;
      renderListsSide();
      renderListDetail();
    });
  });
  document.getElementById('lists-empty').hidden = state.lists.length > 0;
  if (!state.selectedListId) {
    document.getElementById('list-detail').hidden = true;
  }
}

function renderListDetail() {
  const list = currentList();
  const detail = document.getElementById('list-detail');
  if (!list) { detail.hidden = true; return; }
  detail.hidden = false;
  document.getElementById('list-name-input').value = list.name;
  document.getElementById('list-member-count').textContent = (list.contact_ids || []).length;

  const membersUl = document.getElementById('list-members-ul');
  const memberContacts = (list.contact_ids || [])
    .map(id => state.contacts.find(c => c.id === id))
    .filter(Boolean);
  membersUl.innerHTML = memberContacts.length
    ? memberContacts.map(c => `
        <li data-id="${c.id}">
          <span>${esc(c.display_name)} <small style="color:var(--fg-dim)">${esc(handles(c))}</small></span>
          <button data-id="${c.id}">Remover</button>
        </li>`).join('')
    : '<li style="justify-content:center;color:var(--fg-dim)">Lista vazia</li>';
  membersUl.querySelectorAll('button').forEach(btn => {
    btn.addEventListener('click', async () => {
      const cid = btn.dataset.id;
      try {
        await invoke('vorcaro_remove_contact_from_list', { listId: list.id, contactId: cid });
        list.contact_ids = list.contact_ids.filter(x => x !== cid);
        renderListsSide();
        renderListDetail();
      } catch (err) { toast('Erro: ' + err, 'error'); }
    });
  });

  renderListAddCandidates();
}

function renderListAddCandidates() {
  const list = currentList();
  if (!list) return;
  const q = (document.getElementById('list-add-search').value || '').toLowerCase();
  const memberSet = new Set(list.contact_ids || []);
  const candidates = state.contacts.filter(c => {
    if (memberSet.has(c.id)) return false;
    if (!q) return true;
    return [c.display_name, c.whatsapp, c.telegram].filter(Boolean)
      .join(' ').toLowerCase().includes(q);
  }).slice(0, 50);

  const ul = document.getElementById('list-add-ul');
  ul.innerHTML = candidates.length
    ? candidates.map(c => `
        <li>
          <span>${esc(c.display_name)} <small style="color:var(--fg-dim)">${esc(handles(c))}</small></span>
          <button data-id="${c.id}">Adicionar</button>
        </li>`).join('')
    : '<li style="justify-content:center;color:var(--fg-dim)">Sem candidatos</li>';
  ul.querySelectorAll('button').forEach(btn => {
    btn.addEventListener('click', async () => {
      const cid = btn.dataset.id;
      try {
        await invoke('vorcaro_add_contact_to_list', { listId: list.id, contactId: cid });
        list.contact_ids.push(cid);
        renderListsSide();
        renderListDetail();
      } catch (err) { toast('Erro: ' + err, 'error'); }
    });
  });
}

function handles(c) {
  return [c.whatsapp, c.whatsapp_business, c.telegram].filter(Boolean).join(' · ');
}

// ── SETTINGS TAB ─────────────────────────────────────────────────
function bindSettingsTab() {
  document.getElementById('btn-save-settings').addEventListener('click', async () => {
    const settings = {
      min_delay_secs: parseInt(document.getElementById('set-min-delay').value, 10) || 0,
      max_delay_secs: parseInt(document.getElementById('set-max-delay').value, 10) || 0,
      daily_cap_per_platform: parseInt(document.getElementById('set-daily-cap').value, 10) || 0,
      warn_threshold: parseInt(document.getElementById('set-warn').value, 10) || 0,
      auto_pause_after_consecutive_failures:
        parseInt(document.getElementById('set-autopause').value, 10) || 0,
      max_retries_per_recipient:
        parseInt(document.getElementById('set-retries').value, 10) || 0,
    };
    if (settings.max_delay_secs < settings.min_delay_secs) {
      toast('Atraso máximo deve ser ≥ mínimo', 'error');
      return;
    }
    try {
      const saved = await invoke('vorcaro_update_settings', { settings });
      state.settings = saved;
      const flag = document.getElementById('settings-saved-flag');
      flag.hidden = false;
      setTimeout(() => { flag.hidden = true; }, 1800);
    } catch (err) { toast('Erro: ' + err, 'error'); }
  });
}

function fillSettingsForm() {
  const s = state.settings;
  document.getElementById('set-min-delay').value = s.min_delay_secs ?? 30;
  document.getElementById('set-max-delay').value = s.max_delay_secs ?? 90;
  document.getElementById('set-daily-cap').value = s.daily_cap_per_platform ?? 100;
  document.getElementById('set-warn').value = s.warn_threshold ?? 20;
  document.getElementById('set-autopause').value = s.auto_pause_after_consecutive_failures ?? 3;
  document.getElementById('set-retries').value = s.max_retries_per_recipient ?? 0;
}

// ── Lightweight prompt (replaces window.prompt for styled UX) ────
function prompt(title, initial = '') {
  return new Promise(resolve => {
    const dlg = document.getElementById('dlg-prompt');
    const input = document.getElementById('dlg-prompt-input');
    document.getElementById('dlg-prompt-title').textContent = title;
    document.getElementById('dlg-prompt-label').textContent = title;
    input.value = initial;

    const cleanup = () => {
      dlg.removeEventListener('close', onClose);
      document.getElementById('btn-prompt-cancel').removeEventListener('click', onCancel);
    };
    const onClose = () => {
      cleanup();
      resolve(dlg.returnValue === 'cancel' ? null : input.value.trim() || null);
    };
    const onCancel = () => { dlg.close('cancel'); };

    dlg.addEventListener('close', onClose);
    document.getElementById('btn-prompt-cancel').addEventListener('click', onCancel);
    dlg.showModal();
    input.focus();
    input.select();
  });
}

// ── CLOUD API (Settings tab) ────────────────────────────────────
function bindCloudApi() {
  document.getElementById('btn-save-cloud').addEventListener('click', onSaveCloud);
  document.getElementById('btn-test-cloud').addEventListener('click', onTestCloud);
}

async function fillCloudApiForm() {
  try {
    const cfg = await invoke('vorcaro_get_cloud_config');
    document.getElementById('cloud-token').value = cfg.access_token || '';
    document.getElementById('cloud-phone-id').value = cfg.phone_number_id || '';
    document.getElementById('cloud-waba-id').value = cfg.business_account_id || '';
    document.getElementById('cloud-version').value = cfg.api_version || '';
  } catch (err) {
    // First boot, file doesn't exist — fine.
  }
}

async function onSaveCloud() {
  const cfg = {
    access_token: document.getElementById('cloud-token').value,
    phone_number_id: document.getElementById('cloud-phone-id').value.trim(),
    business_account_id: document.getElementById('cloud-waba-id').value.trim(),
    api_version: document.getElementById('cloud-version').value.trim() || null,
  };
  try {
    await invoke('vorcaro_save_cloud_config', { config: cfg });
    // Reload to get the redacted token back so we don't accidentally
    // overwrite it with the redacted form on next save.
    await fillCloudApiForm();
    toast('Credenciais Cloud API salvas', 'ok');
  } catch (err) {
    toast('Erro: ' + err, 'error');
  }
}

async function onTestCloud() {
  const status = document.getElementById('cloud-status');
  status.hidden = false;
  status.className = '';
  status.textContent = 'testando…';
  try {
    const info = await invoke('vorcaro_verify_cloud_connection');
    status.className = 'ok';
    status.textContent = 'OK — ' + info.slice(0, 120);
  } catch (err) {
    status.className = '';
    status.style.color = 'var(--danger)';
    status.textContent = '✗ ' + err;
  }
}

// ── SCRAPE ──────────────────────────────────────────────────────
async function bindScrape() {
  document.getElementById('btn-scrape').addEventListener('click', startScrape);
  document.getElementById('btn-refresh-labels').addEventListener('click', refreshWaLabels);
  document.getElementById('btn-debug-dom').addEventListener('click', runDomDebug);
  document.getElementById('btn-debug-close').addEventListener('click',
    () => document.getElementById('dlg-debug').close());
  document.getElementById('btn-debug-copy').addEventListener('click', () => {
    const ta = document.getElementById('debug-dump-text');
    ta.select();
    document.execCommand('copy');
    toast('Copiado', 'ok');
  });
  document.getElementById('scrape-workspace').addEventListener('change', () => {
    // Reset label dropdown so stale labels from another account don't leak.
    const sel = document.getElementById('scrape-label');
    sel.innerHTML = '<option value="">— sem filtro de etiqueta —</option>';
    refreshWaLabels().catch(() => {});
  });

  document.getElementById('btn-scrape-cancel').addEventListener('click',
    () => document.getElementById('dlg-scrape').close());
  document.getElementById('btn-scrape-import').addEventListener('click', importScrapedSelection);

  document.getElementById('scrape-check-all').addEventListener('change', e => {
    state.scrapeSelected.clear();
    if (e.target.checked) {
      filteredScrapeRows().forEach((_, idx) => state.scrapeSelected.add(rowKey(_, idx)));
    }
    renderScrapeBody();
  });
  document.getElementById('scrape-search').addEventListener('input', e => {
    state.scrapeFilter = e.target.value.toLowerCase();
    renderScrapeBody();
  });

  const { listen } = window.__TAURI__.event;
  // Scrape results.
  await listen('vorcaro://scrape-result', ({ payload }) => {
    openScrapeDialog(payload.platform, payload.rows || [], payload.error);
  });
  // Progress for the slow click-extract phase.
  await listen('vorcaro://scrape-progress', ({ payload }) => {
    toast(`Extraindo telefones… ${payload.current}/${payload.total}`, 'ok');
  });
  // Debug dump from WA driver — opened by 🐞 button OR auto-triggered on listLabels failure.
  await listen('vorcaro://debug-dom-result', ({ payload }) => {
    const dlg = document.getElementById('dlg-debug');
    document.getElementById('debug-dump-text').value = payload.dump || '(vazio)';
    if (!dlg.open) dlg.showModal();
  });

  // WA-Business labels dropdown.
  await listen('vorcaro://wa-labels-result', ({ payload }) => {
    const sel = document.getElementById('scrape-label');
    const prev = sel.value;
    const labels = payload.labels || [];
    let html = '<option value="">— sem filtro de etiqueta —</option>';
    labels.forEach(lbl => { html += `<option value="${esc(lbl)}">${esc(lbl)}</option>`; });
    sel.innerHTML = html;
    if (prev && labels.includes(prev)) sel.value = prev;
    if (payload.error) {
      toast('Etiquetas: ' + payload.error, 'error');
    } else if (labels.length === 0) {
      toast('Nenhuma etiqueta encontrada', 'error');
    } else {
      toast(`${labels.length} etiqueta(s) carregada(s)`, 'ok');
    }
  });
}

async function refreshWaLabels() {
  const workspaceId = document.getElementById('scrape-workspace').value;
  if (!workspaceId) return;
  try {
    await invoke('vorcaro_list_wa_labels', { workspaceId });
  } catch (err) {
    toast('Etiquetas: ' + err, 'error');
  }
}

async function runDomDebug() {
  const workspaceId = document.getElementById('scrape-workspace').value;
  if (!workspaceId) {
    toast('Selecione uma conta primeiro', 'error');
    return;
  }
  try {
    await invoke('vorcaro_debug_chat_pane', { workspaceId });
  } catch (err) {
    toast('Diag: ' + err, 'error');
  }
}

async function startScrape() {
  const workspaceId = document.getElementById('scrape-workspace').value;
  if (!workspaceId) {
    toast('Adicione um WhatsApp ou Telegram em BigBox primeiro.', 'error');
    return;
  }
  const ws = state.workspaces.find(w => w.id === workspaceId);
  const labelFilter = (document.getElementById('scrape-label').value || '').trim() || null;
  try {
    await invoke('vorcaro_scrape_workspace', { workspaceId, labelFilter });
    const label = labelFilter ? ` (etiqueta: ${labelFilter})` : '';
    toast(`Raspando ${ws?.display_name || workspaceId}${label}…`, 'ok');
  } catch (err) {
    toast('Não foi possível raspar: ' + err, 'error');
  }
}

function rowKey(row, idx) {
  // Stable key for selection set — phone/peer_id ideally, else index.
  return (row.phone || row.peer_id || row.username || `idx-${idx}`) + '|' + (row.name || '');
}

function filteredScrapeRows() {
  const q = state.scrapeFilter;
  if (!q) return state.scrapeRows;
  return state.scrapeRows.filter(r => {
    const blob = [r.name, r.phone, r.username, r.peer_id].filter(Boolean).join(' ').toLowerCase();
    return blob.includes(q);
  });
}

function openScrapeDialog(platform, rows, error) {
  state.scrapePlatform = platform;
  state.scrapeRows = rows;
  state.scrapeSelected = new Set(rows.map((r, idx) => rowKey(r, idx)));
  state.scrapeFilter = '';
  document.getElementById('scrape-search').value = '';
  document.getElementById('scrape-check-all').checked = true;

  document.getElementById('dlg-scrape-title').textContent =
    `Raspagem: ${PLATFORM_LABEL[platform] || platform}`;
  const statusEl = document.getElementById('dlg-scrape-status');
  if (error) {
    statusEl.textContent = `Erro: ${error}`;
    statusEl.style.color = 'var(--danger)';
  } else {
    statusEl.textContent = `${rows.length} conversa(s) encontrada(s). Selecione quais importar.`;
    statusEl.style.color = 'var(--fg-dim)';
  }
  renderScrapeBody();
  document.getElementById('dlg-scrape').showModal();
}

function renderScrapeBody() {
  const tbody = document.getElementById('scrape-body');
  const rows = filteredScrapeRows();
  tbody.innerHTML = rows.map((r, idx) => {
    const key = rowKey(r, idx);
    const checked = state.scrapeSelected.has(key) ? 'checked' : '';
    const handle = r.phone || r.username || r.peer_id || '—';
    return `
      <tr data-key="${esc(key)}">
        <td class="col-check"><input type="checkbox" class="srow-check" ${checked}></td>
        <td>${esc(r.name)}</td>
        <td>${esc(handle)}</td>
      </tr>`;
  }).join('') || '<tr><td colspan="3" class="empty">Sem resultados</td></tr>';

  tbody.querySelectorAll('tr[data-key]').forEach(tr => {
    const key = tr.dataset.key;
    tr.querySelector('.srow-check')?.addEventListener('change', e => {
      if (e.target.checked) state.scrapeSelected.add(key);
      else state.scrapeSelected.delete(key);
    });
  });
}

async function importScrapedSelection() {
  const selected = state.scrapeRows.filter((r, idx) =>
    state.scrapeSelected.has(rowKey(r, idx)));
  if (selected.length === 0) {
    toast('Nada selecionado', 'error');
    return;
  }
  try {
    const report = await invoke('vorcaro_import_scraped', {
      platform: state.scrapePlatform,
      rows: selected,
    });
    // Reload contacts from store
    const snap = await invoke('vorcaro_get_state');
    state.contacts = snap.contacts || [];
    renderContacts();
    document.getElementById('dlg-scrape').close();
    toast(`Raspagem: ${report.added} novos · ${report.merged} mesclados · ${report.skipped} ignorados`, 'ok');
  } catch (err) {
    toast('Erro: ' + err, 'error');
  }
}

// ── CAMPAIGN TAB ────────────────────────────────────────────────
let activeCampaignId = null;
let liveCampaignId = null;       // the campaign whose progress is streaming
const liveProgress = new Map();  // campaign_id → { attempts: [], status: '...', etc. }

// Pending attachments for the campaign being composed: { path, name, size }.
// One attachment per campaign in Phase E.2; multi-attachment is Phase E.3.
let campAttachments = [];

const MAX_ATTACHMENT_BYTES = 64 * 1024 * 1024; // 64 MB hard cap (memory safety)

function bindCampaign() {
  document.getElementById('camp-target-mode').addEventListener('change', renderCampaignForm);
  document.getElementById('camp-platform').addEventListener('change', () => {
    refreshCampaignWorkspaces();
    renderCampaignForm();
    renderCampaignRecent();
  });
  document.getElementById('btn-preview').addEventListener('click', onPreview);
  document.getElementById('btn-start').addEventListener('click', onStartCampaign);
  document.getElementById('camp-schedule').addEventListener('input', updateStartButtonLabel);
  document.getElementById('btn-pick-attachment').addEventListener('click',
    () => document.getElementById('camp-file').click());
  document.getElementById('camp-file').addEventListener('change', onAttachmentPicked);
  document.getElementById('camp-cloud-msg-type').addEventListener('change', renderCampaignForm);
  document.getElementById('camp-template-name').addEventListener('change', renderTemplateParams);
  document.getElementById('btn-refresh-templates').addEventListener('click', refreshTemplates);
  document.getElementById('btn-emoji').addEventListener('click', toggleEmojiPopup);
}

// ── Emoji picker ────────────────────────────────────────────────
const EMOJIS = [
  '😀','😁','😂','🤣','😃','😄','😅','😆','😉','😊',
  '😋','😎','🥰','😍','😘','🤗','🤔','😐','🙄','😏',
  '😒','😣','😥','😮','😪','😫','🥱','😴','😵','🤐',
  '🤤','😡','🤬','😱','😨','😰','😢','😭','🥲','🤓',
  '🤩','🥳','🤯','🤠','🤡','🤥','🤫','🤭','🫡','🫢',
  '👍','👎','👏','🙏','👋','✌️','🤞','🤝','🙌','💪',
  '🫶','🫰','🤘','🤙','✋','👌','🫵','☝️','🫳','🫴',
  '❤️','🧡','💛','💚','💙','💜','🖤','🤍','💔','❣️',
  '💯','💢','💥','💫','💦','💨','🔥','⭐','✨','🎉',
  '🎊','🎁','🎂','🎈','🍕','🍻','☕','🌹','🌺','🌷',
  '☀️','🌙','⚡','🌈','✅','❌','⚠️','📢','📣','📌',
];

function toggleEmojiPopup() {
  let popup = document.getElementById('emoji-popup');
  if (popup) { popup.remove(); return; }
  popup = document.createElement('div');
  popup.id = 'emoji-popup';
  popup.className = 'emoji-popup';
  popup.innerHTML = EMOJIS.map(e =>
    `<button type="button" data-e="${e}">${e}</button>`
  ).join('');
  document.body.appendChild(popup);

  // Position near the emoji button.
  const btnRect = document.getElementById('btn-emoji').getBoundingClientRect();
  popup.style.left = Math.min(btnRect.left, window.innerWidth - 280) + 'px';
  popup.style.top = (btnRect.bottom + 4) + 'px';

  popup.addEventListener('click', e => {
    const sym = e.target.dataset?.e;
    if (!sym) return;
    insertAtCursor('camp-body', sym);
    popup.remove();
  });
  // Close when clicking outside
  setTimeout(() => {
    document.addEventListener('click', function onDocClick(ev) {
      if (popup.contains(ev.target) || ev.target.id === 'btn-emoji') return;
      popup.remove();
      document.removeEventListener('click', onDocClick);
    });
  }, 0);
}

function insertAtCursor(textareaId, text) {
  const ta = document.getElementById(textareaId);
  const start = ta.selectionStart;
  const end = ta.selectionEnd;
  ta.value = ta.value.substring(0, start) + text + ta.value.substring(end);
  ta.selectionStart = ta.selectionEnd = start + text.length;
  ta.focus();
}

async function refreshTemplates() {
  try {
    cloudTemplates = await invoke('vorcaro_list_cloud_templates');
    const sel = document.getElementById('camp-template-name');
    const approved = cloudTemplates.filter(t => t.status === 'APPROVED');
    sel.innerHTML = approved.length
      ? approved.map(t => `<option value="${esc(t.name)}::${esc(t.language)}">${esc(t.name)} (${esc(t.language)})</option>`).join('')
      : '<option value="">— nenhum template APROVADO encontrado —</option>';
    renderTemplateParams();
  } catch (err) {
    toast('Erro listando templates: ' + err, 'error');
    document.getElementById('camp-template-name').innerHTML =
      '<option value="">— erro ao carregar —</option>';
  }
}

function currentTemplate() {
  const sel = document.getElementById('camp-template-name');
  const val = sel.value;
  if (!val) return null;
  const [name, language] = val.split('::');
  return cloudTemplates.find(t => t.name === name && t.language === language) || null;
}

function renderTemplateParams() {
  const tmpl = currentTemplate();
  const previewEl = document.getElementById('camp-template-preview');
  const paramsEl = document.getElementById('camp-template-params');
  if (!tmpl) {
    previewEl.textContent = '';
    paramsEl.innerHTML = '';
    return;
  }
  previewEl.textContent = tmpl.body_text || '';
  const n = tmpl.body_param_count || 0;
  if (n === 0) {
    paramsEl.innerHTML = '<p class="hint">Template sem parâmetros.</p>';
    return;
  }
  const defaults = ['{firstname}', '{nome}', '{whatsapp}', '{tag}'];
  let html = '<p class="hint" style="margin-top:8px">Parâmetros do template — use variáveis tipo <code>{firstname}</code> ou texto fixo:</p>';
  for (let i = 1; i <= n; i++) {
    const def = defaults[i - 1] || '';
    html += `<label class="field"><span>Param ${i}</span>
      <input type="text" class="tmpl-param" data-idx="${i - 1}" value="${esc(def)}"></label>`;
  }
  paramsEl.innerHTML = html;
}

async function onAttachmentPicked(e) {
  const file = e.target.files?.[0];
  e.target.value = '';
  if (!file) return;
  if (file.size > MAX_ATTACHMENT_BYTES) {
    toast(`Arquivo grande demais (limite 64 MB)`, 'error');
    return;
  }
  try {
    toast('Carregando anexo…', 'ok');
    const b64 = await fileToBase64(file);
    const path = await invoke('vorcaro_stage_attachment', { name: file.name, b64 });
    // Phase E.2: replace any existing attachment (single per campaign).
    campAttachments = [{ path, name: file.name, size: file.size }];
    renderAttachmentList();
    toast('Anexo pronto', 'ok');
  } catch (err) {
    toast('Erro ao anexar: ' + err, 'error');
  }
}

function fileToBase64(file) {
  return new Promise((resolve, reject) => {
    const fr = new FileReader();
    fr.onload = () => {
      // result is `data:<mime>;base64,<payload>` — strip the prefix.
      const s = String(fr.result);
      const idx = s.indexOf('base64,');
      resolve(idx >= 0 ? s.slice(idx + 7) : s);
    };
    fr.onerror = () => reject(fr.error);
    fr.readAsDataURL(file);
  });
}

function renderAttachmentList() {
  const list = document.getElementById('camp-attach-list');
  list.innerHTML = campAttachments.map((a, i) => `
    <div class="attach-chip">
      <span class="name">${esc(a.name)}</span>
      <span class="size">${formatSize(a.size)}</span>
      <button data-idx="${i}">remover</button>
    </div>`).join('');
  list.querySelectorAll('button').forEach(btn => {
    btn.addEventListener('click', () => {
      campAttachments.splice(Number(btn.dataset.idx), 1);
      renderAttachmentList();
    });
  });
}

function formatSize(bytes) {
  if (bytes < 1024) return bytes + ' B';
  if (bytes < 1024 * 1024) return (bytes / 1024).toFixed(1) + ' KB';
  return (bytes / 1024 / 1024).toFixed(1) + ' MB';
}

function updateStartButtonLabel() {
  const dt = document.getElementById('camp-schedule').value;
  document.getElementById('btn-start').textContent = dt ? 'Agendar envio' : 'Iniciar envio';
}

let cloudTemplates = [];

function renderCampaignForm() {
  const platform = document.getElementById('camp-platform').value;
  const isCloud = platform === 'whatsapp_cloud_api';
  document.getElementById('camp-cloud-section').hidden = !isCloud;

  // Cloud API doesn't support attachments yet (Phase F.2).
  const attachRow = document.getElementById('btn-pick-attachment')?.closest('.field');
  if (attachRow) attachRow.hidden = isCloud;

  // Body field is hidden when Cloud API + template (template params replace it).
  const cloudMsgType = document.getElementById('camp-cloud-msg-type')?.value || 'template';
  const usingTemplate = isCloud && cloudMsgType === 'template';
  document.getElementById('camp-body-row').hidden = usingTemplate;
  document.getElementById('camp-template-row').hidden = !usingTemplate;
  document.getElementById('camp-template-params').hidden = !usingTemplate;

  if (isCloud && cloudTemplates.length === 0) {
    refreshTemplates().catch(() => {});
  }

  // Lists dropdown
  const listSel = document.getElementById('camp-list-id');
  listSel.innerHTML = state.lists.length
    ? state.lists.map(l =>
        `<option value="${l.id}">${esc(l.name)} (${(l.contact_ids || []).length})</option>`
      ).join('')
    : '<option value="">— sem listas —</option>';

  // Tags dropdown
  const tagSel = document.getElementById('camp-tag-name');
  const tags = allTags();
  tagSel.innerHTML = tags.length
    ? tags.map(t => `<option value="${esc(t)}">${esc(t)}</option>`).join('')
    : '<option value="">— sem tags —</option>';

  // Ad-hoc count
  document.getElementById('camp-adhoc-count').textContent = state.selectedIds.size;

  // Show/hide rows based on target mode
  const mode = document.getElementById('camp-target-mode').value;
  document.getElementById('camp-list-row').hidden  = mode !== 'list';
  document.getElementById('camp-tag-row').hidden   = mode !== 'tag';
  document.getElementById('camp-adhoc-row').hidden = mode !== 'adhoc';

  document.getElementById('camp-preview-box').hidden = true;
}

function renderCampaignRecent() {
  const ul = document.getElementById('camp-recent');
  const recent = [...state.campaigns]
    .sort((a, b) => (b.created_at || '').localeCompare(a.created_at || ''))
    .slice(0, 12);
  ul.innerHTML = recent.length
    ? recent.map(c => {
        const stClass = (c.status || 'draft').toLowerCase();
        return `
          <li data-id="${c.id}">
            <span class="pill ${stClass}">${stClass}</span> ${esc(c.name)}
            <span class="meta">${esc(c.platform)} · ${(c.progress || []).length} envio(s)</span>
          </li>`;
      }).join('')
    : '<li style="cursor:default;color:var(--fg-dim)">— nenhuma campanha ainda —</li>';
  ul.querySelectorAll('li[data-id]').forEach(li => {
    li.addEventListener('click', () => {
      activeCampaignId = li.dataset.id;
      document.querySelector('.tab[data-tab="logs"]').click();
      renderLogsCampaignSelect();
      renderLogsForActive();
    });
  });
}

function readCampaignSpec() {
  const mode = document.getElementById('camp-target-mode').value;
  let targets;
  if (mode === 'list') {
    const listId = document.getElementById('camp-list-id').value;
    if (!listId) throw 'selecione uma lista';
    targets = { kind: 'list', value: listId };
  } else if (mode === 'tag') {
    const tag = document.getElementById('camp-tag-name').value;
    if (!tag) throw 'selecione uma tag';
    targets = { kind: 'tag', value: tag };
  } else {
    const ids = [...state.selectedIds];
    if (ids.length === 0) throw 'selecione contatos na aba Contatos';
    targets = { kind: 'ad_hoc', value: ids };
  }
  // datetime-local → ISO 8601 UTC. Empty = immediate.
  let scheduledAt = null;
  const dt = document.getElementById('camp-schedule').value;
  if (dt) {
    const local = new Date(dt);
    if (Number.isNaN(local.getTime())) throw 'data/hora inválida';
    if (local.getTime() < Date.now() - 60_000) {
      throw 'a data agendada já passou';
    }
    scheduledAt = local.toISOString();
  }

  const platform = document.getElementById('camp-platform').value;
  const cloudMsgType = document.getElementById('camp-cloud-msg-type').value;

  let template = null;
  if (platform === 'whatsapp_cloud_api' && cloudMsgType === 'template') {
    const tmpl = currentTemplate();
    if (!tmpl) throw 'selecione um template aprovado';
    const params = Array.from(document.querySelectorAll('.tmpl-param'))
      .sort((a, b) => Number(a.dataset.idx) - Number(b.dataset.idx))
      .map(i => i.value.trim());
    template = { name: tmpl.name, language: tmpl.language, body_params: params };
  }

  // Workspace selection (which BigBox slot to send from).
  let workspaceId = null;
  if (platform !== 'whatsapp_cloud_api') {
    const wsVal = document.getElementById('camp-workspace').value;
    if (!wsVal) {
      throw `Nenhum ${PLATFORM_SHORT[platform] || platform} adicionado em BigBox. Adicione na barra lateral primeiro.`;
    }
    workspaceId = wsVal;
  }

  return {
    name: document.getElementById('camp-name').value.trim() || 'Campanha sem nome',
    body: document.getElementById('camp-body').value,
    platform,
    targets,
    scheduled_at: scheduledAt,
    attachments: platform === 'whatsapp_cloud_api' ? [] : campAttachments.map(a => a.path),
    template,
    workspace_id: workspaceId,
  };
}

async function onPreview() {
  let spec;
  try { spec = readCampaignSpec(); }
  catch (msg) { toast(String(msg), 'error'); return; }

  try {
    const p = await invoke('vorcaro_preview_campaign', {
      targets: spec.targets,
      platform: spec.platform,
    });
    const box = document.getElementById('camp-preview-box');
    let warn = '';
    if (p.warn) {
      warn = `<div class="warn">⚠ Mais de ${state.settings.warn_threshold || 20} destinatários.
        Envios em massa pelo WhatsApp Web podem causar <strong>banimento da conta</strong>.
        Considere usar listas menores ou aumentar o atraso em Configurações.</div>`;
    }
    box.innerHTML = `
      <div class="row"><span>Destinatários</span><b>${p.recipient_count}</b></div>
      <div class="row"><span>Com identificador para ${esc(spec.platform)}</span><b>${p.recipients_with_handle}</b></div>
      <div class="row"><span>Sem identificador (serão pulados)</span><b>${p.recipients_missing_handle}</b></div>
      <div class="row"><span>Saldo diário restante</span><b>${p.daily_cap_remaining}</b></div>
      ${warn}`;
    box.hidden = false;
  } catch (err) {
    toast('Erro no preview: ' + err, 'error');
  }
}

async function onStartCampaign() {
  let spec;
  try { spec = readCampaignSpec(); }
  catch (msg) { toast(String(msg), 'error'); return; }
  if (!spec.body.trim() && !spec.template) {
    toast('Mensagem vazia', 'error');
    return;
  }

  // Inline preview so we can give a clear error if the targets / platform
  // combination yields no usable recipients.
  let p;
  try {
    p = await invoke('vorcaro_preview_campaign', {
      targets: spec.targets,
      platform: spec.platform,
    });
  } catch (err) {
    toast('Erro no preview: ' + err, 'error');
    return;
  }
  if (p.recipient_count === 0) {
    toast('Nenhum destinatário encontrado pra esta seleção', 'error');
    return;
  }
  if (p.recipients_with_handle === 0) {
    const field = spec.platform === 'whatsapp_business_web' ? 'WA Business'
                : spec.platform === 'telegram' ? 'Telegram'
                : 'WhatsApp';
    toast(`Nenhum dos ${p.recipient_count} contatos tem o campo "${field}" preenchido. Edite os contatos ou troque a plataforma.`, 'error');
    return;
  }

  const action = spec.scheduled_at ? 'Agendar' : 'Iniciar';
  const when = spec.scheduled_at
    ? ` em ${new Date(spec.scheduled_at).toLocaleString()}`
    : ' agora';
  const skipped = p.recipients_missing_handle > 0
    ? ` (${p.recipients_missing_handle} serão pulados por falta de número)`
    : '';
  if (!confirm(`${action} envio pra ${p.recipients_with_handle} destinatário(s)${skipped}${when}?\nAtraso entre envios: ${state.settings.min_delay_secs}-${state.settings.max_delay_secs}s.`)) {
    return;
  }
  try {
    const id = await invoke('vorcaro_start_campaign', { spec });
    activeCampaignId = id;
    liveCampaignId = id;
    liveProgress.set(id, {
      attempts: [],
      status: spec.scheduled_at ? 'scheduled' : 'running',
      meta: spec,
    });
    // Reset the form's attachment state so the user doesn't accidentally
    // attach the same file to the next campaign.
    campAttachments = [];
    renderAttachmentList();
    // Refresh state to pick up the new campaign record.
    const snap = await invoke('vorcaro_get_state');
    state.campaigns = snap.campaigns || [];
    renderCampaignRecent();
    renderLogsCampaignSelect();
    document.querySelector('.tab[data-tab="logs"]').click();
    renderLogsForActive();
    toast('Campanha iniciada', 'ok');
  } catch (err) {
    toast('Erro: ' + err, 'error');
  }
}

// ── LOGS TAB ────────────────────────────────────────────────────
function bindLogs() {
  document.getElementById('logs-campaign-select').addEventListener('change', e => {
    activeCampaignId = e.target.value || null;
    renderLogsForActive();
  });
  document.getElementById('btn-pause').addEventListener('click',
    () => activeCampaignId && controlCampaign('pause'));
  document.getElementById('btn-resume').addEventListener('click',
    () => activeCampaignId && controlCampaign('resume'));
  document.getElementById('btn-abort').addEventListener('click', () => {
    if (!activeCampaignId) return;
    if (!confirm('Abortar campanha?')) return;
    controlCampaign('abort');
  });
}

async function controlCampaign(action) {
  const id = activeCampaignId;
  const cmd = ({
    pause: 'vorcaro_pause_campaign',
    resume: 'vorcaro_resume_campaign',
    abort: 'vorcaro_abort_campaign',
  })[action];
  try {
    await invoke(cmd, { id });
    toast(`Comando ${action} enviado`, 'ok');
  } catch (err) { toast('Erro: ' + err, 'error'); }
}

function renderLogsCampaignSelect() {
  const sel = document.getElementById('logs-campaign-select');
  const items = [...state.campaigns]
    .sort((a, b) => (b.created_at || '').localeCompare(a.created_at || ''));
  sel.innerHTML = '<option value="">— selecione uma campanha —</option>' +
    items.map(c =>
      `<option value="${c.id}" ${c.id === activeCampaignId ? 'selected' : ''}>${esc(c.name)} (${esc(c.status || 'draft')})</option>`
    ).join('');
}

function renderLogsForActive() {
  const id = activeCampaignId;
  const empty = document.getElementById('logs-empty');
  const body = document.getElementById('logs-body');
  const summary = document.getElementById('logs-summary');
  const pill = document.getElementById('logs-status-pill');

  if (!id) {
    body.innerHTML = '';
    summary.innerHTML = '';
    pill.textContent = '—';
    pill.className = 'pill';
    empty.hidden = false;
    return;
  }
  empty.hidden = true;

  const campaign = state.campaigns.find(c => c.id === id);
  const liveAttempts = (liveProgress.get(id)?.attempts) || [];
  const stored = (campaign?.progress) || [];
  // Live attempts already include everything appended this session. Persisted
  // progress is the source of truth for already-finished items.
  const allAttempts = stored.length >= liveAttempts.length ? stored : liveAttempts;

  const status = (liveProgress.get(id)?.status) || campaign?.status || 'draft';
  pill.textContent = status;
  pill.className = 'pill ' + String(status).toLowerCase();

  const counts = { sent: 0, failed: 0, invalid_number: 0, skipped: 0, queued: 0 };
  allAttempts.forEach(a => { counts[a.status] = (counts[a.status] || 0) + 1; });
  summary.innerHTML = `
    <span class="stat"><b>Total:</b> ${allAttempts.length}</span>
    <span class="stat"><b>✓ Enviados:</b> ${counts.sent}</span>
    <span class="stat"><b>✗ Falha:</b> ${counts.failed}</span>
    <span class="stat"><b># Número inválido:</b> ${counts.invalid_number}</span>
    <span class="stat"><b>↷ Pulados:</b> ${counts.skipped}</span>`;

  body.innerHTML = allAttempts.slice().reverse().map(a => {
    const contact = state.contacts.find(c => c.id === a.contact_id);
    const when = a.at ? new Date(a.at).toLocaleTimeString() : '';
    const stKey = String(a.status).toLowerCase().replace('invalid_number','invalid');
    return `
      <tr>
        <td style="color:var(--fg-dim);font-size:12px">${esc(when)}</td>
        <td>${esc(contact?.display_name || a.contact_id)}</td>
        <td class="status-cell ${stKey}">${esc(a.status)}</td>
        <td style="color:var(--fg-dim);font-size:12px">${esc(a.error || '')}</td>
      </tr>`;
  }).join('') || '<tr><td colspan="4" class="empty">Sem envios ainda</td></tr>';
}

async function listenCampaignProgress() {
  const { listen } = window.__TAURI__.event;
  await listen('vorcaro://campaign-progress', ({ payload }) => {
    const cid = payload.campaign_id;
    if (!liveProgress.has(cid)) liveProgress.set(cid, { attempts: [], status: 'running' });
    const slot = liveProgress.get(cid);

    if (payload.kind === 'attempt') {
      slot.attempts.push(payload.payload);
      // Also reflect into state.campaigns for persistence in UI
      const camp = state.campaigns.find(c => c.id === cid);
      if (camp) {
        camp.progress = camp.progress || [];
        camp.progress.push(payload.payload);
      }
    } else if (payload.kind === 'scheduled') {
      slot.status = 'scheduled';
      const camp = state.campaigns.find(c => c.id === cid);
      if (camp) camp.status = 'scheduled';
    } else if (payload.kind === 'paused' || payload.kind === 'auto-paused' || payload.kind === 'daily-cap-reached') {
      slot.status = 'paused';
      const camp = state.campaigns.find(c => c.id === cid); if (camp) camp.status = 'paused';
      if (payload.kind === 'auto-paused') {
        toast(`Campanha pausada após ${payload.payload.consecutive_failures} falhas seguidas`, 'error');
      } else if (payload.kind === 'daily-cap-reached') {
        toast('Limite diário atingido — campanha pausada', 'error');
      }
    } else if (payload.kind === 'campaign-finished') {
      slot.status = payload.payload.status;
      const camp = state.campaigns.find(c => c.id === cid);
      if (camp) camp.status = payload.payload.status;
    }

    if (cid === activeCampaignId) {
      renderLogsForActive();
    }
    renderCampaignRecent();
    renderLogsCampaignSelect();
  });
}

// ── Utility ──────────────────────────────────────────────────────
function esc(s) {
  if (s == null) return '';
  return String(s)
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');
}
