
(function(){
  if (window.__vorcaro && window.__vorcaro.version) return;

  function safeInvoke(cmd, payload) {
    try { return window.__TAURI__.core.invoke(cmd, payload); }
    catch (_) {
      try { return window.__TAURI_INTERNALS__.invoke(cmd, payload); }
      catch (__) { return Promise.reject('no tauri bridge'); }
    }
  }

  function reportSend(attemptId, status, error) {
    // Tauri IPC expects camelCase keys.
    safeInvoke('vorcaro_send_result', {
      attemptId: attemptId,
      status: status,
      error: error || null,
    });
  }

  function showCampaignOverlay() {
    if (document.getElementById('__vorcaro_overlay__')) return;
    var el = document.createElement('div');
    el.id = '__vorcaro_overlay__';
    el.style.cssText = 'position:fixed;top:0;left:0;right:0;bottom:0;z-index:2147483647;' +
      'background:rgba(0,0,0,0.55);backdrop-filter:blur(2px);-webkit-backdrop-filter:blur(2px);' +
      'display:flex;align-items:center;justify-content:center;color:#fff;' +
      'font-family:-apple-system,Segoe UI,sans-serif;cursor:not-allowed;user-select:none;';
    el.innerHTML = '<div style="background:rgba(20,20,30,0.95);border:2px solid #fff;border-radius:12px;' +
      'padding:28px 40px;max-width:520px;text-align:center"><div style="font-size:48px">📣</div>' +
      '<div style="font-size:20px;font-weight:700;margin:8px 0">Vorcaro está enviando uma campanha</div>' +
      '<div style="font-size:14px;opacity:0.85">NÃO use esta aba. ' +
      'Para parar: Vorcaro → Logs → Abortar ou Pausar.</div></div>';
    var swallow = function(ev) { ev.stopPropagation(); ev.preventDefault(); return false; };
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

  function scrapeChats(platform) {
    var rows = [];
    try {
      var items = document.querySelectorAll(
        '.chatlist .chatlist-chat, .chat-list .chatlist-chat, .chatlist-container .chatlist-chat'
      );
      var seen = Object.create(null);

      items.forEach(function(item) {
        var titleEl = item.querySelector('.user-title .peer-title')
                   || item.querySelector('.peer-title')
                   || item.querySelector('.user-title');
        var name = titleEl ? (titleEl.textContent || '').trim() : '';
        if (!name) return;

        var peerId = item.getAttribute('data-peer-id') || null;

        var key = (peerId || '') + '|' + name;
        if (seen[key]) return;
        seen[key] = true;

        rows.push({ name: name, phone: null, username: null, peer_id: peerId });
      });

      if (rows.length === 0) {
        safeInvoke('vorcaro_scrape_result', { platform: platform, rows: [], error: 'chat list not found — abra o Telegram Web e faça login' });
        return;
      }
    } catch (e) {
      safeInvoke('vorcaro_scrape_result', { platform: platform, rows: [], error: String(e) });
      return;
    }
    safeInvoke('vorcaro_scrape_result', { platform: platform, rows: rows, error: null });
  }

  // ── Phase D: sendTo ──────────────────────────────────────────
  //
  // Strategy:
  //   1. Resolve the recipient by setting `location.hash = '#@username'` —
  //      Telegram Web K is an SPA that picks this up and opens the chat.
  //   2. Wait for the message composer (`.input-message-input`) to appear.
  //   3. Set the composer's text + dispatch input event.
  //   4. Click `.btn-send`.
  //   5. Confirm via the most recent outgoing message DOM node.
  //
  // Limitations of Phase D: phone numbers (Telegram contacts without a public
  // username) are NOT supported here — opening a phone-keyed chat from the
  // Web K UI requires multi-step search/contacts interaction that's brittle.
  // For now, contacts whose `telegram` field is a phone get reported as
  // `invalid_number` with a clear message; the user should update the contact
  // to use `@username` instead.

  function typeIntoComposer(el, text) {
    el.focus();
    el.innerHTML = '';
    var lines = String(text).split('\n');
    for (var i = 0; i < lines.length; i++) {
      if (i > 0) el.appendChild(document.createElement('br'));
      el.appendChild(document.createTextNode(lines[i]));
    }
    el.dispatchEvent(new InputEvent('input', { bubbles: true, inputType: 'insertText', data: text }));
  }

  function b64ToFile(b64, name, mime) {
    var binary = atob(b64);
    var len = binary.length;
    var bytes = new Uint8Array(len);
    for (var i = 0; i < len; i++) bytes[i] = binary.charCodeAt(i);
    return new File([bytes], name || 'file', { type: mime || 'application/octet-stream' });
  }

  async function injectAttachmentTG(att) {
    var file = b64ToFile(att.b64, att.name, att.mime);
    // Telegram Web K hidden file inputs:
    //   - Photos/videos: `.attach-file-photo input[type=file]` or similar
    //   - Documents:     `.attach-file-document input[type=file]`
    // The DOM varies between versions; we try a few candidates.
    var candidates = [
      '.attach-file-photo input[type="file"]',
      '.attach-file-document input[type="file"]',
      'input[type="file"][accept*="image"]',
      'input[type="file"][accept*="video"]',
      'input[type="file"]',
    ];
    var input = null;
    for (var i = 0; i < candidates.length && !input; i++) {
      input = document.querySelector(candidates[i]);
    }
    if (!input) {
      // Try opening the attach menu (paperclip).
      var paperclip = document.querySelector('.attach-file')
                   || document.querySelector('button.btn-icon[aria-label*="ttach" i]')
                   || document.querySelector('button.btn-icon[aria-label*="rquivo" i]');
      if (paperclip) {
        paperclip.click();
        await new Promise(function(r){ setTimeout(r, 400); });
        for (var j = 0; j < candidates.length && !input; j++) {
          input = document.querySelector(candidates[j]);
        }
      }
    }
    if (!input) return { ok: false, error: 'input de arquivo não encontrado no Telegram' };
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

  async function sendTo(handle, text, attemptId, attachments) {
    attachments = attachments || [];
    try {
      var cleaned = String(handle || '').trim();
      if (!cleaned) {
        return reportSend(attemptId, 'invalid_number', 'identificador vazio');
      }

      // Phone-like handles aren't reachable through the hash-route trick.
      var isPhone = /^\+?\d{6,}$/.test(cleaned.replace(/\s+/g, ''));
      if (isPhone) {
        return reportSend(
          attemptId,
          'invalid_number',
          'Telegram via DOM requer @username — atualize o contato'
        );
      }

      var username = cleaned.startsWith('@') ? cleaned : ('@' + cleaned);
      var route = '#' + username;

      if (location.hash !== route) {
        location.hash = route;
      }
      // Give Telegram Web K time to resolve + render the conversation.
      await new Promise(function(r){ setTimeout(r, 800); });

      var found = await waitForOne([
        { name: 'composer', sel: '.input-message-input[contenteditable="true"]' },
        { name: 'composer', sel: '.input-message-input' },
        { name: 'notfound', sel: '.popup-error' },
        { name: 'notfound', sel: '.toast-error' },
        { name: 'notfound', sel: '.popup-not-found' },
      ], 14000);

      if (!found) {
        return reportSend(attemptId, 'failed', 'composer não apareceu em 14s — usuário inexistente ou Telegram lento');
      }
      if (found.which === 'notfound') {
        return reportSend(attemptId, 'invalid_number', 'usuário não encontrado: ' + username);
      }

      var composer = document.querySelector('.input-message-input[contenteditable="true"]')
                  || document.querySelector('.input-message-input');
      if (!composer) {
        return reportSend(attemptId, 'failed', 'composer sumiu durante o envio');
      }

      // ── Attachment branch ──────────────────────────────────
      if (attachments.length > 0) {
        var attRes = await injectAttachmentTG(attachments[0]);
        if (!attRes.ok) return reportSend(attemptId, 'failed', attRes.error);

        // Telegram pops a media preview popup with its own caption field.
        var capFound = await waitForOne([
          { name: 'caption', sel: '.popup-send-photo .input-message-input' },
          { name: 'caption', sel: '.media-viewer-caption .input-message-input' },
          { name: 'caption', sel: '.popup-send-photo [contenteditable="true"]' },
        ], 12000);
        if (capFound && text && capFound.el) {
          typeIntoComposer(capFound.el, text);
          await new Promise(function(r){ setTimeout(r, 300); });
        }

        var popupSend = document.querySelector('.popup-send-photo .btn-send:not(.is-hidden)')
                     || document.querySelector('.popup-send-photo button.btn-primary')
                     || document.querySelector('.btn-send:not(.is-hidden)');
        if (!popupSend) {
          return reportSend(attemptId, 'failed', 'botão de envio do preview não encontrado');
        }
        popupSend.click();

        // Confirm via new outgoing bubble.
        var confirmStart = Date.now();
        while (Date.now() - confirmStart < 15000) {
          var sent = document.querySelector('.bubbles-inner .bubble.is-out:last-of-type');
          if (sent) return reportSend(attemptId, 'sent', null);
          await new Promise(function(r){ setTimeout(r, 300); });
        }
        return reportSend(attemptId, 'sent', 'envio sem confirmação visual');
      }

      // ── Text-only branch ───────────────────────────────────
      typeIntoComposer(composer, text);
      await new Promise(function(r){ setTimeout(r, 350); });

      var sendBtn = document.querySelector('.btn-send:not(.is-hidden)')
                 || document.querySelector('button.btn-send')
                 || document.querySelector('button[aria-label*="Send" i]')
                 || document.querySelector('button[aria-label*="Enviar" i]');
      if (!sendBtn) {
        return reportSend(attemptId, 'failed', 'botão de envio não encontrado');
      }
      sendBtn.click();

      // Confirm by watching for a new outgoing message bubble.
      var confirmStart = Date.now();
      while (Date.now() - confirmStart < 12000) {
        var outgoing = document.querySelector(
          '.bubbles-inner .bubble.is-out:last-of-type, .Message--outgoing:last-of-type'
        );
        if (outgoing) {
          return reportSend(attemptId, 'sent', null);
        }
        await new Promise(function(r){ setTimeout(r, 300); });
      }
      return reportSend(attemptId, 'sent', 'envio sem confirmação visual');
    } catch (e) {
      return reportSend(attemptId, 'failed', String(e));
    }
  }

  window.__vorcaro = {
    version: 'phase-g-22',
    scrapeChats: scrapeChats,
    sendTo: sendTo,
    showCampaignOverlay: showCampaignOverlay,
    hideCampaignOverlay: hideCampaignOverlay,
  };
})();
