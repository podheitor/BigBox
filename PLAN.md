# BigBox — Development Plan

## Status: ✅ MVP Complete

### Phase 1: Core — ✅ DONE
- [x] Tauri v2 project scaffold (Rust backend + vanilla JS frontend)
- [x] Service catalog system (JSON-based, compiled into binary)
- [x] Add/remove services at runtime with persistent TOML config
- [x] Isolated WebView per service (separate session directories)
- [x] Custom titlebar (minimize, maximize, close)
- [x] Sidebar with avatar icons and service navigation
- [x] Drag-and-drop reorder (Pointer Events — HTML5 DnD fails on Wayland)

### Phase 2: UX Polish — ✅ DONE
- [x] Unread badge counters (MutationObserver + title parsing)
- [x] Global mute toggle (mutes all service WebViews)
- [x] Native right-click context menu (reload, mark as read, remove)
- [x] Welcome screen when no service is selected
- [x] About dialog

### Phase 3: Performance — ✅ DONE
- [x] Session persistence across restarts (data_directory per service)
- [x] Sequential background preload (after first service opens, warm remaining)
- [x] Adaptive preload delays (10s initial, 2s between services)
- [x] Last-active service memory (localStorage)
- [x] Open-time telemetry for future preload prioritization

### Phase 4: Platform Integration — ✅ DONE
- [x] GTK layout with overlay positioning (sidebar always visible)
- [x] Proper size_allocate override for Wayland compatibility
- [x] Window resize handling (reposition all WebViews)
- [x] Auto-grant notification/media permissions (WebKitGTK signal)
- [x] Notification.permission override (JS injection for WhatsApp/Telegram)

### Phase 5: Distribution — ✅ DONE
- [x] .deb package for Ubuntu/Debian
- [x] Windows installer (.msi)
- [x] Public GitHub release

---

## Roadmap

### v0.2 — Smart Features
- [ ] AI-powered auto-chat (LLM integration for auto-replies)
- [ ] Mass messaging / broadcast campaigns
- [ ] Per-service notification sounds
- [ ] Keyboard shortcuts for service switching

### v0.3 — Platform Expansion
- [ ] macOS build (.dmg)
- [ ] Flatpak / Snap packages
- [ ] System tray with unread count
- [ ] Auto-start on login

### v0.4 — Power User
- [ ] Custom CSS per service (dark mode overrides)
- [ ] Multi-account support (multiple WhatsApp instances)
- [ ] Service group tabs
- [ ] Import/export settings

---

## Supported Services

| Service | Status | Badge | Notes |
|---------|--------|-------|-------|
| WhatsApp | ✅ | ✅ | Chrome UA required |
| Telegram | ✅ | ✅ | |
| Gmail | ✅ | ✅ | |
| Google Calendar | ✅ | — | |
| Google Drive | ✅ | — | |
| Slack | ✅ | ✅ | Chrome UA required |
| Discord | ✅ | ✅ | Chrome UA required |
| Notion | ✅ | — | |
| GitHub | ✅ | — | |
| YouTube | ✅ | — | |
| Spotify | ✅ | — | |
| Trello | ✅ | — | |
