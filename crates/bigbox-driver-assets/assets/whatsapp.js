
(function(){
  if (window.__vorcaro && window.__vorcaro.version) return;

  function safeInvoke(cmd, payload) {
    try { return window.__TAURI__.core.invoke(cmd, payload); }
    catch (_) {
      try { return window.__TAURI_INTERNALS__.invoke(cmd, payload); }
      catch (__) { return Promise.reject('no tauri bridge'); }
    }
  }

  function extractPhoneFromDataId(s) {
    if (!s) return null;
    var m = String(s).match(/(\d{8,15})@/);
    return m ? '+' + m[1] : null;
  }

  function nameAsPhone(name) {
    var t = String(name || '').replace(/\s+/g, '');
    if (/^\+?\d{8,}$/.test(t)) {
      return t.startsWith('+') ? t : ('+' + t);
    }
    return null;
  }

  // ── WhatsApp Business label filter ─────────────────────────
  //
  // WA Business Web has a "filter by label" UI in the chat-list toolbar.
  // We click it, pick the matching label, wait for the list to refilter,
  // scrape what's visible, then restore the unfiltered "All" view.
  // Selectors are tolerant; if any step fails we surface a clear error.

  function findFilterTrigger() {
    var sels = [
      '[aria-label="Filter"]',
      '[aria-label="Filtrar"]',
      '[aria-label*="ilter" i]',
      '[aria-label*="iltrar" i]',
      '[data-icon="filter"]',
      'span[data-icon="filter"]',
      // Common alt: dropdown beside the search bar
      'button[aria-haspopup="menu"][aria-label*="abel" i]',
    ];
    for (var i = 0; i < sels.length; i++) {
      var el = document.querySelector(sels[i]);
      if (!el) continue;
      return el.closest('button, div[role="button"]') || el;
    }
    return null;
  }

  function closeAnyPopover() {
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    // Click in empty space to dismiss.
    var pane = document.querySelector('#pane-side, #side, header');
    if (pane) try { pane.click(); } catch(_){}
  }

  function leafText(el) {
    if (el.querySelector('[role="button"], [role="menuitem"], [role="option"]')) return null;
    var raw = (el.textContent || '').trim();
    if (!raw || raw.length > 60) return null;
    if (/^[\d\s•·\-]+$/.test(raw)) return null;
    return raw.replace(/\s+/g, ' ').replace(/\s+\d+\s*$/, '').trim();
  }

  // ── WA Business "Etiquetas" navigation ─────────────────────
  //
  // Custom organization labels live behind the chat-pane ☰ menu, NOT the
  // filter button. Path:
  //   ☰ menu  →  "Etiquetas" / "Labels"  →  full-pane list view  →
  //   click a label  →  pane shows that label's chats.
  // We script that path here.

  function findChatPaneMenuButton() {
    // Scope to the chat-list side (#pane-side) when present, fall back to the
    // whole body. No tight geometric clamps; modern WA Web's column widths vary.
    var pane = document.querySelector('#pane-side, #side');
    var paneRect = pane ? pane.getBoundingClientRect() : null;
    var scope = pane || document.body;
    var candidates = scope.querySelectorAll(
      'button, div[role="button"], span[data-icon], [aria-label]'
    );
    var hits = [];
    for (var i = 0; i < candidates.length; i++) {
      var el = candidates[i];
      var rect = el.getBoundingClientRect();
      // Visible and in the top portion of the pane only.
      if (rect.width === 0 || rect.height === 0) continue;
      if (rect.top > 200) continue;
      if (paneRect) {
        // Element must be inside the chat-list column.
        if (rect.left < paneRect.left - 4 || rect.right > paneRect.right + 4) continue;
      }

      var aria = (el.getAttribute('aria-label') || '').toLowerCase();
      var title = (el.getAttribute('title') || '').toLowerCase();
      var icon = '';
      if (el.tagName === 'SPAN') icon = el.getAttribute('data-icon') || '';
      if (!icon) {
        var iconEl = el.querySelector && el.querySelector('[data-icon]');
        if (iconEl) icon = iconEl.getAttribute('data-icon') || '';
      }
      var hay = aria + ' ' + title + ' ' + icon;
      var isMenu = /\b(menu|mais op|more opt|more|kebab|ellipsis|opções)\b/.test(hay)
                || /(^|-)more(-|$|[a-z])/.test(icon)
                || /(^|-)menu(-|$|[a-z])/.test(icon);
      if (isMenu) {
        var btn = el.tagName === 'SPAN' ? el.closest('button, div[role="button"]') : el;
        // Some aria-only matches aren't real buttons; require some clickable wrapper.
        if (!btn) btn = el.closest('button, [role="button"]');
        if (btn) hits.push(btn);
      }
    }
    if (hits.length === 0) return null;
    // The kebab is usually the right-most icon in the top bar.
    hits.sort(function(a, b) {
      return b.getBoundingClientRect().left - a.getBoundingClientRect().left;
    });
    return hits[0];
  }

  // Diagnostic helper: dump every clickable / labeled element in the chat
  // pane (or whole body as fallback). No geometric filter — we want to see
  // EVERYTHING so we can build proper selectors from real data.
  function debugChatPane() {
    var lines = [];
    lines.push('url: ' + location.href);
    lines.push('readyState: ' + document.readyState);
    lines.push('body children: ' + (document.body ? document.body.children.length : 0));
    lines.push('WA loaded: ' + isWaLoaded());
    var pane = document.querySelector('#pane-side, #side');
    lines.push('pane-side present: ' + !!pane);
    lines.push('chat-list-filters present: ' +
      !!document.querySelector('[role="tablist"][aria-label="chat-list-filters"]'));
    lines.push('chat grid present: ' +
      !!document.querySelector('[aria-label="Lista de conversas"]'));
    lines.push('---');

    // No geometry filters — when the WA WebView is hidden by BigBox, every
    // bounding rect is zero. We just want to see what the DOM contains.
    var all = document.body
      ? document.body.querySelectorAll('button, [role="button"], [aria-label], [data-icon], span[data-icon]')
      : [];
    var seen = Object.create(null);
    var count = 0;
    for (var i = 0; i < all.length && count < 120; i++) {
      var el = all[i];
      var aria = el.getAttribute('aria-label') || '';
      var title = el.getAttribute('title') || '';
      var roleA = el.getAttribute('role') || '';
      var icon = '';
      if (el.tagName === 'SPAN') icon = el.getAttribute('data-icon') || '';
      if (!icon) {
        var ic = el.querySelector && el.querySelector('[data-icon]');
        if (ic) icon = ic.getAttribute('data-icon') || '';
      }
      var text = (el.textContent || '').trim().replace(/\s+/g, ' ').slice(0, 50);
      if (!aria && !title && !icon && !roleA && !text) continue;
      var summary = '<' + el.tagName.toLowerCase()
        + (roleA ? ' role="' + roleA + '"' : '')
        + (aria ? ' aria="' + aria + '"' : '')
        + (title ? ' title="' + title + '"' : '')
        + (icon ? ' data-icon="' + icon + '"' : '')
        + '>' + (text ? ' text="' + text + '"' : '');
      if (seen[summary]) continue;
      seen[summary] = true;
      lines.push(summary);
      count++;
    }
    lines.push('---');
    lines.push('total candidatos: ' + all.length + ', exibidos: ' + count);
    safeInvoke('vorcaro_debug_dom_result', { dump: lines.join('\n') });
  }

  function findMenuItemByText(words) {
    var items = document.querySelectorAll(
      '[role="menuitem"], li, div[role="button"], div[tabindex], button'
    );
    for (var i = 0; i < items.length; i++) {
      var t = (items[i].textContent || '').trim().toLowerCase();
      if (t.length === 0 || t.length > 40) continue;
      for (var w = 0; w < words.length; w++) {
        if (t === words[w] || t.indexOf(words[w]) === 0) return items[i];
      }
    }
    return null;
  }

  // The labels feature on modern WA Business Web (2025+) lives on the chat-list
  // filters tablist — a row of tabs above the chat list with id/aria
  // "chat-list-filters". Tabs: Tudo | Não lidas | Favoritas | Grupos | Etiquetas
  // The "Etiquetas" tab has a dropdown arrow; clicking it pops up a menu of
  // the user's custom labels. No ☰ menu navigation needed.

  function isWaLoaded() {
    return !!(
         document.querySelector('[role="tablist"][aria-label="chat-list-filters"]')
      || document.querySelector('[aria-label="Lista de conversas"]')
      || document.querySelector('[aria-label="Chat list"]')
      || document.querySelector('#pane-side, #side')
    );
  }

  function findFiltersTablist() {
    // Global lookup — no #pane-side dependency, no geometry checks.
    // When BigBox hides the WA WebView (user navigated to Vorcaro tab),
    // getBoundingClientRect returns zero on every element, so a geometric
    // filter would discard everything even though the DOM is intact.
    return document.querySelector('[role="tablist"][aria-label="chat-list-filters"]')
        || document.querySelector('[aria-label="chat-list-filters"][role="tablist"]')
        || document.querySelector('[role="tablist"]');
  }

  function findTabByText(words) {
    var tablist = findFiltersTablist();
    if (!tablist) return null;
    var tabs = tablist.querySelectorAll('[role="tab"], button, div[role="button"]');
    for (var i = 0; i < tabs.length; i++) {
      var t = (tabs[i].textContent || '').trim().toLowerCase();
      // WA appends an icon name as text (e.g. "Etiquetasic-arrow-drop-down");
      // we match the prefix.
      for (var w = 0; w < words.length; w++) {
        if (t === words[w] || t.indexOf(words[w]) === 0) return tabs[i];
      }
    }
    return null;
  }

  // Ensure the "Etiquetas" dropdown is open. The Etiquetas tab is a toggle:
  // a single click may CLOSE an already-open dropdown. To be robust we check
  // if label items are already visible; if not, click and check; if still
  // not, click again (in case the first click toggled it shut).
  async function ensureLabelsDropdownOpen() {
    if (findVisibleLabelItems().length > 0) return { ok: true };

    var tab = findTabByText(['etiquetas', 'labels', 'etiqueta']);
    if (!tab) {
      return { ok: false, error: 'aba "Etiquetas" não encontrada na barra de filtros' };
    }

    for (var attempt = 0; attempt < 3; attempt++) {
      tab.click();
      await new Promise(function(r){ setTimeout(r, 600); });
      if (findVisibleLabelItems().length > 0) return { ok: true };
    }

    return { ok: false, error: 'cliquei na aba "Etiquetas" 3x e o dropdown não populou' };
  }

  // Backward-compat alias for callers still expecting the old signature.
  async function openLabelsDropdown() {
    var r = await ensureLabelsDropdownOpen();
    return { ok: r.ok, error: r.error, addedRoots: [] };
  }

  function normalizeLabelText(raw) {
    if (!raw) return null;
    // Strip WA's icon-name prefixes ("ic-label-filled", "ic-arrow-drop-down").
    // The pattern is kebab-case, ALL LOWERCASE. Match must be case-sensitive
    // or it will swallow the label name (e.g. "ic-label-filledAssinante Insta"
    // becomes "Insta" instead of "Assinante Insta" when /i is on).
    var t = raw.replace(/ic-[a-z0-9\-]+/g, '');
    t = t.replace(/\s+\d+\s*$/, '');
    t = t.replace(/\s+/g, ' ').trim();
    if (!t || t.length < 2 || t.length > 60) return null;
    if (/^(close|fechar|search|pesquisar|cancel|cancelar|done|concluir|ok|nova lista)$/i.test(t)) return null;
    if (/^[\d\s•·\-]+$/.test(t)) return null;
    return t;
  }

  // Find WA-Business label items anywhere in the document. Each label row
  // contains an `ic-label-...` icon text marker — robust signature regardless
  // of whether the popover was newly added or shown from a hidden state.
  // No geometry filtering: when BigBox hides the WA WebView, every rect is
  // zero, but the DOM elements and their handlers are still live.
  function findVisibleLabelItems() {
    var out = [];
    var seenText = Object.create(null);
    var all = document.querySelectorAll(
      'li[role="button"], button, [role="menuitem"], [role="option"]'
    );
    for (var i = 0; i < all.length; i++) {
      var el = all[i];
      var raw = (el.textContent || '').trim();
      if (!/ic-label[\-\w]*/.test(raw)) continue;
      var t = normalizeLabelText(raw);
      if (!t) continue;
      var key = t.toLowerCase();
      // The label appears twice in the DOM (once as the <li> wrapper and once
      // as the <button> inside) — dedupe by text and prefer the button for clicks.
      if (seenText[key]) {
        if (el.tagName === 'BUTTON') {
          for (var j = 0; j < out.length; j++) {
            if (out[j].name.toLowerCase() === key && out[j].el.tagName !== 'BUTTON') {
              out[j].el = el;
              break;
            }
          }
        }
        continue;
      }
      seenText[key] = true;
      out.push({ el: el, name: t });
    }
    return out;
  }

  function scrapeLabelsFromDropdown(_addedRoots) {
    return findVisibleLabelItems().map(function(x) { return x.name; });
  }

  function dumpDropdownContents(addedRoots) {
    // Diagnostic: dump every clickable/leaf element across all added roots
    // so we can see why some labels weren't picked up. Caps at 100 entries.
    var lines = ['dropdown raw dump:'];
    var count = 0;
    for (var k = 0; k < addedRoots.length && count < 100; k++) {
      var root = addedRoots[k];
      if ((root.textContent || '').trim().length < 4) continue;
      var items = root.querySelectorAll(
        '[role="menuitem"], [role="option"], [role="row"], [role="listitem"], li, button, [role="button"], div[tabindex], div[onclick]'
      );
      for (var i = 0; i < items.length && count < 100; i++) {
        var el = items[i];
        var raw = (el.textContent || '').trim().replace(/\s+/g, ' ').slice(0, 60);
        var roleA = el.getAttribute('role') || '';
        var ariaA = el.getAttribute('aria-label') || '';
        var summary = '<' + el.tagName.toLowerCase()
          + (roleA ? ' role="' + roleA + '"' : '')
          + (ariaA ? ' aria="' + ariaA + '"' : '')
          + '> text="' + raw + '"';
        lines.push(summary);
        count++;
      }
    }
    lines.push('--- total exibido: ' + count);
    return lines.join('\n');
  }

  function findLabelInDropdown(_addedRoots, want) {
    want = want.trim().toLowerCase();
    var items = findVisibleLabelItems();
    for (var i = 0; i < items.length; i++) {
      var key = items[i].name.toLowerCase();
      if (key === want || key.indexOf(want) === 0) return items[i].el;
    }
    return null;
  }

  function clickBackButton() {
    // Try ESC first; cheap and frequently effective.
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    var backIcons = document.querySelectorAll(
      '[data-icon="back"], [data-icon="back-light"], [data-icon="back-refreshed"], [data-icon="back-refreshed-thin"]'
    );
    for (var i = 0; i < backIcons.length; i++) {
      var btn = backIcons[i].closest('button, div[role="button"]');
      if (btn) { btn.click(); return true; }
    }
    var ariaBack = document.querySelectorAll('[aria-label*="ack" i], [aria-label*="oltar" i]');
    if (ariaBack.length > 0) {
      var b = ariaBack[0].closest('button, div[role="button"]') || ariaBack[0];
      b.click();
      return true;
    }
    return false;
  }

  async function navigateBackToChatList(times) {
    times = times || 1;
    for (var i = 0; i < times; i++) {
      clickBackButton();
      await new Promise(function(r){ setTimeout(r, 350); });
    }
  }

  function scrapeLabelsFromCurrentView() {
    var pane = document.querySelector('#pane-side, #side');
    if (!pane) return [];
    var labels = [];
    var seen = Object.create(null);

    // Strategy 1: rows with span[title] but NO avatar img (avatar = chat row).
    pane.querySelectorAll('span[title]:not([title=""])').forEach(function(span) {
      var row = span.closest('[role="listitem"], [role="row"], li, div[tabindex]');
      // Chat rows have an avatar <img>. Label rows usually don't.
      if (row && row.querySelector('img')) return;
      var t = (span.getAttribute('title') || '').trim();
      if (!t || t.length > 60) return;
      var key = t.toLowerCase();
      if (seen[key]) return;
      seen[key] = true;
      labels.push(t);
    });

    // Strategy 2: data-icon="tag" or label-shaped icons next to text rows.
    if (labels.length === 0) {
      var tagIcons = pane.querySelectorAll(
        '[data-icon="tag"], [data-icon^="label"], [data-icon="tag-thin"], [data-icon="label-refreshed"]'
      );
      tagIcons.forEach(function(icon) {
        var row = icon.closest('[role="listitem"], [role="row"], li, div[tabindex]');
        if (!row) return;
        var titleEl = row.querySelector('span[title]') || row.querySelector('span[dir="auto"]');
        if (!titleEl) return;
        var t = (titleEl.getAttribute('title') || titleEl.textContent || '').trim();
        if (!t || t.length > 60) return;
        var key = t.toLowerCase();
        if (seen[key]) return;
        seen[key] = true;
        labels.push(t);
      });
    }

    return labels;
  }

  function findLabelRowByName(name) {
    var pane = document.querySelector('#pane-side, #side');
    if (!pane) return null;
    var want = name.trim().toLowerCase();
    // span[title] in non-avatar rows.
    var spans = pane.querySelectorAll('span[title]:not([title=""])');
    for (var i = 0; i < spans.length; i++) {
      var row = spans[i].closest('[role="listitem"], [role="row"], li, div[tabindex]');
      if (row && row.querySelector('img')) continue;
      var t = (spans[i].getAttribute('title') || '').trim().toLowerCase();
      if (t === want || t.indexOf(want) === 0) {
        // The clickable row is usually the closest tabindex/listitem.
        return row || spans[i].closest('button, div[role="button"]') || spans[i];
      }
    }
    return null;
  }

  // List built-in chat filters AND custom WA-Business labels.
  //
  // We try several strategies because WA Web's UI keeps changing:
  //   1. Filter chips already visible at the top of the chat list (modern
  //      WA Business 2024-2025 puts All/Unread/Groups + custom labels as
  //      horizontal chips, no menu click required).
  //   2. Filter popover triggered by clicking a filter button (older variant).
  //   3. Main menu → Etiquetas / Labels (most reliable for custom labels but
  //      different DOM in different versions).
  //
  // We union the results so the user sees whatever the UI exposes.

  function scrapeVisibleChips() {
    var pane = document.querySelector('#pane-side, #side');
    if (!pane) return [];
    var firstChat = pane.querySelector('[role="listitem"], [role="row"]');
    if (!firstChat) return [];
    var firstTop = firstChat.getBoundingClientRect().top;

    var candidates = pane.querySelectorAll(
      '[role="tab"], [role="button"][aria-pressed], button[aria-pressed], [role="button"]'
    );
    var found = [];
    var seen = Object.create(null);
    candidates.forEach(function(el) {
      var rect = el.getBoundingClientRect();
      if (rect.bottom > firstTop) return; // not above the chat list
      if (rect.width < 20 || rect.height < 18) return;
      var t = leafText(el);
      if (!t) return;
      var key = t.toLowerCase();
      if (seen[key]) return;
      seen[key] = true;
      found.push(t);
    });
    return found;
  }

  async function scrapeFromFilterPopover() {
    var trigger = findFilterTrigger();
    if (!trigger) return [];

    var addedRoots = [];
    var obs = new MutationObserver(function(mutations) {
      for (var i = 0; i < mutations.length; i++) {
        var nodes = mutations[i].addedNodes;
        for (var j = 0; j < nodes.length; j++) {
          if (nodes[j].nodeType === 1) addedRoots.push(nodes[j]);
        }
      }
    });
    obs.observe(document.body, { childList: true, subtree: true });
    trigger.click();
    await new Promise(function(r){ setTimeout(r, 700); });
    obs.disconnect();

    if (addedRoots.length === 0) return [];

    addedRoots.sort(function(a, b) {
      return (b.textContent || '').length - (a.textContent || '').length;
    });

    var found = [];
    var seen = Object.create(null);
    for (var k = 0; k < addedRoots.length; k++) {
      var root = addedRoots[k];
      if ((root.textContent || '').trim().length < 4) continue;
      var items = root.querySelectorAll(
        '[role="button"], [role="option"], [role="menuitem"], li, button, div[tabindex]'
      );
      for (var i = 0; i < items.length; i++) {
        var t = leafText(items[i]);
        if (!t) continue;
        var key = t.toLowerCase();
        if (seen[key]) continue;
        seen[key] = true;
        found.push(t);
      }
      if (found.length > 0) break;
    }

    closeAnyPopover();
    return found;
  }

  async function listLabels() {
    if (!isWaLoaded()) {
      safeInvoke('vorcaro_wa_labels_result', {
        labels: [],
        error: 'WhatsApp Web não está carregado nesta aba. Clique no ícone do WA no menu lateral do BigBox, espere a lista de conversas aparecer, depois volte aqui.',
      });
      return;
    }
    var opened = await ensureLabelsDropdownOpen();
    if (!opened.ok) {
      // Distinguish "this account has no Etiquetas tab" (personal WA) from
      // a real failure to find a Business UI.
      var tablist = findFiltersTablist();
      if (tablist && !findTabByText(['etiquetas', 'labels', 'etiqueta'])) {
        safeInvoke('vorcaro_wa_labels_result', {
          labels: [],
          error: 'esta conta WhatsApp não tem a aba "Etiquetas" — etiquetas são um recurso só do WA Business. Faça login com uma conta Business neste workspace para usar este filtro.',
        });
      } else {
        try { debugChatPane(); } catch (_) {}
        safeInvoke('vorcaro_wa_labels_result', { labels: [], error: opened.error });
      }
      return;
    }
    var labels = findVisibleLabelItems().map(function(x){ return x.name; });

    // Close the dropdown.
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));

    safeInvoke('vorcaro_wa_labels_result', {
      labels: labels,
      error: labels.length === 0
        ? 'dropdown abriu mas nenhuma etiqueta reconhecida — sua conta talvez não tenha etiquetas configuradas'
        : null,
    });
  }

  async function applyLabelFilter(labelName) {
    var want = String(labelName || '').trim();
    if (!want) return { ok: true };

    var opened = await ensureLabelsDropdownOpen();
    if (!opened.ok) return { ok: false, error: opened.error };

    var items = findVisibleLabelItems();
    var wantLower = want.toLowerCase();
    for (var i = 0; i < items.length; i++) {
      var key = items[i].name.toLowerCase();
      if (key === wantLower || key.indexOf(wantLower) === 0) {
        items[i].el.click();
        await new Promise(function(r){ setTimeout(r, 900); });
        return { ok: true };
      }
    }

    // Auto-debug: show what we actually found so the dev/user can see the mismatch
    try {
      safeInvoke('vorcaro_debug_dom_result', {
        dump: 'applyLabelFilter procurando "' + labelName + '"\n'
          + 'mas o dropdown atual lista ' + items.length + ' etiqueta(s):\n  '
          + items.map(function(x){ return x.name; }).join('\n  '),
      });
    } catch (_) {}

    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    return { ok: false, error: 'etiqueta "' + labelName + '" não está no dropdown (' + items.length + ' etiquetas visíveis — modal de diag abriu)' };
  }

  async function clearLabelFilter() {
    // Restore the unfiltered chat list by clicking the "Tudo" / "All" tab.
    var allTab = findTabByText(['tudo', 'all', 'todas', 'todos']);
    if (allTab) {
      allTab.click();
    } else {
      document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    }
    await new Promise(function(r){ setTimeout(r, 400); });
  }

  // ── Phone extraction by clicking each chat ──────────────────
  //
  // WA Web hides phone numbers in the chat list DOM for saved contacts (it
  // uses `lid_xxx@lid` rather than `<phone>@c.us`). To recover them we have
  // to open each chat and read the phone from the conversation header or
  // contact-info panel. Slow (~3s/chat) but automatic.

  function scanForPhone(scope) {
    // Phone patterns — with OR without leading "+", BR-style with parens, etc.
    var rx = [
      /\+\s?\d{1,3}\s?\(?\d{2,4}\)?[\s\-]?\d{3,5}[\s\-]?\d{3,5}/,
      /\+\d{10,15}/,
      /\(\d{2,3}\)\s?\d{4,5}[\-\s]?\d{4}/,           // (61) 91234-5678
      /\b\d{2,3}\s\d{4,5}[\-\s]?\d{4}\b/,             // 55 11 91234 5678
    ];
    var walker = document.createTreeWalker(scope, NodeFilter.SHOW_TEXT, null, false);
    var node;
    while ((node = walker.nextNode())) {
      var text = node.nodeValue;
      if (!text || text.length < 7) continue;
      for (var i = 0; i < rx.length; i++) {
        var m = text.match(rx[i]);
        if (!m) continue;
        var digits = m[0].replace(/[^\d]/g, '');
        if (digits.length >= 10 && digits.length <= 15) {
          // If the match had no '+' but starts with 0 (Brazilian carrier code), strip it.
          if (m[0].indexOf('+') === -1 && digits.charAt(0) === '0') {
            digits = digits.slice(1);
          }
          return '+' + digits;
        }
      }
    }
    return null;
  }

  function findChatGrid() {
    return document.querySelector('[role="grid"][aria-label="Lista de conversas"]')
        || document.querySelector('[role="grid"][aria-label*="onversas" i]')
        || document.querySelector('[role="grid"][aria-label*="hat" i]')
        || document.querySelector('[role="grid"]');
  }

  function findChatRowByName(name) {
    // Scope to the chat-list grid only — searching globally caused matches
    // against the contact-info drawer (which also has role="listitem" entries).
    var grid = findChatGrid();
    var scope = grid || document;
    var items = scope.querySelectorAll('div[role="listitem"], div[role="row"]');
    for (var i = 0; i < items.length; i++) {
      var nameEl = items[i].querySelector('span[title][dir="auto"]')
                || items[i].querySelector('span[title]');
      if (!nameEl) continue;
      var t = (nameEl.getAttribute('title') || nameEl.textContent || '').trim();
      if (t === name) return items[i];
    }
    return null;
  }

  function dispatchRealClick(el) {
    // .click() alone doesn't always trigger React handlers. Send a real
    // pointer sequence first, then fall back to click.
    var rect = el.getBoundingClientRect();
    var cx = rect.left + rect.width / 2 || 0;
    var cy = rect.top + rect.height / 2 || 0;
    try {
      el.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, clientX: cx, clientY: cy, button: 0 }));
      el.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, clientX: cx, clientY: cy, button: 0 }));
      el.dispatchEvent(new PointerEvent('pointerup', { bubbles: true, clientX: cx, clientY: cy, button: 0 }));
      el.dispatchEvent(new MouseEvent('mouseup', { bubbles: true, clientX: cx, clientY: cy, button: 0 }));
      el.dispatchEvent(new MouseEvent('click', { bubbles: true, clientX: cx, clientY: cy, button: 0 }));
    } catch (_) {}
    try { el.click(); } catch (_) {}
  }

  // Identify the conversation header (the <header> on the right side, NOT the
  // chat-list header on the left). Uses DOM containment, not geometry — geometry
  // is unreliable when BigBox hides the WA WebView.
  function findConversationHeader() {
    var headers = document.querySelectorAll('header');
    var pane = document.querySelector('#pane-side, #side');
    // First pass: any header that is NOT inside the chat-list pane.
    for (var i = 0; i < headers.length; i++) {
      if (pane && pane.contains(headers[i])) continue;
      if (headers[i].querySelector('span[title]')) return headers[i];
    }
    // Fallback: take the last header that has a title span.
    for (var j = headers.length - 1; j >= 0; j--) {
      if (headers[j].querySelector('span[title]')) return headers[j];
    }
    return null;
  }

  var __debugDumpedOnce = false;

  function findDrawerCloseButton() {
    // The contact-info drawer has a Fechar/Close button (ic-close icon).
    // Find via aria-label first, then via the icon span.
    var btns = document.querySelectorAll(
      'button[aria-label="Fechar"], button[aria-label="Close"], ' +
      '[role="button"][aria-label="Fechar"], [role="button"][aria-label="Close"]'
    );
    if (btns.length > 0) return btns[0];
    var icon = document.querySelector('span[data-icon="ic-close"], span[data-icon="x-light"]');
    if (icon) return icon.closest('button, [role="button"]');
    return null;
  }

  async function closeDrawerIfOpen() {
    var btn = findDrawerCloseButton();
    if (btn) {
      dispatchRealClick(btn);
      await new Promise(function(r){ setTimeout(r, 400); });
    } else {
      document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
      await new Promise(function(r){ setTimeout(r, 200); });
    }
  }

  // ── IndexedDB phone lookup ──────────────────────────────────
  //
  // WhatsApp Web stores all contacts (with phone numbers) in the
  // `model-storage` IndexedDB. Reading them directly is faster, more
  // reliable, and doesn't depend on clicking around the UI. The schema:
  //   DB: model-storage
  //   ObjectStore: contact
  //   Record: { id: { user, server, _serialized }, name, pushname, ... }
  // We build a { display_name: '+phone' } map once per scrape.

  function openIdbPromise(name) {
    return new Promise(function(resolve) {
      try {
        var req = indexedDB.open(name);
        req.onsuccess = function() { resolve(req.result); };
        req.onerror = function() { resolve(null); };
        req.onblocked = function() { resolve(null); };
      } catch (e) {
        resolve(null);
      }
    });
  }

  function getAllFromStore(db, storeName) {
    return new Promise(function(resolve) {
      try {
        var tx = db.transaction([storeName], 'readonly');
        var os = tx.objectStore(storeName);
        var req = os.getAll();
        req.onsuccess = function() { resolve(req.result || []); };
        req.onerror = function() { resolve([]); };
      } catch (e) {
        resolve([]);
      }
    });
  }

  function extractIdFields(record) {
    // Try several shapes WA has used over the years.
    var phone = null;
    var name = record.name || record.pushname || record.formattedName
            || record.shortName || record.notifyName || record.verifiedName;
    if (record.id) {
      // Reject groups (g.us) and broadcasts (broadcast) — their IDs are
      // <creator-phone>-<timestamp>@g.us. Extracting digits from those
      // produces a fake phone that the deep-link can't resolve, and the
      // driver ends up typing into whatever chat was previously visible.
      var serialized = (typeof record.id === 'string') ? record.id
                    : (record.id._serialized || '');
      var server = (record.id && record.id.server) || '';
      if (/@g\.us$/.test(serialized) || server === 'g.us'
          || /@broadcast$/.test(serialized) || server === 'broadcast'
          || /@newsletter$/.test(serialized) || server === 'newsletter') {
        return { phone: null, name: name };
      }
      // Accept only individual contact servers.
      if (typeof record.id === 'string') {
        var m = record.id.match(/^(\d{8,15})@(c\.us|s\.whatsapp\.net|lid)/);
        if (m && m[2] !== 'lid') phone = '+' + m[1];
      } else if (record.id.user && record.id.server
                 && (record.id.server === 'c.us' || record.id.server === 's.whatsapp.net')
                 && /^\d{8,15}$/.test(String(record.id.user))) {
        phone = '+' + record.id.user;
      } else if (record.id._serialized) {
        var m2 = String(record.id._serialized).match(/^(\d{8,15})@(c\.us|s\.whatsapp\.net)/);
        if (m2) phone = '+' + m2[1];
      }
    }
    return { phone: phone, name: name };
  }

  async function buildIdbContactMap() {
    var map = {};
    var dbList;
    try {
      dbList = (indexedDB.databases ? await indexedDB.databases() : [])
        .filter(function(d){ return d && d.name; })
        .map(function(d){ return d.name; });
    } catch (e) {
      dbList = [];
    }
    // Fallback list of known WA Web DBs in case databases() is unavailable
    // or returns empty.
    var fallback = ['model-storage', 'wam', 'wam-store', 'wam-deferred-store',
                    'signal-storage', 'dynamicMessages'];
    fallback.forEach(function(n) { if (dbList.indexOf(n) < 0) dbList.push(n); });

    for (var i = 0; i < dbList.length; i++) {
      var db = await openIdbPromise(dbList[i]);
      if (!db) continue;
      var stores = Array.from(db.objectStoreNames);
      // Look at any store that might hold contacts.
      for (var j = 0; j < stores.length; j++) {
        if (!/contact/i.test(stores[j])) continue;
        var records = await getAllFromStore(db, stores[j]);
        records.forEach(function(rec) {
          var x = extractIdFields(rec);
          if (x.name && x.phone) {
            map[x.name] = x.phone;
            // Also key by lowercase + normalized whitespace for tolerant lookup
            map[x.name.toLowerCase().replace(/\s+/g, ' ')] = x.phone;
          }
        });
      }
      try { db.close(); } catch(_){}
    }
    return map;
  }

  function lookupIdbPhone(map, name) {
    if (!name) return null;
    if (map[name]) return map[name];
    var key = name.toLowerCase().replace(/\s+/g, ' ');
    return map[key] || null;
  }

  function findSearchInput() {
    // The chat-list search field. WA uses an <input role="textbox"> with the
    // "Pesquisar ou começar uma nova conversa" placeholder.
    return document.querySelector('input[role="textbox"]')
        || document.querySelector('[role="textbox"][aria-label*="esquisar" i]')
        || document.querySelector('[role="textbox"][aria-label*="earch" i]')
        || document.querySelector('div[contenteditable="true"][role="textbox"]');
  }

  function setInputValue(el, val) {
    // React's controlled inputs bypass the native setter — go through the
    // descriptor.
    if (el.tagName === 'INPUT' || el.tagName === 'TEXTAREA') {
      var proto = Object.getPrototypeOf(el);
      var desc = Object.getOwnPropertyDescriptor(proto, 'value');
      if (desc && desc.set) desc.set.call(el, val);
      else el.value = val;
      el.dispatchEvent(new Event('input', { bubbles: true }));
      el.dispatchEvent(new Event('change', { bubbles: true }));
    } else {
      // contenteditable
      el.textContent = val;
      el.dispatchEvent(new InputEvent('input', { bubbles: true }));
    }
  }

  async function openChatViaSearch(name) {
    var input = findSearchInput();
    if (!input) return false;

    // Focus + clear + type
    dispatchRealClick(input);
    try { input.focus(); } catch (_) {}
    await new Promise(function(r){ setTimeout(r, 150); });

    setInputValue(input, '');
    await new Promise(function(r){ setTimeout(r, 100); });
    setInputValue(input, name);
    await new Promise(function(r){ setTimeout(r, 800); });

    // Press Enter to open the top match.
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', code: 'Enter', bubbles: true }));
    input.dispatchEvent(new KeyboardEvent('keyup',   { key: 'Enter', code: 'Enter', bubbles: true }));
    await new Promise(function(r){ setTimeout(r, 900); });

    return true;
  }

  async function clearSearch() {
    var input = findSearchInput();
    if (!input) return;
    setInputValue(input, '');
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    await new Promise(function(r){ setTimeout(r, 200); });
  }

  async function extractPhoneByClickingChat(name) {
    // Make sure no stale drawer from a previous chat is still open.
    await closeDrawerIfOpen();

    // Open chat via search bar instead of clicking the chat list row.
    // The row-click approach failed because WA's React event handlers don't
    // reliably respond to synthetic clicks on virtualized chat-list rows.
    // The search input is a regular <input> that responds to programmatic
    // value changes + Enter key.
    var opened = await openChatViaSearch(name);
    if (!opened) return null;

    // Click the conversation header to open the contact-info drawer.
    var convHeader = findConversationHeader();
    if (!convHeader) return null;
    var titleArea = convHeader.querySelector('span[title]');
    var headerBtn = titleArea
      ? (titleArea.closest('div[role="button"], button') || titleArea)
      : convHeader;

    var addedRoots = [];
    var obs = new MutationObserver(function(muts) {
      for (var i = 0; i < muts.length; i++) {
        muts[i].addedNodes.forEach(function(n) {
          if (n.nodeType === 1) addedRoots.push(n);
        });
      }
    });
    obs.observe(document.body, { childList: true, subtree: true });

    dispatchRealClick(headerBtn);
    await new Promise(function(r){ setTimeout(r, 900); });
    obs.disconnect();

    addedRoots.sort(function(a, b) {
      return (b.textContent || '').length - (a.textContent || '').length;
    });

    // Verify drawer is for the expected contact: its text must contain the
    // expected name as a substring (case-insensitive, whitespace-normalized).
    // The drawer renders the contact name multiple times (Bloquear X / Denunciar X
    // buttons, plus the title heading), so substring matching is reliable.
    function normalize(s) {
      return (s || '').toLowerCase().replace(/\s+/g, ' ').trim();
    }
    var expectedKey = normalize(name);
    var matchedRoot = null;
    for (var k = 0; k < addedRoots.length; k++) {
      if ((addedRoots[k].textContent || '').length < 10) continue;
      if (normalize(addedRoots[k].textContent).indexOf(expectedKey) >= 0) {
        matchedRoot = addedRoots[k];
        break;
      }
    }

    var phone = null;
    if (matchedRoot) {
      phone = scanForPhone(matchedRoot);
    }

    // Diagnostic dump for the first chat — always, regardless of outcome.
    if (!__debugDumpedOnce) {
      __debugDumpedOnce = true;
      try {
        var dump = 'extractPhoneByClickingChat — "' + name + '"\n';
        dump += 'phone extraído: ' + (phone || '(null)') + '\n';
        dump += 'addedRoots: ' + addedRoots.length + '\n';
        dump += 'drawer match para nome esperado: ' + !!matchedRoot + '\n';
        dump += '---\n';
        for (var r2 = 0; r2 < Math.min(addedRoots.length, 2); r2++) {
          var root = addedRoots[r2];
          var rText = root.textContent || '';
          dump += '\n[ROOT ' + r2 + ' textLen=' + rText.length
            + ' containsExpected=' + (normalize(rText).indexOf(expectedKey) >= 0) + ']\n';
          var spans = root.querySelectorAll(
            'span[dir="auto"], span[title], [data-icon], h1, h2, h3, [aria-label]'
          );
          var shown = 0;
          for (var s2 = 0; s2 < spans.length && shown < 30; s2++) {
            var txt = (spans[s2].textContent || '').trim();
            if (!txt || txt.length > 100) continue;
            var icon = spans[s2].getAttribute('data-icon') || '';
            var aria = spans[s2].getAttribute('aria-label') || '';
            dump += '<' + spans[s2].tagName.toLowerCase()
              + (icon ? ' icon=' + icon : '') + (aria ? ' aria=' + aria : '')
              + '> "' + txt + '"\n';
            shown++;
          }
        }
        safeInvoke('vorcaro_debug_dom_result', { dump: dump });
      } catch (_) {}
    }

    // Close the drawer via the explicit Fechar button (ESC alone wasn't enough
    // — left the drawer open and blocked the next chat's interaction).
    await closeDrawerIfOpen();

    return phone;
  }

  function dumpFirstChatRowsStructure() {
    var grid = findChatGrid();
    if (!grid) return '(no grid found)';
    var rows = grid.querySelectorAll('div[role="listitem"], div[role="row"]');
    var lines = ['First chat rows structure:'];
    lines.push('grid role: ' + (grid.getAttribute('role') || '')
      + ' aria: "' + (grid.getAttribute('aria-label') || '') + '"');
    lines.push('total rows: ' + rows.length);
    for (var i = 0; i < Math.min(rows.length, 5); i++) {
      var r = rows[i];
      var nameEl = r.querySelector('span[title]');
      var name = nameEl ? (nameEl.getAttribute('title') || '').slice(0, 50) : '(no name)';
      lines.push('\n--- ROW ' + i + ' "' + name + '" ---');
      lines.push('  row.tagName=' + r.tagName + ' role="' + (r.getAttribute('role') || '') + '"');
      // Any data-id / id anywhere inside the row
      var attrEls = r.querySelectorAll('[data-id], [id]');
      var shown = 0;
      for (var j = 0; j < attrEls.length && shown < 8; j++) {
        var did = attrEls[j].getAttribute('data-id');
        var iid = attrEls[j].getAttribute('id');
        if (!did && !iid) continue;
        lines.push('  <' + attrEls[j].tagName.toLowerCase() + '>'
          + (did ? ' data-id="' + did + '"' : '')
          + (iid ? ' id="' + iid + '"' : ''));
        shown++;
      }
      // Clickable children
      var clickables = r.querySelectorAll('button, [role="button"], [tabindex]');
      lines.push('  clickables: ' + clickables.length);
      for (var k = 0; k < Math.min(clickables.length, 4); k++) {
        var c = clickables[k];
        lines.push('    <' + c.tagName.toLowerCase()
          + ' role="' + (c.getAttribute('role') || '') + '"'
          + ' tabindex="' + (c.getAttribute('tabindex') || '') + '"'
          + ' aria="' + (c.getAttribute('aria-label') || '').slice(0, 40) + '">');
      }
    }
    return lines.join('\n');
  }

  async function scrapeChats(platform, opts) {
    opts = opts || {};
    if (!isWaLoaded()) {
      safeInvoke('vorcaro_scrape_result', {
        platform: platform,
        rows: [],
        error: 'WhatsApp Web não está carregado nesta aba. Clique no ícone do WA no menu lateral do BigBox, espere a lista de conversas aparecer, depois volte aqui.',
      });
      return;
    }
    var appliedLabel = false;
    if (opts.label) {
      var res = await applyLabelFilter(opts.label);
      if (!res.ok) {
        safeInvoke('vorcaro_scrape_result', { platform: platform, rows: [], error: res.error });
        return;
      }
      appliedLabel = true;
    }

    try {
      var pane = document.querySelector('#pane-side') || document.querySelector('#side');
      if (!pane) {
        safeInvoke('vorcaro_scrape_result', { platform: platform, rows: [], error: 'chat pane não encontrado — abra o WhatsApp Web e faça login' });
        return;
      }

      // Pass 1: collect names + try cheap data-id phone extraction.
      var rows = [];
      var seenName = Object.create(null);
      var items = pane.querySelectorAll('div[role="listitem"], div[role="row"]');
      for (var i = 0; i < items.length; i++) {
        var item = items[i];
        var nameEl = item.querySelector('span[title][dir="auto"]')
                  || item.querySelector('span[title]');
        if (!nameEl) continue;
        var name = (nameEl.getAttribute('title') || nameEl.textContent || '').trim();
        if (!name) continue;
        if (seenName[name]) continue;
        seenName[name] = true;

        var phone = null;
        var attrEls = item.querySelectorAll('[data-id], [id]');
        for (var k = 0; k < attrEls.length && !phone; k++) {
          phone = extractPhoneFromDataId(attrEls[k].getAttribute('data-id'))
               || extractPhoneFromDataId(attrEls[k].getAttribute('id'));
        }
        if (!phone) phone = nameAsPhone(name);

        rows.push({ name: name, phone: phone });
      }

      // Phase 1 fallback: read all contacts from WA's IndexedDB store. Fast
      // (<1s) and avoids clicking through chats. If the schema isn't there,
      // we fall back to clicking.
      var idbMap = {};
      try { idbMap = await buildIdbContactMap(); } catch (_) {}
      var idbHits = 0;
      for (var j2 = 0; j2 < rows.length; j2++) {
        if (rows[j2].phone) continue;
        var p2 = lookupIdbPhone(idbMap, rows[j2].name);
        if (p2) { rows[j2].phone = p2; idbHits++; }
      }
      try {
        safeInvoke('vorcaro_debug_dom_result', {
          dump: 'IndexedDB lookup: ' + idbHits + '/' + rows.length + ' phones found.\n'
            + 'IDB map size: ' + Object.keys(idbMap).length + '\n'
            + (idbHits === 0
                ? 'Schema check: tente listar abaixo o que veio do dump da estrutura.\n'
                : '')
            + dumpFirstChatRowsStructure(),
        });
      } catch (_) {}

      // Phase 2: for chats still missing phones, fall back to the click-based
      // extraction (slower, less reliable, but works for some accounts where
      // IDB schema differs).
      var MAX_CLICK_EXTRACT = 100;
      var clicked = 0;
      for (var j = 0; j < rows.length; j++) {
        if (rows[j].phone) continue;
        if (clicked >= MAX_CLICK_EXTRACT) break;
        clicked++;
        try {
          safeInvoke('vorcaro_scrape_progress', { current: clicked, total: rows.length });
        } catch (_) {}
        try {
          var p = await extractPhoneByClickingChat(rows[j].name);
          if (p) rows[j].phone = p;
        } catch (e) {
          // skip and continue
        }
      }

      var outRows = rows.map(function(r) {
        return { name: r.name, phone: r.phone, username: null, peer_id: null };
      });

      // If extraction rate is poor, embed the (already-emitted) DOM diagnostic
      // hint into the result's error so the user sees it directly in the
      // scrape result modal instead of behind a separate dialog.
      var withPhone = outRows.filter(function(r){ return !!r.phone; }).length;
      var errMsg = null;
      if (rows.length >= 5 && withPhone < rows.length / 2) {
        errMsg = withPhone + ' de ' + rows.length + ' chats tiveram telefone extraído. '
          + 'Modal de diagnóstico abriu com o DOM do primeiro chat — cole o conteúdo para o dev ajustar os seletores.';
      }
      safeInvoke('vorcaro_scrape_result', { platform: platform, rows: outRows, error: errMsg });
    } finally {
      if (appliedLabel) try { clearLabelFilter(); } catch(_) {}
    }
  }

  // ── Phase C: sendTo ──────────────────────────────────────────
  //
  // Strategy: navigate to the WhatsApp /send deep link with phone+text query
  // params. Wait for the composer to appear (success) OR for the "invalid
  // number" modal (failure). Click send. Confirm via the outgoing tick state.
  //
  // The driver runs inside the WhatsApp Web origin, so it has full DOM access.
  // We never modify other tabs.

  // ── Campaign overlay (lock the WA tab while sending) ─────
  //
  // While the orchestrator is iterating recipients, the WA tab needs to be
  // off-limits — any user interaction (clicking a chat, typing a message)
  // collides with the driver and can cause messages to go to the wrong chat.

  function showCampaignOverlay(extra) {
    var existing = document.getElementById('__vorcaro_overlay__');
    if (existing) {
      var msg = existing.querySelector('[data-vorcaro-msg]');
      if (msg && extra) msg.textContent = extra;
      return;
    }
    var el = document.createElement('div');
    el.id = '__vorcaro_overlay__';
    el.style.cssText = [
      'position:fixed', 'top:0', 'left:0', 'right:0', 'bottom:0',
      'z-index:2147483647',
      'background:rgba(0,0,0,0.55)',
      'backdrop-filter:blur(2px)',
      '-webkit-backdrop-filter:blur(2px)',
      'display:flex', 'flex-direction:column',
      'align-items:center', 'justify-content:center',
      'color:#ffffff',
      'font-family:-apple-system,Segoe UI,Roboto,sans-serif',
      'cursor:not-allowed',
      'user-select:none',
    ].join(';');
    el.innerHTML =
      '<div style="background:rgba(20,20,30,0.95);border:2px solid #fff;' +
        'border-radius:12px;padding:28px 40px;max-width:520px;text-align:center;' +
        'box-shadow:0 8px 32px rgba(0,0,0,0.5)">' +
        '<div style="font-size:48px;line-height:1;margin-bottom:12px">📣</div>' +
        '<div style="font-size:20px;font-weight:700;margin-bottom:8px">' +
          'Vorcaro está enviando uma campanha' +
        '</div>' +
        '<div style="font-size:14px;opacity:0.85;line-height:1.4">' +
          'NÃO use esta aba do WhatsApp até a campanha terminar. ' +
          'Mensagens podem ir pro chat errado se você navegar agora.' +
        '</div>' +
        '<div data-vorcaro-msg style="font-size:13px;opacity:0.7;margin-top:14px">' +
          (extra || 'Para parar: Vorcaro → Logs → Abortar ou Pausar.') +
        '</div>' +
      '</div>';

    var swallow = function(ev) {
      ev.stopPropagation();
      ev.preventDefault();
      return false;
    };
    ['click','dblclick','mousedown','mouseup','pointerdown','pointerup',
     'keydown','keyup','keypress','wheel','touchstart','touchend',
     'contextmenu','submit','focus','focusin'].forEach(function(t) {
      el.addEventListener(t, swallow, true);
    });

    document.documentElement.appendChild(el);
  }

  function hideCampaignOverlay() {
    var el = document.getElementById('__vorcaro_overlay__');
    if (el) el.remove();
  }

  function reportSend(attemptId, status, error) {
    try {
      safeInvoke('bb_log', {
        line: '[' + new Date().toISOString() + '] >>> reportSend aid=' + attemptId
              + ' status=' + status + ' error=' + (error || ''),
      });
    } catch (_) {}
    var p;
    try {
      // Tauri's IPC layer expects camelCase keys (auto-mapped to Rust's
      // snake_case args). Using `attempt_id` here caused EVERY send to
      // fail at the IPC layer — the orchestrator never got the result
      // and timed out at 90s, even when the actual WhatsApp send went
      // through. Hence the duplicate/lost messages.
      p = safeInvoke('vorcaro_send_result', {
        attemptId: attemptId, status: status, error: error || null,
      });
    } catch (e) {
      try { safeInvoke('bb_log', { line: '!! reportSend safeInvoke threw: ' + String(e) }); } catch (_) {}
      return;
    }
    if (p && p.then) {
      p.then(
        function() { try { safeInvoke('bb_log', { line: '  >>> reportSend invoke ok' }); } catch (_) {} },
        function(err) { try { safeInvoke('bb_log', { line: '  !! reportSend invoke rejected: ' + String(err) }); } catch (_) {} }
      );
    }
  }

  function waitForOne(selectors, timeoutMs) {
    return new Promise(function(resolve) {
      var start = Date.now();
      function check() {
        for (var i = 0; i < selectors.length; i++) {
          var el = document.querySelector(selectors[i].sel);
          if (el) { return resolve({ which: selectors[i].name, el: el }); }
        }
        if (Date.now() - start > timeoutMs) return resolve(null);
        setTimeout(check, 200);
      }
      check();
    });
  }

  function nativeInputValueSetter(el) {
    // React's controlled inputs need this trick to register typed text.
    var proto = Object.getPrototypeOf(el);
    var desc = Object.getOwnPropertyDescriptor(proto, 'value');
    return desc && desc.set ? desc.set.bind(el) : null;
  }

  function typeIntoComposer(el, text) {
    // WA Web's composer is a Lexical editor. Dispatch a single beforeinput
    // event with inputType=insertText — Lexical's handler consumes it and
    // inserts the text via its own reconciler. Do NOT fall back to
    // innerHTML mutation: when Lexical also handles the event, the manual
    // mutation runs in parallel and we get DUPLICATE text in the composer.
    el.focus();
    el.dispatchEvent(new InputEvent('beforeinput', {
      bubbles: true, cancelable: true,
      inputType: 'insertText', data: String(text),
    }));
  }

  function b64ToFile(b64, name, mime) {
    // atob → Uint8Array → File. Works for any size up to memory limits.
    var binary = atob(b64);
    var len = binary.length;
    var bytes = new Uint8Array(len);
    for (var i = 0; i < len; i++) bytes[i] = binary.charCodeAt(i);
    return new File([bytes], name || 'file', { type: mime || 'application/octet-stream' });
  }

  function findAttachInput(mime) {
    // WhatsApp Web keeps hidden <input type=file> elements per attachment
    // category. We try image/video first, then any document input as fallback.
    var isMedia = /^(image|video)\//.test(mime || '');
    var preferred = isMedia
      ? ['input[type="file"][accept*="image"]', 'input[type="file"][accept*="video"]']
      : ['input[type="file"][accept*="*"]', 'input[type="file"]:not([accept*="image"]):not([accept*="video"])'];
    var fallbacks = ['input[type="file"]'];
    var selectors = preferred.concat(fallbacks);
    for (var i = 0; i < selectors.length; i++) {
      var el = document.querySelector(selectors[i]);
      if (el) return el;
    }
    return null;
  }

  async function injectAttachment(att) {
    var file = b64ToFile(att.b64, att.name, att.mime);
    var input = findAttachInput(att.mime);
    if (!input) {
      // Try opening the attach menu first; some builds materialize the input lazily.
      var paperclip = document.querySelector('[data-testid="conversation-clip"]')
                   || document.querySelector('span[data-icon="clip"]')
                   || document.querySelector('button[aria-label*="ttach" i]');
      if (paperclip) {
        paperclip.click();
        await new Promise(function(r){ setTimeout(r, 400); });
        input = findAttachInput(att.mime);
      }
    }
    if (!input) return { ok: false, error: 'input de arquivo não encontrado' };

    try {
      var dt = new DataTransfer();
      dt.items.add(file);
      input.files = dt.files;
      input.dispatchEvent(new Event('change', { bubbles: true }));
    } catch (e) {
      return { ok: false, error: 'falha ao anexar: ' + String(e) };
    }
    return { ok: true };
  }

  async function openChatViaSearch(query) {
    // Find the search input at the top of the chat list pane.
    var input = document.querySelector('input[role="textbox"]')
             || document.querySelector('[role="textbox"][aria-label*="esquisar" i]')
             || document.querySelector('[role="textbox"][aria-label*="earch" i]')
             || document.querySelector('[contenteditable="true"][role="textbox"]');
    if (!input) return false;

    // Focus + clear + type.
    try { input.focus(); } catch (_) {}
    await new Promise(function(r){ setTimeout(r, 150); });

    if (input.tagName === 'INPUT' || input.tagName === 'TEXTAREA') {
      var proto = Object.getPrototypeOf(input);
      var desc = Object.getOwnPropertyDescriptor(proto, 'value');
      var setVal = function(v) {
        if (desc && desc.set) desc.set.call(input, v);
        else input.value = v;
        input.dispatchEvent(new Event('input', { bubbles: true }));
      };
      setVal('');
      await new Promise(function(r){ setTimeout(r, 100); });
      setVal(query);
    } else {
      input.textContent = '';
      input.dispatchEvent(new InputEvent('input', { bubbles: true }));
      await new Promise(function(r){ setTimeout(r, 100); });
      input.dispatchEvent(new InputEvent('beforeinput', {
        bubbles: true, cancelable: true, inputType: 'insertText', data: query,
      }));
      if (!(input.textContent || '').includes(query)) {
        input.textContent = query;
        input.dispatchEvent(new InputEvent('input', { bubbles: true }));
      }
    }

    // Wait for search results to render.
    await new Promise(function(r){ setTimeout(r, 1200); });

    // Press Enter on the search input — WA opens the top result.
    input.dispatchEvent(new KeyboardEvent('keydown', {
      key: 'Enter', code: 'Enter', which: 13, keyCode: 13, bubbles: true, cancelable: true,
    }));
    input.dispatchEvent(new KeyboardEvent('keyup', {
      key: 'Enter', code: 'Enter', which: 13, keyCode: 13, bubbles: true, cancelable: true,
    }));

    // Wait for chat pane to load on the right.
    await new Promise(function(r){ setTimeout(r, 1200); });

    return true;
  }

  function findConvHeaderName() {
    // Find the contact name in the right-pane conversation header.
    // Skip elements whose title is a tooltip/instruction (e.g. "clique para
    // mostrar os dados do contato") — those aren't the contact's name.
    var TOOLTIP_REGEX = /(clique|click|tap)\s+para/i;
    // Skip the business-account badge that WhatsApp shows in the header for
    // WA Business contacts ("Conta comercial" / "Business account"). It is a
    // span[title] that often precedes the real name span — without this we
    // mistake the badge for the contact name and abort the send.
    var BIZ_BADGE_REGEX = /^(conta\s+(comercial|empresarial|de\s+empresa)|business\s+account|official\s+business\s+account)$/i;

    function pickName(scope) {
      var candidates = scope.querySelectorAll('span[title]');
      for (var i = 0; i < candidates.length; i++) {
        var t = (candidates[i].getAttribute('title') || candidates[i].textContent || '').trim();
        if (!t) continue;
        if (TOOLTIP_REGEX.test(t)) continue;
        if (BIZ_BADGE_REGEX.test(t)) continue; // skip WA Business account badge
        if (/^[\s\d:•]+$/.test(t)) continue; // skip timestamps / counters
        return t;
      }
      return '';
    }

    var headers = document.querySelectorAll('header');
    var pane = document.querySelector('#pane-side, #side');
    for (var i = 0; i < headers.length; i++) {
      if (pane && pane.contains(headers[i])) continue;
      var name = pickName(headers[i]);
      if (name) return name;
    }
    for (var j = headers.length - 1; j >= 0; j--) {
      var name2 = pickName(headers[j]);
      if (name2) return name2;
    }
    return '';
  }

  function debugLog(line) {
    try { safeInvoke('bb_log', { line: '[' + new Date().toISOString() + '] ' + line }); } catch (_) {}
  }

  async function sendTo(phone, text, attemptId, attachments, expectedName) {
    attachments = attachments || [];
    debugLog('sendTo enter phone=' + phone + ' expected=' + expectedName + ' aid=' + attemptId);
    try {
      var digits = String(phone).replace(/[^0-9]/g, '');
      if (!digits || digits.length < 8) {
        debugLog('  invalid phone, returning');
        return reportSend(attemptId, 'invalid_number', 'phone too short');
      }
      var hasAttachments = attachments.length > 0;
      // When attaching, we don't pre-fill the URL text — WhatsApp opens a
      // separate caption field on the media preview screen. Pre-filling would
      // leave stray text in the main composer.
      var urlText = hasAttachments ? '' : (text || '');
      var url = 'https://web.whatsapp.com/send?phone=' + digits
              + '&text=' + encodeURIComponent(urlText)
              + '&type=phone_number&app_absent=0';

      // Navigate via WA's own search bar instead of URL deep-link. The
      // deep-link only works on initial page load — replaceState/popstate
      // changes URL but doesn't trigger the WA router to open the chat.
      debugLog('  navigating via search for "' + digits + '"');
      var navOk = await openChatViaSearch(digits);
      debugLog('  search nav result=' + navOk + ' href=' + location.href);
      if (!navOk) {
        return reportSend(attemptId, 'failed', 'busca interna do WA não encontrada — UI mudou');
      }

      var COMPOSER_SELECTORS = [
        { name: 'composer', sel: 'footer [contenteditable][role="textbox"]' },
        { name: 'composer', sel: 'div[contenteditable="true"][data-tab="10"]' },
        { name: 'composer', sel: 'div[contenteditable="true"][data-lexical-editor="true"]' },
        // Failure modals (Brazilian-Portuguese + English wording variants).
        { name: 'invalid',  sel: '[data-testid="popup-controls-ok"]' },
        { name: 'invalid',  sel: 'div[role="dialog"] button[role="button"]' },
      ];

      var found = await waitForOne(COMPOSER_SELECTORS, 18000);
      debugLog('  waitForOne result: ' + (found ? found.which : 'null'));
      if (!found) {
        return reportSend(attemptId, 'failed', 'composer não apareceu em 18s');
      }
      if (found.which === 'invalid') {
        try { found.el.click(); } catch(_){}
        return reportSend(attemptId, 'invalid_number', 'WhatsApp rejeitou o número');
      }

      // (No URL guard anymore — we use search navigation, not deep-link.)

      await new Promise(function(r){ setTimeout(r, 600); });

      if (expectedName) {
        var headerName = findConvHeaderName();
        debugLog('  header check: headerName="' + headerName + '" expected="' + expectedName + '"');
        var nameMatch = headerName && headerName.toLowerCase() === String(expectedName).toLowerCase();
        var phoneMatch = headerName && headerName.replace(/[^0-9]/g, '').indexOf(digits) >= 0;
        if (headerName && !nameMatch && !phoneMatch) {
          return reportSend(attemptId, 'failed',
            'chat aberto é "' + headerName + '" mas esperava "' + expectedName + '" / ' + digits + ' — abortando antes de mandar errado');
        }
      }

      if (hasAttachments) {
        // Inject the first attachment. Phase E.2 supports one attachment per
        // campaign — multi-attachment per send needs separate DOM handling
        // (an "add more" button inside the preview screen). Tracking as E.3.
        var res = await injectAttachment(attachments[0]);
        if (!res.ok) {
          return reportSend(attemptId, 'failed', res.error);
        }

        // Wait for the media preview screen — it has its own caption composer.
        var previewFound = await waitForOne([
          { name: 'caption', sel: 'div[contenteditable="true"][data-tab="undefined"], div[contenteditable="true"][role="textbox"]' },
          { name: 'caption', sel: '[contenteditable="true"][aria-label*="aption" i]' },
          { name: 'caption', sel: '[contenteditable="true"][aria-label*="egenda" i]' },
        ], 15000);
        if (!previewFound) {
          return reportSend(attemptId, 'failed', 'preview de mídia não apareceu em 15s');
        }

        // Type the caption (uses the main body text).
        if (text && previewFound.el) {
          typeIntoComposer(previewFound.el, text);
          await new Promise(function(r){ setTimeout(r, 350); });
        }

        // Preview-screen send button is usually distinct.
        var previewSend = document.querySelector('[data-testid="send"]')
                       || document.querySelector('span[data-icon="send"]')
                       || document.querySelector('button[aria-label="Send"]')
                       || document.querySelector('button[aria-label="Enviar"]');
        if (!previewSend) {
          return reportSend(attemptId, 'failed', 'botão de envio do preview não encontrado');
        }
        previewSend.click();
      } else {
        // Text-only path: now that we navigate via search bar (not URL
        // deep-link with ?text=), the composer is EMPTY when we arrive.
        // We type the text via Lexical-compatible beforeinput — there's
        // no URL pre-fill to race against, so no duplication.
        var composer = document.querySelector('footer [contenteditable][role="textbox"]')
                    || document.querySelector('div[contenteditable="true"][data-tab="10"]')
                    || document.querySelector('div[contenteditable="true"][data-lexical-editor="true"]');
        if (!composer) {
          return reportSend(attemptId, 'failed', 'composer não encontrado depois de navegar');
        }

        var expected = String(text || '').replace(/\s+/g, ' ').trim();
        debugLog('  typing into composer: "' + expected.slice(0,60) + '"');
        typeIntoComposer(composer, text);
        await new Promise(function(r){ setTimeout(r, 500); });

        var finalText = (composer.textContent || '').replace(/\s+/g, ' ').trim();
        debugLog('  composer after type: "' + finalText.slice(0,60) + '"');
        if (expected.length > 0 && finalText.indexOf(expected) < 0) {
          return reportSend(attemptId, 'failed',
            'composer não aceitou o texto. esperado="' + expected.slice(0,40) +
            '" obtido="' + finalText.slice(0,40) + '"');
        }
        if (expected.length > 0 && finalText.length > expected.length * 1.8) {
          return reportSend(attemptId, 'failed',
            'composer com texto duplicado/inchado — abortando antes de mandar');
        }

        var sendBtn = document.querySelector('button[aria-label="Send"]')
                   || document.querySelector('button[aria-label="Enviar"]')
                   || document.querySelector('span[data-icon="send"]')
                   || document.querySelector('[data-testid="compose-btn-send"]');
        if (!sendBtn) {
          return reportSend(attemptId, 'failed', 'botão de envio não encontrado');
        }
        debugLog('  clicking send button');
        sendBtn.click();
      }

      debugLog('  waiting for outgoing tick');
      var tickStart = Date.now();
      while (Date.now() - tickStart < 15000) {
        var tick = document.querySelector('span[data-icon="msg-time"], span[data-icon="msg-check"], span[data-icon="msg-dblcheck"]');
        if (tick) {
          debugLog('  tick detected, returning sent');
          return reportSend(attemptId, 'sent', null);
        }
        await new Promise(function(r){ setTimeout(r, 300); });
      }
      debugLog('  tick timeout (15s), returning sent-without-confirm');
      // Couldn't confirm — but the click happened. Treat as sent-but-unconfirmed.
      return reportSend(attemptId, 'sent', 'envio sem confirmação de tick');
    } catch (e) {
      return reportSend(attemptId, 'failed', String(e));
    }
  }

  window.__vorcaro = {
    version: 'phase-g-27',
    scrapeChats: scrapeChats,
    listLabels: listLabels,
    debugChatPane: debugChatPane,
    sendTo: sendTo,
    showCampaignOverlay: showCampaignOverlay,
    hideCampaignOverlay: hideCampaignOverlay,
  };
})();
