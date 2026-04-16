<div align="center">

# 📦 BigBox

### All your messaging apps in one blazing-fast window.

**WhatsApp · Telegram · Gmail · Slack · Discord · and more**

[![License: GPL-3.0](https://img.shields.io/badge/License-GPL--3.0-blue.svg)](LICENSE)
[![Built with Tauri](https://img.shields.io/badge/Built%20with-Tauri%20v2-orange)](https://tauri.app)
[![Rust](https://img.shields.io/badge/Backend-Rust-red)](https://www.rust-lang.org)

*A lightweight, native alternative to Rambox, Franz, and Ferdi — built with Rust for maximum performance.*

</div>

---

## Why BigBox?

| | Rambox / Franz | BigBox |
|---|---|---|
| **RAM usage** | 800MB+ (Electron) | ~150MB (native WebKit) |
| **Startup time** | 5-10s | <2s |
| **Binary size** | 200MB+ | ~15MB |
| **Framework** | Electron (Chromium) | Tauri v2 (Rust + native WebView) |
| **License** | Freemium / Proprietary | GPL-3.0 (fully open source) |

BigBox uses your system's native WebView engine instead of bundling an entire Chromium browser. The result: **5× less RAM, instant startup, and a tiny footprint.**

---

## Features

- 🚀 **Instant launch** — Native Rust backend, no Electron overhead
- 🔒 **Isolated sessions** — Each service has its own cookies, storage, and login
- 🔔 **Unread badges** — Real-time notification counters on sidebar icons
- 🔇 **Global mute** — Silence all services with one click
- 🖱️ **Drag & drop reorder** — Arrange your services the way you want
- ➕ **Add/remove services** — Customize your workspace at runtime
- 📋 **Native context menu** — Right-click to reload, mark as read, or remove
- ⚡ **Smart preload** — Background warming of services for instant switching
- 🔔 **Auto-granted notifications** — No repeated permission prompts
- 🐧 **Wayland-native** — First-class support for modern Linux desktops

---

## Supported Services

| Service | Notifications | Notes |
|---------|:---:|-------|
| **WhatsApp** | ✅ | Full Web experience |
| **Telegram** | ✅ | Web K client |
| **Gmail** | ✅ | Full Gmail interface |
| **Slack** | ✅ | Workspace app |
| **Discord** | ✅ | Full Discord client |
| **Google Calendar** | — | |
| **Google Drive** | — | |
| **Notion** | — | |
| **GitHub** | — | |
| **YouTube** | — | |
| **Spotify** | — | Web player |
| **Trello** | — | |

> Any web service can be added — BigBox is not limited to this list.

---

## Installation

### Linux (Ubuntu/Debian)

Download the `.deb` package from [Releases](https://github.com/podheitor/BigBox-Tauri/releases):

```bash
sudo dpkg -i bigbox_0.1.0_amd64.deb
```

**Build from source:**

```bash
# Install dependencies
sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev

# Install Tauri CLI
cargo install tauri-cli --version "^2"

# Build
cargo tauri build
```

### Windows

Download the `.msi` installer from [Releases](https://github.com/podheitor/BigBox-Tauri/releases) and run it.

### Build from source (any platform)

```bash
git clone https://github.com/podheitor/BigBox-Tauri.git
cd BigBox-Tauri
cargo install tauri-cli --version "^2"
cargo tauri build
```

---

## Architecture

```
BigBox-Tauri/
├── src-tauri/           # Rust backend (Tauri v2)
│   ├── src/
│   │   ├── lib.rs       # App setup, GTK layout, window events
│   │   ├── commands.rs  # IPC commands, WebView management
│   │   ├── config.rs    # Persistent TOML configuration
│   │   └── services.rs  # Service catalog (compiled-in JSON)
│   └── tauri.conf.json
├── frontend/            # Shell UI (vanilla HTML/CSS/JS)
│   ├── index.html
│   ├── style.css
│   └── app.js
└── data/
    └── services.json    # Built-in service definitions
```

**Tech stack:**

| Layer | Technology |
|-------|-----------|
| Backend | Rust + Tauri v2 |
| Frontend | Vanilla HTML/CSS/JS (zero dependencies) |
| Linux WebView | WebKit2GTK 4.1 |
| Windows WebView | Edge WebView2 |
| macOS WebView | WKWebView |

---

## Roadmap

### v0.2 — Smart Features
- 🤖 AI-powered auto-chat (LLM integration for auto-replies)
- 📢 Mass messaging / broadcast campaigns
- 🔊 Per-service notification sounds
- ⌨️ Keyboard shortcuts

### v0.3 — Platform Expansion
- 🍎 macOS build
- 📦 Flatpak / Snap packages
- 🔲 System tray with unread count

See [PLAN.md](PLAN.md) for the full development plan.

---

## Support the Project

BigBox is **free and open source**. If you find it useful, consider supporting development:

### 🎬 [PodHeitor on YouTube](https://www.youtube.com/@PodHeitor)

Subscribe and join to support free/open-source projects:

**👉 [youtube.com/@PodHeitor/join](https://www.youtube.com/@PodHeitor/join)**

---

## Contributing

Contributions are welcome! Please open an issue or pull request.

1. Fork the repo
2. Create a feature branch (`git checkout -b feature/amazing-feature`)
3. Commit your changes (`git commit -m 'Add amazing feature'`)
4. Push to the branch (`git push origin feature/amazing-feature`)
5. Open a Pull Request

---

## License

**GPL-3.0-or-later** — © 2025 Heitor Faria

Free software. Use it, modify it, distribute it.
