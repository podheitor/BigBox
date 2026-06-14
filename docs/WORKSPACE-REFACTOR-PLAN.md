# BigBox — Cargo Workspace Refactor Plan

> **Priority for the next session.** This is the architecture refactor that turns
> the single `src-tauri` crate into a real multi-crate Cargo workspace with true
> compilation isolation, parallel builds, and clean inward dependencies.
>
> This document is the spec. It is opinionated on purpose. Where a decision has a
> trade-off, the trade-off is stated and **one** option is chosen. Do not
> re-litigate the chosen options without a concrete measured reason.

> **DECISION (2026-06-14): the app crate stays at `src-tauri/`.** The original
> §1 topology relocated the binary into `crates/bigbox-app/` (moving frontend,
> icons, `tauri.conf.json`). That silently breaks `cargo tauri build` config
> auto-discovery and every artifact path baked into `release.yml` (×3 jobs),
> `PKGBUILD`, and the Flathub manifest. The build-parallelism/isolation win comes
> entirely from splitting into multiple workspace crates — it is **independent of
> where the binary crate physically lives**. So: the Tauri binary crate remains
> `src-tauri/` (package `bigbox`, lib `bigbox_lib`, binary `bigbox`, owning
> `tauri.conf.json` / `frontend` ref / `build.rs` / `icons` / `gen` /
> `capabilities`), and the other nine crates are extracted into `crates/`. The
> workspace root `Cargo.toml` lists `members = ["src-tauri", "crates/*"]`. To keep
> every existing artifact path valid, a root `.cargo/config.toml` pins
> `target-dir = "src-tauri/target"` (a workspace would otherwise move it to the
> repo-root `target/`). Net effect: **zero changes to CI/PKGBUILD/flatpak**.
> Wherever this doc says `bigbox-app`/`crates/bigbox-app`, read it as "the
> `src-tauri` crate" unless noted.

---

## ✅ Execution status (2026-06-14)

The refactor is **substantially complete and builds green** (`cargo build
--workspace`, 0 warnings). The single `src-tauri` crate is now a **9-crate
workspace**:

```
crates/ bigbox-core  bigbox-contract  bigbox-driver-assets  bigbox-config
        bigbox-cloud  bigbox-orchestrator  bigbox-shell  bigbox-vorcaro
src-tauri/  (package `bigbox`, lib `bigbox_lib`) = the app (Builder + setup)
```

Done: Steps 1–6 + 8 from §8. The hard inversion (§4) is done — the engine is
Tauri-free, talking out through `DriverTransport`/`ProgressSink` ports. The CI
guard (`scripts/check-no-tauri.sh` + `.github/workflows/arch-guard.yml`) and the
`BB_DRIVER_DIR` dev path are in place. **Verified:** all six inner crates are
tauri-free via `cargo tree`.

**Deviations from the spec as written (all deliberate):**
1. **App stays at `src-tauri/`** (not `crates/bigbox-app/`) — see the DECISION
   note above. Root `.cargo/config.toml` pins `target-dir = "src-tauri/target"`
   so CI/PKGBUILD/flatpak are untouched. Package stays `bigbox`, lib `bigbox_lib`.
2. **`bigbox-contract` deps = `bigbox-core` + `uuid` + `serde_json`** (not "core
   only"): the port signatures use `Uuid` and `serde_json::Value`. Still no
   tauri/tokio/reqwest.
3. **Tauri commands live in a sub-module**, not each edge crate's root
   (`bigbox-shell` → `mod commands`, `bigbox-vorcaro` → `mod ipc`). `#[tauri::command]`
   generates a `__cmd__*` macro that collides with its own re-export at a
   library crate root.
4. **The GTK overlay layout lives in `bigbox-shell`**, not the app (§5 sketched
   it in the app). The host IPC commands call `collapse_shell_impl`/`expand_shell_impl`,
   so the layout must sit beside them — otherwise the app↔shell dep would point
   the wrong way. The app just calls `bigbox_shell::setup_gtk_layout(app)`.
5. **`bigbox-driver-assets` keeps the `VORCARO_*` consts AND adds `whatsapp()` /
   `telegram()`** runtime-load fns; `bigbox-shell` calls the fns (the `BB_DRIVER_DIR`
   dev path).
6. **`cairo` dropped** (was an unused dependency).
7. **`SendOutcome` moved to `bigbox-core`** (shared by orchestrator + cloud) so
   neither depends on the other.

**Deferred (per §8 step 7):** the 400-method `bigbox-drivers` crate + the typed
platform-driver trait in `bigbox-contract`. Drivers are JS assets today; the
typed Rust contract lands when that layer actually does. That is why the
workspace currently has 9 crates, not 10 — add `bigbox-drivers` (and its line in
`scripts/check-no-tauri.sh`) when it lands.

**Remaining:** validate on BigLinux with the deploy recipe (kill + relaunch
after install). Local sandbox builds need `RUSTC_WRAPPER="" RUSTFLAGS=""` to
bypass a missing mold/sccache in this environment — not a code issue.

---

## 0. Current state (what we are refactoring)

One crate: `src-tauri/` → package `bigbox`, lib `bigbox_lib` (`staticlib`,
`cdylib`, `rlib`) + a thin bin.

```
src-tauri/src/
  main.rs              29 LOC   env vars + bigbox_lib::run()
  lib.rs              249 LOC   tauri::Builder wiring, GTK layout, UI constants
  commands.rs        1534 LOC   53 Tauri IPC commands: webview host + config CRUD
  config.rs            57 LOC   AppConfig/UserService + TOML load/save        (no tauri)
  services.rs          38 LOC   ServiceDef + embedded catalog + session_dir   (no tauri)
  vorcaro/
    mod.rs            848 LOC   Tauri IPC for CRM + VorcaroStore state + glue
    model.rs          265 LOC   pure serde domain types                       (no tauri)
    store.rs           33 LOC   vorcaro.toml persistence
    csv_io.rs         144 LOC   CSV import
    attachments.rs    108 LOC   attachment staging
    cloud_api.rs      410 LOC   WhatsApp Cloud API client (reqwest)
    orchestrator.rs   630 LOC   campaign engine (tokio) — TAURI-COUPLED
    drivers.rs       1913 LOC   two `pub const &str` JS driver blobs (assets, not code)
```

### The two problems this refactor fixes

1. **Everything rebuilds on every edit.** A one-character change in `drivers.rs`
   (the JS blobs we iterated on ~27 times — see `start_here.md`) recompiles the
   *entire* crate, including all of Tauri/wry/webkit glue. The crate is the unit
   of incremental invalidation, and today there is one crate.
2. **No build parallelism.** Cargo parallelizes across crates in the dependency
   DAG. With one crate there is no DAG to parallelize. The heavy Tauri
   compilation is serialized in front of every change to pure engine logic.

### The coupling that has to be broken

`orchestrator.rs` — the campaign engine, the part we edit most — directly imports
`tauri::{AppHandle, Emitter, Manager}`, holds `tauri::State`, calls
`wv.eval(...)`, and spawns on `tauri::async_runtime`. So the engine cannot move
out of the Tauri blast radius **until that dependency is inverted** (Section 4).
This is the single hardest task in the refactor and the one that unlocks the rest.

---

## 1. Recommended crate topology

Ten crates. Domains map 1:1 to crates. Dependencies point **inward** toward
`bigbox-core` / `bigbox-contract`. No crate depends sideways.

```
BigBox-Tauri/
├── Cargo.toml                      # [workspace] root: members, shared deps, patch, profiles
├── crates/
│   ├── bigbox-core/                # LEAF. Pure domain vocabulary: types, IDs, errors, UI consts.
│   │   └── src/lib.rs              #   serde/chrono/uuid only. NO tauri, NO tokio, NO reqwest.
│   │
│   ├── bigbox-contract/            # The indivisible ~400-method semantic contract (a trait).
│   │   └── src/lib.rs              #   + port traits for the engine. deps: bigbox-core ONLY.
│   │
│   ├── bigbox-driver-assets/       # LEAF. The JS driver blobs (include_str! of .js files).
│   │   ├── assets/whatsapp.js      #   deps: none. Trivial to compile.
│   │   ├── assets/telegram.js
│   │   └── src/lib.rs
│   │
│   ├── bigbox-config/              # All on-disk persistence + embedded assets.
│   │   ├── data/services.json      #   config.toml, services catalog, vorcaro.toml, session dirs.
│   │   └── src/lib.rs              #   deps: bigbox-core, toml, serde_json, dirs.
│   │
│   ├── bigbox-cloud/               # WhatsApp Cloud API HTTP client.
│   │   └── src/lib.rs              #   deps: bigbox-core, reqwest, serde_json. NO tauri.
│   │
│   ├── bigbox-drivers/             # Implementations of bigbox-contract per platform.
│   │   └── src/lib.rs              #   deps: bigbox-contract, bigbox-core, bigbox-driver-assets.
│   │
│   ├── bigbox-orchestrator/        # Campaign engine. The hot-edit crate.
│   │   └── src/lib.rs              #   deps: bigbox-contract, bigbox-core, bigbox-config,
│   │                               #         bigbox-cloud, tokio, csv, base64, rand, chrono.
│   │                               #   *** NO tauri *** (talks out via port traits).
│   │
│   ├── bigbox-shell/               # Tauri IPC: webview host (the 18 commands::* fns).
│   │   └── src/lib.rs              #   deps: tauri, bigbox-core, bigbox-config, bigbox-driver-assets.
│   │
│   ├── bigbox-vorcaro/             # Tauri IPC: CRM/campaigns (the vorcaro_* fns) + adapters.
│   │   └── src/lib.rs              #   deps: tauri, bigbox-core, bigbox-config,
│   │                               #         bigbox-orchestrator, bigbox-cloud, bigbox-drivers.
│   │
│   └── bigbox-app/                 # The binary. tauri::Builder wiring + window/GTK layout.
│       ├── build.rs                #   deps: tauri, tauri-build, bigbox-shell, bigbox-vorcaro,
│       ├── tauri.conf.json         #         + platform deps (gtk/cairo/webkit2gtk/zbus on linux).
│       ├── icons/  frontend/       #   Owns tauri.conf.json, frontend assets, icons.
│       └── src/
│           ├── main.rs             #   ~15 LOC: env vars + bigbox_app::run()
│           └── lib.rs              #   run(): Builder, manage state, register handlers, GTK layout
```

### Dependency DAG (every edge points inward / downward)

```
                         bigbox-app  (bin + Builder + windowing)
                         /         \
                bigbox-shell      bigbox-vorcaro
                   /   |            /    |     \
                  /    |   bigbox-orchestrator  bigbox-drivers
                 /     |       /   |   \            /     \
                /      |  config   |   cloud  contract  driver-assets
               |       |    |      |    |        |          (leaf)
               |       +----+------+----+--------+
               |                  |
               +----> bigbox-core <-----  (everyone; the leaf vocabulary)
                          ^
                          |
                  bigbox-contract --> bigbox-core
```

- **Leaves (no internal deps):** `bigbox-core`, `bigbox-driver-assets`.
- **Tauri-free middle:** `contract`, `config`, `cloud`, `drivers`, `orchestrator`.
- **Tauri edge (the only crates that may name `tauri`):** `shell`, `vorcaro`, `app`.
- `shell` and `vorcaro` are **siblings** — neither depends on the other. They are
  joined only at `bigbox-app`. This is what makes them compile in parallel.

---

## 2. Workspace `Cargo.toml` (root)

```toml
[workspace]
resolver = "2"
members  = ["crates/*"]

# Single source of truth for versions. Each crate writes `dep.workspace = true`.
[workspace.dependencies]
serde      = { version = "1", features = ["derive"] }
serde_json = "1"
toml       = "0.8"
dirs       = "5"
uuid       = { version = "1", features = ["v4", "serde"] }
chrono     = { version = "0.4", features = ["serde"] }
csv        = "1"
rand       = "0.8"
base64     = "0.22"
tokio      = { version = "1", features = ["sync", "time", "rt", "macros"] }
reqwest    = { version = "0.12", features = ["json", "rustls-tls"], default-features = false }
tauri      = { version = "2", features = ["devtools", "unstable"] }
tauri-build = { version = "2", features = [] }

# Internal crates (path deps, also versioned for future publish):
bigbox-core          = { path = "crates/bigbox-core" }
bigbox-contract      = { path = "crates/bigbox-contract" }
bigbox-driver-assets = { path = "crates/bigbox-driver-assets" }
bigbox-config        = { path = "crates/bigbox-config" }
bigbox-cloud         = { path = "crates/bigbox-cloud" }
bigbox-drivers       = { path = "crates/bigbox-drivers" }
bigbox-orchestrator  = { path = "crates/bigbox-orchestrator" }
bigbox-shell         = { path = "crates/bigbox-shell" }
bigbox-vorcaro       = { path = "crates/bigbox-vorcaro" }

[workspace.package]
version = "0.1.7"
edition = "2021"
license = "GPL-3.0-or-later"
authors = ["Heitor Faria"]

# MUST live in the workspace root — [patch] is workspace-global, not per-crate.
[patch.crates-io]
wry = { git = "https://github.com/podheitor/wry", branch = "bigbox-0.54.4" }

# Profiles are workspace-global. Keep release tuning here.
[profile.release]
opt-level     = 3
lto           = true
codegen-units = 1
strip         = true
panic         = "abort"

# Optional but recommended: faster incremental dev builds for the heavy crates.
# [profile.dev.package."*"]
# opt-level = 1
```

> **Note on `lto = true` + `codegen-units = 1`.** These maximize *runtime*
> performance but partly serialize the *final* release link/codegen — they do
> not undo the dev-build parallelism win. Keep them for release; day-to-day
> iteration uses `cargo build` (dev profile), which is where the parallelism and
> small-blast-radius wins land. If release link time becomes painful, relax to
> `codegen-units = 16` and measure; do not touch dev.

---

## 3. Per-crate `Cargo.toml` examples

All inherit `version`/`edition`/`license` from `[workspace.package]`.

### `crates/bigbox-core/Cargo.toml`
```toml
[package]
name = "bigbox-core"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
serde.workspace = true
chrono.workspace = true
uuid.workspace = true
# NOTHING heavy. No tauri, no tokio, no reqwest. This crate must stay cheap.
```

### `crates/bigbox-contract/Cargo.toml`
```toml
[package]
name = "bigbox-contract"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
bigbox-core.workspace = true
# That is the whole point: the 400-method trait depends on the vocabulary and
# nothing else, so it compiles fast and rarely, and fans out to implementors.
```

### `crates/bigbox-driver-assets/Cargo.toml`
```toml
[package]
name = "bigbox-driver-assets"
version.workspace = true
edition.workspace = true
license.workspace = true
# No [dependencies]. Just include_str! of assets/*.js.
```

### `crates/bigbox-config/Cargo.toml`
```toml
[package]
name = "bigbox-config"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
bigbox-core.workspace = true
serde.workspace = true
serde_json.workspace = true
toml.workspace = true
dirs.workspace = true
```

### `crates/bigbox-cloud/Cargo.toml`
```toml
[package]
name = "bigbox-cloud"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
bigbox-core.workspace = true
serde.workspace = true
serde_json.workspace = true
reqwest.workspace = true
```

### `crates/bigbox-drivers/Cargo.toml`
```toml
[package]
name = "bigbox-drivers"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
bigbox-contract.workspace = true
bigbox-core.workspace = true
bigbox-driver-assets.workspace = true
```

### `crates/bigbox-orchestrator/Cargo.toml`
```toml
[package]
name = "bigbox-orchestrator"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
bigbox-contract.workspace = true
bigbox-core.workspace = true
bigbox-config.workspace = true
bigbox-cloud.workspace = true
tokio.workspace = true
chrono.workspace = true
csv.workspace = true
base64.workspace = true
rand.workspace = true
# NO tauri. If you reach for it here, you are in the wrong layer — see Section 4.
```

### `crates/bigbox-shell/Cargo.toml`
```toml
[package]
name = "bigbox-shell"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
tauri.workspace = true
bigbox-core.workspace = true
bigbox-config.workspace = true
bigbox-driver-assets.workspace = true
```

### `crates/bigbox-vorcaro/Cargo.toml`
```toml
[package]
name = "bigbox-vorcaro"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
tauri.workspace = true
bigbox-core.workspace = true
bigbox-config.workspace = true
bigbox-orchestrator.workspace = true
bigbox-cloud.workspace = true
bigbox-drivers.workspace = true
```

### `crates/bigbox-app/Cargo.toml`
```toml
[package]
name = "bigbox-app"
version.workspace = true
edition.workspace = true
license.workspace = true
description = "All your messaging apps in one window — cross-platform"

[lib]
name = "bigbox_app"
crate-type = ["staticlib", "cdylib", "rlib"]

[[bin]]
name = "bigbox"
path = "src/main.rs"

[build-dependencies]
tauri-build.workspace = true

[dependencies]
tauri.workspace = true
bigbox-shell.workspace = true
bigbox-vorcaro.workspace = true
# state types that Builder.manage() needs may also require: bigbox-core, bigbox-config

[target.'cfg(target_os = "linux")'.dependencies]
gtk   = { version = "0.18", features = ["v3_24"] }
cairo = { version = "0.18", package = "cairo-rs" }
webkit2gtk = "2.0"
zbus = { version = "5", default-features = false, features = ["tokio"] }
```

---

## 4. The one hard inversion: getting Tauri out of the engine

The orchestrator currently *reaches out* to Tauri (`AppHandle`, `Emitter`,
`wv.eval`, `tauri::State`). To make it a clean, Tauri-free crate we apply
**ports & adapters**: the engine depends on **traits it defines** (in
`bigbox-contract`), and the Tauri layer (`bigbox-vorcaro`) provides the concrete
implementations.

Define in `bigbox-contract` (sketch):

```rust
// Port 1: how the engine pushes a command into a live chat webview and awaits
// the driver's reply. The Tauri layer implements this with wv.eval + oneshot.
#[async_trait::async_trait]   // or native `async fn in trait` (stable since 1.75)
pub trait DriverTransport: Send + Sync {
    async fn send_to(&self, service_label: &str, payload: SendPayload)
        -> Result<SendOutcome, TransportError>;
}

// Port 2: how the engine emits progress to the UI. Tauri layer implements with Emitter.
pub trait ProgressSink: Send + Sync {
    fn emit_progress(&self, event: ProgressEvent);
}
```

The engine signature becomes, e.g.:

```rust
pub async fn run_campaign(
    campaign: Campaign,
    transport: Arc<dyn DriverTransport>,
    progress:  Arc<dyn ProgressSink>,
    store:     Arc<Mutex<VorcaroState>>,   // plain std Arc/Mutex, not tauri::State
) -> Result<(), EngineError> { /* tokio loop, no tauri symbols */ }
```

`bigbox-vorcaro` (the Tauri crate) implements both ports over `AppHandle`/
`Emitter`/`get_webview` and injects them when starting a campaign.

> **Trade-off.** This costs ~2 small adapter structs + threading `Arc<dyn …>`
> through the engine entry points, instead of grabbing `AppHandle` ad hoc. We
> accept it: it is the price of HARD CONSTRAINTS #4 (cross-crate comms via traits)
> and #5 (no outward deps), and it makes the engine unit-testable without a
> running Tauri app — a real long-term maintenance win.
>
> **Rejected alternative.** Letting `bigbox-orchestrator` depend on `tauri`
> directly. It is less code today but pins the most-edited crate behind the
> heaviest dependency, defeating the entire purpose of the refactor. Not chosen.

The 400-method semantic contract (`trait PlatformDriver` or similar) lives beside
these ports in `bigbox-contract`. It stays **one trait** — see Section 7.

---

## 5. `lib.rs` / `main.rs` skeletons

### `crates/bigbox-app/src/main.rs` — minimal, no logic
```rust
// Prevents extra console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    #[cfg(target_os = "linux")]
    bigbox_app::set_linux_webkit_env(); // the NO_AT_BRIDGE / compositing env block

    bigbox_app::run();
}
```

### `crates/bigbox-app/src/lib.rs` — wiring only (the only place that knows every layer)
```rust
pub const TITLEBAR_H: i32 = bigbox_core::layout::TITLEBAR_H;
pub const SIDEBAR_W:  i32 = bigbox_core::layout::SIDEBAR_W;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(bigbox_shell::AppState::default())
        .manage(bigbox_vorcaro::VorcaroStore::default())
        .manage(bigbox_vorcaro::OrchestratorState::default())
        .invoke_handler(tauri::generate_handler![
            bigbox_shell::get_config,    /* …18 shell cmds… */
            bigbox_vorcaro::vorcaro_get_state, /* …35 vorcaro cmds… */
        ])
        .setup(|app| {
            bigbox_vorcaro::rehydrate_on_boot(app.handle().clone());
            #[cfg(target_os = "linux")]   setup_gtk_layout(app)?;     // windowing lives here
            #[cfg(target_os = "windows")] bigbox_shell::precreate_service_windows(app.handle());
            wire_window_tracking(app)?;   // resize/move → reposition webviews
            wire_menu_events(app);        // mark-read / reload / remove
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running bigbox");
}

// GTK overlay layout, set_linux_webkit_env(), wire_* helpers stay in this crate —
// they are app-composition concerns, not domain logic.
```

### `crates/bigbox-orchestrator/src/lib.rs` — engine, no tauri
```rust
pub use port::{DriverTransport, ProgressSink}; // re-exported from contract for callers
pub async fn run_campaign(/* see Section 4 */) -> Result<(), EngineError> { /* … */ }
```

### `crates/bigbox-driver-assets/src/lib.rs`
```rust
pub const WHATSAPP: &str = include_str!("../assets/whatsapp.js");
pub const TELEGRAM: &str = include_str!("../assets/telegram.js");

/// Dev override: load driver JS from $BB_DRIVER_DIR at runtime so iterating on a
/// driver does NOT require a Rust rebuild (we did this ~27 times). Falls back to
/// the embedded const. Release builds ship the embedded copy.
pub fn whatsapp() -> std::borrow::Cow<'static, str> { load_or("whatsapp.js", WHATSAPP) }
```

> **Trade-off (driver iteration).** `include_str!` still rebuilds *downstream*
> when the .js changes. Since we iterate drivers constantly, we add the
> `BB_DRIVER_DIR` runtime-load path for dev: edit JS, restart app, **no rebuild**.
> Release stays fully embedded (no external files to ship). This directly targets
> the workflow pain in `start_here.md`.

---

## 6. Why this enables *real* multithreaded builds

Cargo's unit of compilation, of incremental invalidation, and of scheduling is
the **crate**. With one crate today, all three are the whole codebase. Splitting
into the DAG above changes each:

1. **Parallel scheduling across the DAG.** Cargo builds independent crates
   concurrently (bounded by `-j` / the jobserver). At `cargo build`:
   - `bigbox-core` and `bigbox-driver-assets` (the two leaves) start
     **immediately and in parallel**.
   - Once `core` is done, `contract`, `config`, `cloud` build **in parallel**.
   - Once `contract` is done, `drivers` and the bulk of `orchestrator` build in
     parallel with `cloud`/`config`.
   - `shell` and `vorcaro` are siblings → they build **in parallel** (this is the
     big one: the heavy Tauri/wry/webkit compilation now happens on two crates
     concurrently instead of being one serial blob).
   - `app` links last. A single monolith offers none of this overlap.

2. **Small incremental blast radius.** Editing the campaign loop recompiles
   `orchestrator` + its downstream (`vorcaro`, `app`) — but **not** `tauri`/`wry`
   object code (only relinking), **not** `shell`, **not** `core`/`contract`/
   `cloud`. Today the same edit recompiles the entire crate including all Tauri
   glue. The keep-Tauri-out-of-the-engine rule (Section 4) is what guarantees the
   most-edited code never drags the heaviest dependency.

3. **The giant trait fans out instead of serializing.** When the 400-method
   contract changes, its implementors (`bigbox-drivers`, future platform crates)
   and consumers (`orchestrator`) recompile **in parallel** behind the single
   `contract` build. In a monolith, the trait, every impl, and every caller are
   one codegen unit — recompiled together, serially, on any edit anywhere in the
   crate, not just on trait edits.

4. **Heavy deps are quarantined.** `reqwest`/`tokio` rebuild concerns stay in
   `cloud`/`orchestrator`; `tauri`/`wry`/`webkit2gtk` stay in `shell`/`vorcaro`/
   `app`. A version bump to one no longer forces recompilation of unrelated
   domains.

---

## 7. Rules for where future code must live (enforce in review)

- **A new pure data type / ID / error / UI constant →** `bigbox-core`. Nothing
  else may define the shared vocabulary.
- **A new method on the semantic contract →** `bigbox-contract`, on the existing
  trait. **Never** create a second "contract" trait to avoid touching the big
  one, and **never** put it in a leaf consumer.
- **The 400-method trait stays one trait.** Do **not** split it into sub-traits
  unless it becomes a genuine *type-system* need (e.g. you require a smaller
  object-safe view, or distinct supertrait bounds for a generic). Splitting for
  "tidiness" fragments a single semantic contract and buys no build win — the
  crate boundary already gives the isolation. If a split is ever justified, it is
  a typed decomposition (supertraits in the same crate), not a file move.
- **A new platform automation impl →** `bigbox-drivers` (impl of the contract).
- **A new JS driver blob →** `bigbox-driver-assets/assets/*.js`.
- **Any on-disk persistence or embedded asset →** `bigbox-config`.
- **A new outbound network/API client →** `bigbox-cloud` (or its own leaf crate
  if unrelated to messaging cloud APIs).
- **New campaign/scheduling/engine logic →** `bigbox-orchestrator`. It MUST stay
  Tauri-free: if you need to reach the UI or a webview, add/extend a **port
  trait** in `bigbox-contract` and implement it in `bigbox-vorcaro`.
- **A new IPC command for the webview host →** `bigbox-shell`, registered in
  `bigbox-app`.
- **A new IPC command for CRM/campaigns →** `bigbox-vorcaro`, registered in
  `bigbox-app`.
- **`tauri::Builder`, window/GTK/Win32 layout, `generate_context!`,
  `tauri.conf.json`, frontend, icons →** `bigbox-app` only.
- **Hard rule:** `tauri` may appear in the dependency list of **only**
  `bigbox-shell`, `bigbox-vorcaro`, `bigbox-app`. A PR that adds `tauri` to any
  other crate is wrong by construction — the logic belongs one layer up, behind a
  port. Consider a CI check (`cargo metadata` / `cargo-deny`) that fails if
  `tauri` appears under `core/contract/config/cloud/drivers/orchestrator/
  driver-assets`.

---

## 8. File-by-file migration map (for execution)

| From (`src-tauri/src/…`)                | To                                                            |
|-----------------------------------------|--------------------------------------------------------------|
| `vorcaro/model.rs`                      | `bigbox-core` (types) — split persistence out                |
| `config.rs` types, `services.rs` `ServiceDef` | `bigbox-core` (the structs)                            |
| UI consts `TITLEBAR_H`/`SIDEBAR_W`      | `bigbox-core::layout`                                         |
| `config.rs` load/save, `services.rs` catalog/`session_dir`, `vorcaro/store.rs` | `bigbox-config` |
| `data/services.json`                    | `crates/bigbox-config/data/services.json`                    |
| `vorcaro/cloud_api.rs`                  | `bigbox-cloud`                                                |
| `vorcaro/drivers.rs` (2 consts)         | `bigbox-driver-assets/assets/*.js` + `src/lib.rs`            |
| (new) the 400-method trait + ports      | `bigbox-contract`                                            |
| (new) per-platform trait impls          | `bigbox-drivers`                                              |
| `vorcaro/orchestrator.rs` (de-Tauri'd)  | `bigbox-orchestrator` (see Section 4)                        |
| `vorcaro/csv_io.rs`, `vorcaro/attachments.rs` | `bigbox-orchestrator` (engine-side IO)                 |
| `commands.rs` (53 cmds: the webview-host ones) | `bigbox-shell`                                        |
| `vorcaro/mod.rs` (the `vorcaro_*` IPC fns + `VorcaroStore` + port adapters) | `bigbox-vorcaro`        |
| `main.rs`                               | `bigbox-app/src/main.rs` (trimmed)                           |
| `lib.rs` (Builder, GTK layout, setup)   | `bigbox-app/src/lib.rs`                                       |
| `tauri.conf.json`, `icons/`, `build.rs`, `frontend/` | `crates/bigbox-app/`                            |

### Suggested execution order (each step compiles before the next)
1. Create workspace skeleton + empty crates; move `main`/`lib`/`commands`/
   `config`/`services`/`vorcaro` *wholesale* into `bigbox-app` first so the
   workspace builds unchanged. (Pure mechanical; green build checkpoint.)
2. Extract leaves: `bigbox-core`, then `bigbox-driver-assets`. Re-point imports.
3. Extract `bigbox-config` (persistence + catalog).
4. Extract `bigbox-cloud`.
5. **The hard one:** define `bigbox-contract` ports, invert
   orchestrator → extract `bigbox-orchestrator` (Tauri-free). Add adapters in app.
6. Split the Tauri IPC: `bigbox-shell` (host) and `bigbox-vorcaro` (CRM); leave
   only Builder wiring in `bigbox-app`.
7. Introduce `bigbox-contract` 400-method trait + `bigbox-drivers` impls when the
   typed driver layer actually lands (today drivers are JS assets; the Rust trait
   is the forward-looking contract).
8. Add the CI guard from Section 7 + the `BB_DRIVER_DIR` dev path.

Keep a green build at every step. Validate on BigLinux with the deploy recipe in
memory (`biglinux-kill-relaunch-after-deploy.md`) — and **restart BigBox after
every install**.

---

## 9. Constraint compliance check

| HARD CONSTRAINT                                   | Satisfied by |
|---------------------------------------------------|--------------|
| 1. Cargo workspace, not a single crate            | Section 2 (`[workspace]`, 10 members) |
| 2. Each domain its own crate                      | Section 1 (domains ↔ crates 1:1) |
| 3. `main.rs` almost no logic                      | Section 5 (env + `run()`) |
| 4. Cross-crate comms via traits / public APIs     | Section 4 (`DriverTransport`/`ProgressSink` ports; contract trait) |
| 5. No sideways deps; deps point inward            | Section 1 DAG (shell ∥ vorcaro; all → core/contract) |
| 6. Compiles under stable Rust                     | native `async fn` in traits is stable (1.75+); no nightly features |
