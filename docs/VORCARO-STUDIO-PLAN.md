# Vorcaro's Studio — Development Plan

Feature for sending messages to many recipients on WhatsApp (personal + Business) and Telegram, with contact lists, tags, CSV import, scraping, and conservative safety defaults.

**Status: Phases A → F shipped. Phase F.2 (Cloud API attachments) and Phase E.3 (multi-attachment per send) deferred.**
See section 14 for what landed vs. what's outstanding.

---

## 1. Decisions (locked)

| # | Question | Decision |
|---|---|---|
| 1 | Telegram delivery | **DOM injection on Telegram Web** (same approach as WhatsApp). Telegram Bot API NOT used. |
| 2 | Recipient sources | **All four:** scrape existing chats, manual entry, CSV import, saved lists + tags in BigBox config. |
| 3 | UI placement | **New sidebar entry** (13th icon), local panel hosted by BigBox (not a remote URL). |
| 4 | Safety profile | **Conservative defaults**, user can loosen. 30-90s randomized delay, daily cap 100, warn at >20 recipients, auto-pause after 3 consecutive failures. |
| 5 | WhatsApp Business | **Yes — DOM path via web.whatsapp.com**, separate sidebar slot + session dir. Same driver script as personal WA. |
| 6 | WhatsApp Cloud API | **Deferred to a later version (Phase F).** Platform enum + `Box<dyn Sender>` dispatch designed so adding `CloudApiSender` later is a drop-in. |

---

## 2. Architecture

Vorcaro's Studio is a local HTML panel hosted at a 13th sidebar slot. The Rust side owns:

- A new IPC surface for campaign CRUD, contact / list / tag CRUD, and a send-orchestration state machine.
- Two driver scripts (`VORCARO_WHATSAPP_DRIVER`, `VORCARO_TELEGRAM_DRIVER`) injected into the existing WA/WA-Business/TG WebViews on creation.
- Bidirectional bridge: studio panel → Rust orchestrator → `wv.eval()` into target WebView → driver picks recipient, fills text, clicks send → reports back via `__TAURI__.core.invoke('vorcaro_send_result', …)`.

The studio panel WebView never talks directly to the chat-service WebViews; it always goes through Rust.

---

## 3. File layout

```
src-tauri/src/
├── vorcaro/                 (new module)
│   ├── mod.rs               state, IPC handler registration
│   ├── model.rs             Contact, ContactList, Tag, Campaign, Settings structs
│   ├── store.rs             load/save vorcaro.toml
│   ├── orchestrator.rs      send loop: queue, delays, jitter, daily cap (Phase C+)
│   ├── drivers.rs           VORCARO_WHATSAPP_DRIVER + VORCARO_TELEGRAM_DRIVER JS (Phase C+)
│   └── csv.rs               CSV import/parsing
├── commands.rs              register new commands; load drivers into WA/TG WebViews
└── lib.rs                   mount module; add vorcaro special-case to open_service

frontend/
├── vorcaro/                 (new local panel)
│   ├── index.html           4 tabs: Contacts · Lists · Campaign · Logs
│   ├── studio.js
│   └── studio.css
└── app.js                   route id == "vorcaro" → load local panel, not URL
```

---

## 4. Data model (persisted to `~/.config/bigbox/vorcaro.toml`)

```rust
struct Contact {
    id: Uuid,
    display_name: String,
    whatsapp: Option<String>,            // E.164 phone for personal WA
    whatsapp_business: Option<String>,   // usually same as whatsapp, but kept separate
    telegram: Option<String>,            // @username or phone
    tags: Vec<String>,
    source: ContactSource,               // Scraped | Manual | Imported
    notes: Option<String>,
}

struct ContactList { id: Uuid, name: String, contact_ids: Vec<Uuid> }

enum Platform {
    WhatsAppWeb,
    WhatsAppBusinessWeb,
    Telegram,
    // Future: WhatsAppCloudAPI  (Phase F)
}

struct Campaign {
    id: Uuid,
    name: String,
    body: String,
    attachments: Vec<PathBuf>,
    targets: TargetSpec,                 // List(id) | Tag(name) | AdHoc(Vec<Uuid>)
    platform: Platform,
    status: CampaignStatus,              // Draft|Running|Paused|Done|Aborted
    created_at: DateTime<Utc>,
    started_at: Option<DateTime<Utc>>,
    finished_at: Option<DateTime<Utc>>,
    progress: Vec<SendAttempt>,          // per-recipient: queued|sent|failed|skipped
}

struct Settings {
    min_delay_secs: u32,                 // default 30
    max_delay_secs: u32,                 // default 90
    daily_cap_per_platform: u32,         // default 100
    warn_threshold: u32,                 // default 20
    auto_pause_after_consecutive_failures: u32, // default 3
}
```

---

## 5. Rust IPC surface

| Command | Purpose | Phase |
|---|---|---|
| `vorcaro_get_state` | Bootstrap panel: contacts, lists, tags, settings, recent campaigns | A |
| `vorcaro_save_contact` / `_delete_contact` | Manual CRUD | A |
| `vorcaro_import_csv(path)` | Parse + dedupe by phone/username | A |
| `vorcaro_save_list` / `_delete_list` | Named lists | A |
| `vorcaro_save_tag` / `_apply_tag` / `_remove_tag` | Tags | A |
| `vorcaro_update_settings` | Safety knobs | A |
| `vorcaro_scrape_chats(platform)` | `wv.eval()` into WA/WA-Business/TG to dump chat list; returns rows for confirm + import | B |
| `vorcaro_preview_campaign(spec)` | Resolves target → final recipient set + count; ban-risk warning | C |
| `vorcaro_start_campaign(spec)` | Kicks off orchestrator task | C |
| `vorcaro_pause` / `_resume` / `_abort(campaign_id)` | Control | C |
| `vorcaro_send_result` | Driver→Rust callback per send | C |
| Event `vorcaro://progress` | Streams per-send results to panel for live log | C |

---

## 6. Sender abstraction

```rust
#[async_trait]
trait Sender {
    async fn send(&self, recipient: &Contact, body: &str) -> SendOutcome;
    async fn scrape_chats(&self) -> Result<Vec<ScrapedChat>>;
}
```

Implementations:

- `WebDriverSender { platform: WhatsAppWeb | WhatsAppBusinessWeb }` — same driver, different WebView label.
- `TelegramDriverSender`
- *(deferred)* `CloudApiSender` — Phase F, no WebView, pure `reqwest`.

Orchestrator holds a `Box<dyn Sender>` selected by `Campaign.platform`.

---

## 7. Driver scripts (Phase C+)

### `VORCARO_WHATSAPP_DRIVER` (injected into `whatsapp` AND `whatsapp-business` WebViews)

- Exposes `window.__vorcaro = { sendTo(phone, text), scrapeChats(), version }`.
- `sendTo(phone, text)`:
  1. `window.location.href = "https://web.whatsapp.com/send?phone=" + E164 + "&text=" + encodeURIComponent(text)`.
  2. MutationObserver waits ≤15 s for the composer `[contenteditable][data-tab="10"]`. If WA shows "Phone number shared via url is invalid", resolve `{status:"invalid_number"}`.
  3. Click `button[aria-label="Send"]` / `span[data-icon="send"]`.
  4. Confirm by watching the just-sent message bubble's tick state; resolve `{status:"sent"}` once double-tick or single-tick appears.
- `scrapeChats`: walks `#pane-side [role="listitem"]`, extracts display name + phone (when available via `data-id`).
- Posts results via `window.__TAURI__.core.invoke('vorcaro_send_result', {...})`.

### `VORCARO_TELEGRAM_DRIVER` (injected into `telegram` WebView)

- `sendTo(usernameOrPhone, text)`:
  1. Open search bar, type the username, click first result.
  2. Find `.input-message-input`, set innerText, dispatch `input` event.
  3. Click `.btn-send`.
  4. Confirm by watching for the outgoing message DOM node.
- `scrapeChats`: walks `.chatlist .chatlist-chat`.

Both load alongside existing scripts in `commands.rs` only for the matching service id.

---

## 8. Orchestrator (Phase C)

`tokio::spawn`'d task per active campaign:

- Pull next contact → call `wv.eval(format!("__vorcaro.sendTo({phone}, {body})"))` on the WA/WA-Business/TG WebView.
- Wait for `vorcaro_send_result` event (timeout 60s) or "no driver" failure.
- Sleep `rand::range(min_delay..max_delay)` seconds between sends.
- Honor `daily_cap_per_platform` (counts persisted by date).
- Pause/resume/abort via channels.
- Persist progress after every attempt so a crash doesn't lose state.
- Auto-pause after `auto_pause_after_consecutive_failures` consecutive failures.

---

## 9. Frontend panel (4 tabs)

- **Contacts** — table, search/filter by name/tag, add/edit/delete, "Scrape WhatsApp" / "Scrape WhatsApp Business" / "Scrape Telegram" / "Import CSV" buttons. Tag chips inline.
- **Lists** — left: list names; right: drag-add contacts; "Use tag as list" shortcut.
- **Campaign** — message editor (text + attachments later), target picker (List | Tag | Ad-hoc multi-select), platform dropdown (WhatsApp / WhatsApp Business / Telegram), "Preview recipients" → confirms count + shows ban-risk warning if > `warn_threshold`, "Send" / "Schedule".
- **Logs** — live progress per recipient (sent ✓ / failed ✗ / skipped), pause/abort buttons, export to CSV.

---

## 10. Sidebar integration

In `data/services.json` add two built-in entries:

```json
{ "id": "whatsapp-business", "name": "WhatsApp Business",
  "url": "https://web.whatsapp.com", "color": "#128C7E",
  "user_agent_override": "Mozilla/5.0 … Chrome/125.0.0.0 …" }

{ "id": "vorcaro", "name": "Vorcaro's Studio",
  "url": "vorcaro://panel", "color": "#7c3aed", "builtin_local": true }
```

`whatsapp-business` reuses the existing service plumbing — separate session dir under `~/.local/share/bigbox/sessions/whatsapp-business/` is free thanks to id-keyed storage.

`vorcaro` is special-cased in `open_service`: load the local HTML panel (frontend/vorcaro/index.html) and do NOT apply the chat-service injection scripts or UA override.

---

## 11. Safety defaults (Phase C)

- 30-90 s randomized delay between sends.
- Daily cap: 100 sends per platform per day (persisted by date in `vorcaro.toml`).
- Confirm dialog when `recipients.len() > 20`, with explicit "this can get your WhatsApp banned" text.
- Pause campaign automatically if 3 consecutive sends fail (likely rate-limit or block).
- Settings page exposes all knobs but defaults stay conservative.

---

## 12. Phased delivery

1. **Phase A — Skeleton + contacts.** ✅ Shipped.
2. **Phase B — Scraping.** ✅ Shipped.
3. **Phase C — WhatsApp send.** ✅ Shipped.
4. **Phase D — Telegram send.** ✅ Shipped.
5. **Phase E — Polish.** ✅ Shipped (resume-aware loop, scheduling, retries, expanded substitutions, boot rehydration).
6. **Phase E.2 — Attachments (single file per campaign).** ✅ Shipped for WhatsApp Web + Telegram Web.
7. **Phase E.3 — Multiple attachments per send (deferred).**
8. **Phase F — WhatsApp Cloud API.** ✅ Shipped (text + templates; no media yet).
9. **Phase F.2 — Cloud API media (deferred).** Meta's `/media` upload flow + media-template support.

---

## 13. Open questions (non-blocking)

- **Naming + icon** — confirm "Vorcaro's Studio" is the final shipped name and choose final color/icon. Current: purple `#7c3aed` placeholder.

---

## 14. What's actually shipped

### Phase A
- Catalog entries `whatsapp_business` (separate session, same UA) + `vorcaro` (`local://vorcaro` sentinel).
- `frontend/vorcaro/` local panel — 5 tabs: Contatos, Listas, Campanha, Logs, Configurações.
- 12 IPC handlers: get_state, save/delete contact, save/delete list, save/apply/remove/rename tag, add/remove contact ↔ list, import_csv (content string), update_settings.
- Persistence at `~/.config/bigbox/vorcaro.toml`.
- `open_service` special-cases `vorcaro` → loads panel via `WebviewUrl::App`, skips chat-service injection scripts.

### Phase B
- `VORCARO_WHATSAPP_DRIVER` + `VORCARO_TELEGRAM_DRIVER` with `scrapeChats(platform)`.
- Drivers injected only into WhatsApp / WhatsApp Business / Telegram WebViews.
- IPC: `vorcaro_scrape_chats`, `vorcaro_scrape_result` (driver callback → event), `vorcaro_import_scraped` (dedupe + merge).
- Frontend: "Raspar WA / WA-B / TG" buttons + result picker modal.
- `Platform` enum variants explicitly renamed to `whatsapp_web` / `whatsapp_business_web` / `telegram` (serde's snake_case would've mangled to `whats_app_web`).

### Phase C
- WhatsApp `sendTo` driver: deep-link navigation, composer wait, invalid-number modal detection, contenteditable typing fallback, send-button click, tick-state confirmation.
- `orchestrator.rs` — one `tauri::async_runtime::spawn`'d task per running campaign, with `tokio::sync::Notify` + `AsyncMutex<CampaignStatus>` for pause/resume/abort, `tokio::sync::oneshot` for per-attempt result routing (90 s timeout), 250 ms-sliced jitter sleep, persisted `DailyCap` enforced, auto-pause after N consecutive failures.
- 6 IPC commands: preview / start / pause / resume / abort / send_result.
- Campaign tab UI: name, platform, target mode (Lista / Tag / AdHoc), body, preview → count + handle coverage + ban-risk warning, send.
- Logs tab UI: per-campaign live progress streamed via `vorcaro://campaign-progress` event, summary stats, control buttons.
- Variable substitution: `{nome}`, `{name}`.

### Phase D
- Telegram `sendTo` driver: URL-hash navigation (`#@username`), composer wait, type, send via `.btn-send`, outgoing-bubble confirmation.
- Platform dropdown unlocked.
- Phone-only Telegram handles rejected with `invalid_number` ("requer @username").

### Phase E
- `CampaignStatus::Scheduled` + `Campaign.scheduled_at: Option<DateTime<Utc>>`. Orchestrator sleeps in 30 s slices until the trigger; abort interrupts.
- `Settings.max_retries_per_recipient` (default 0).
- **Resume-aware loop**: skips contacts already in `Sent / InvalidNumber / Skipped`; respects retry budget for `Failed`. Fixes the "resume restarts from index 0" wart from Phase C.
- `rehydrate_on_boot()` — re-spawns orchestrator tasks for campaigns that were `Scheduled` or `Running` at last shutdown.
- Expanded substitutions: `{nome}`, `{name}`, `{firstname}`, `{primeironome}`, `{whatsapp}`, `{telegram}`, `{tag}`, `{notes}`.
- Frontend: `datetime-local` picker, "Agendar envio" button label, blue `scheduled` pill, retries field in Configurações.

### Phase E.2
- `attachments.rs` module: `stage(name, b64)` writes file to `~/.cache/bigbox/vorcaro/attachments/<uuid>-<safe-name>` with sanitized filename + uuid prefix dedup; `read_as_base64(path)` returns `(orig_name, mime, b64)` with a 14-extension MIME table.
- `vorcaro_stage_attachment` IPC.
- Orchestrator reads attachments once per campaign, base64-encodes, threads into `sendTo` as a 4th arg.
- WhatsApp driver: `b64ToFile` (atob → Uint8Array → File), `findAttachInput` (image/video/document accept-attr fallbacks + paperclip-open retry), `injectAttachment` (DataTransfer + dispatch `change`). With-attachment branch skips URL `text=` pre-fill, waits for preview screen, types body as caption, clicks preview's send button.
- Telegram driver: parallel `injectAttachmentTG`, `.popup-send-photo` preview handling with separate caption field.
- Frontend: file picker on Campaign form, attachment chip with name + size + remove button, 64 MB hard cap, reset after successful start.
- Driver versions bumped to `phase-e2-1`.

### Phase F (WhatsApp Cloud API)
- `Platform::WhatsAppCloudApi` variant (serde `whatsapp_cloud_api`).
- `cloud_api.rs` module: `WhatsAppCloudConfig` persisted to `~/.config/bigbox/vorcaro_secrets.toml` with chmod 0600 on Unix. `verify_connection`, `list_templates` (paginated, counts `{{N}}` body placeholders), `send_text`, `send_template` (Graph v17.0 by default; configurable).
- Orchestrator branches on platform: Cloud API takes a no-WebView async-HTTP path; per-recipient variable substitution applied to both body and each template param before send. Recipient handle pulled from `contact.whatsapp` (or `whatsapp_business` fallback).
- Error code mapping: Meta `131026 / 131047 / 131051` → `SendStatus::InvalidNumber` (recipient unreachable / outside-24h-window / not on WhatsApp); other 4xx/5xx → `Failed` with `Meta <code>: <message>` error text.
- `TemplateUsage { name, language, body_params }` on `Campaign`.
- 4 IPC commands: `vorcaro_get_cloud_config` (redacts token to `…XXXX`), `vorcaro_save_cloud_config` (preserves stored token when frontend sends a redacted one), `vorcaro_verify_cloud_connection`, `vorcaro_list_cloud_templates`.
- Frontend Settings: separate "WhatsApp Cloud API" section with token (password input), phone number ID, WABA ID, API version, Save + Test connection buttons.
- Frontend Campaign: 4th option in platform dropdown; Cloud-API-only block with "Tipo de mensagem" toggle (Template / Free-form), template picker (only APPROVED templates), inline body-text preview, per-param input fields auto-defaulted to `{firstname}`, `{nome}`, `{whatsapp}`, `{tag}`. Attachment row hidden for Cloud API (Phase F.2).

---

## 15. Known limitations to validate against real DOM

- **WhatsApp composer selectors** — three fallbacks (`footer [contenteditable][role="textbox"]`, `[data-tab="10"]`, lexical-editor) are speculative; will need to iterate after first live test.
- **WhatsApp tick confirmation** — if the tick takes > 15 s the driver reports "sent without tick" rather than failing. The send did happen.
- **Telegram phone-keyed chats** — not supported; users with only a phone in `contact.telegram` get `invalid_number`. Use `@username` instead.
- **Single WebView per service** — during a campaign the chat-service WebView's URL is hijacked (`/send?phone=…` or `#@username`); the user can't interact with that tab until the run finishes. Fine for batch jobs; worth flagging.
- **WhatsApp Business via DOM = same ban risk as personal.** The "Business" label is just a separate session — Meta's anti-spam rules apply identically. The compliant high-volume path is Phase F (Cloud API).

---

## 16. What's deferred + design sketches

### Phase E.3 — Multiple attachments per send
Currently one file per campaign. WhatsApp's media-preview screen has an "add more" affordance; Telegram has an album mode. Both need a different DOM flow than the first injection. The orchestrator passes the full `attachments[]` array to the driver already — only the driver needs work.

### Phase F.2 — Cloud API media attachments
Meta requires a two-step flow:
1. `POST /{phone_number_id}/media` with the file → returns `media_id`.
2. Use `media_id` in the message payload (`image: { id }` / `video: { id }` / etc.).
Plus media-templates: templates with a header image/video use `components: [{ type: "header", parameters: [{ type: "image", image: { id } }] }]`. Doable; small follow-up to `cloud_api.rs`.

### Phase F additions to ship later if useful
- Webhook subscription so we can receive delivery/read status callbacks instead of relying on the `messages[0].id` response.
- Per-platform delay overrides (Cloud API can safely fire at much higher rates than DOM-driven sends).
- Token rotation reminders (Meta long-lived tokens expire after 60 days unless converted to permanent).
