# ShowTrack Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Local-first TV show + movie tracker (TV Time style) that imports the user's TV Time export and tracks watching going forward, per the approved spec at `docs/superpowers/specs/2026-07-02-showtrack-design.md`.

**Architecture:** Tauri 2 desktop app. Rust core owns SQLite (library + cached TMDB metadata), the TMDB client, an import engine for the TV Time export HTML, a refresh engine for airing shows, and a custom `poster://` protocol that lazily caches TMDB images on disk. SvelteKit (Svelte 5, adapter-static SPA) frontend renders five screens and calls Tauri commands; all mutations are optimistic.

**Tech Stack:** Rust (rusqlite bundled, reqwest, chrono, thiserror, async-trait, tokio fs/sync), Tauri 2 + tauri-plugin-dialog, SvelteKit 2 + Svelte 5 (TS, adapter-static, fallback SPA), pnpm.

## Global Constraints

- Repo: `G:\Projects\showtrack` (git already initialized, spec committed). All paths below are repo-relative.
- Identifier `com.nokama.showtrack`, product name **ShowTrack**, window dark bg `#0e0f13`, `visible: false` at launch (shown from Rust on page load — the qobuz-dl no-white-flash pattern; never set `"maximized": true` in tauri.conf.json).
- Frontend versions mirror the known-working PitchScope set: `@sveltejs/kit ^2.9.0`, `svelte ^5.0.0`, `@sveltejs/adapter-static ^3.0.6`, `vite ^6.0.3`, `typescript ~5.6.2`, `@tauri-apps/api ^2`, `@tauri-apps/cli ^2`.
- All serde DTOs crossing IPC use `#[serde(rename_all = "camelCase")]`.
- All Tauri commands are `async fn`; DB access only via the `with_db` spawn_blocking helper; never hold a `MutexGuard` across an `.await`.
- **Never use `INSERT OR REPLACE`** on `shows`, `episodes`, or `movies` — REPLACE deletes the conflicting row first and `ON DELETE CASCADE` would silently wipe `episode_watches`. Always `INSERT … ON CONFLICT(…) DO UPDATE`.
- Dates: `air_date` stored as `YYYY-MM-DD` TEXT (string comparison = date comparison); timestamps as RFC3339 TEXT. "Aired" means `air_date IS NOT NULL AND air_date <= today(local)`.
- TV Time export lives at `%USERPROFILE%\Downloads\tvtime-summary-2026-07-01.html` (used by `#[ignore]` integration tests only; unit tests use the committed fixture).
- Runnable exe: `pnpm tauri build --no-bundle` (never bare `cargo build --release` — webview would point at the dev server). Before any rebuild that replaces the exe: `Get-Process showtrack -ErrorAction SilentlyContinue | Stop-Process -Force`.
- Rust tests run from `src-tauri/`: `cargo test`. Frontend gates: `pnpm check` and `pnpm build` clean.
- Commit after every task (repo-local git identity already configured).

## File Structure

```
package.json / svelte.config.js / vite.config.js / tsconfig.json / .gitignore / app-icon.png
src/
  app.html, app.css
  lib/
    types.ts          # TS mirrors of Rust DTOs
    api.ts            # typed invoke() wrappers + imgUrl() helper
    ui/PosterImg.svelte, ProgressBar.svelte, ShowCard.svelte, Modal.svelte
  routes/
    +layout.ts (ssr=false), +layout.svelte (sidebar, onboarding gate, launch refresh, banner)
    +page.svelte                 # Shows (Watch Next)
    upcoming/+page.svelte
    movies/+page.svelte
    search/+page.svelte
    settings/+page.svelte        # key, import wizard, refresh, cache info
    show/[id]/+page.svelte       # show detail
src-tauri/
  Cargo.toml, build.rs, tauri.conf.json, capabilities/default.json, icons/
  src/
    main.rs, lib.rs      # builder wiring, AppState, poster protocol registration
    error.rs             # AppError (thiserror) + Result alias
    models.rs            # IPC DTOs (ShowCard, ShowDetail, MovieRow, ImportReport, …)
    db.rs                # open/migrate (PRAGMA user_version), open_memory for tests
    queries.rs           # every SQL statement in the app
    export.rs            # TV Time export HTML parser
    tmdb.rs              # Tmdb trait, TmdbClient, JSON parse fns, meta DTOs
    import.rs            # pure marking logic + import engine + report
    refresh.rs           # refresh engine for airing shows
    images.rs            # disk cache + poster:// handler
    commands.rs          # thin #[tauri::command] wrappers
  tests/fixtures/sample_export.html
```

Interfaces used by more than one task are defined once in Task 2 (`models.rs`), Task 4 (`tmdb.rs` meta types), and Task 6 (`queries.rs` signatures) — later tasks consume them exactly as written there.

---

### Task 1: Scaffold project (compilable Tauri 2 + SvelteKit skeleton)

**Files:**
- Create: `package.json`, `svelte.config.js`, `vite.config.js`, `tsconfig.json`, `.gitignore`, `src/app.html`, `src/app.css`, `src/routes/+layout.ts`, `src/routes/+layout.svelte`, `src/routes/+page.svelte`, `src-tauri/Cargo.toml`, `src-tauri/build.rs`, `src-tauri/tauri.conf.json`, `src-tauri/capabilities/default.json`, `src-tauri/src/main.rs`, `src-tauri/src/lib.rs`, `app-icon.png`, `src-tauri/icons/*` (generated)

**Interfaces:**
- Produces: a repo where `pnpm check`, `pnpm build`, and `cargo check` all pass; library crate name `showtrack_lib`.

- [ ] **Step 1: Write root config files**

`package.json`:
```json
{
  "name": "showtrack",
  "version": "0.1.0",
  "description": "Personal TV show and movie tracker",
  "type": "module",
  "scripts": {
    "dev": "vite dev",
    "build": "vite build",
    "preview": "vite preview",
    "check": "svelte-kit sync && svelte-check --tsconfig ./tsconfig.json",
    "tauri": "tauri"
  },
  "license": "MIT",
  "dependencies": {
    "@tauri-apps/api": "^2",
    "@tauri-apps/plugin-dialog": "^2"
  },
  "devDependencies": {
    "@sveltejs/adapter-static": "^3.0.6",
    "@sveltejs/kit": "^2.9.0",
    "@sveltejs/vite-plugin-svelte": "^5.0.0",
    "@tauri-apps/cli": "^2",
    "svelte": "^5.0.0",
    "svelte-check": "^4.0.0",
    "typescript": "~5.6.2",
    "vite": "^6.0.3"
  }
}
```

`svelte.config.js`:
```js
import adapter from "@sveltejs/adapter-static";
import { vitePreprocess } from "@sveltejs/vite-plugin-svelte";

/** @type {import('@sveltejs/kit').Config} */
const config = {
  preprocess: vitePreprocess(),
  kit: { adapter: adapter({ fallback: "index.html" }) },
};
export default config;
```

`vite.config.js`:
```js
import { defineConfig } from "vite";
import { sveltekit } from "@sveltejs/kit/vite";

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;
export default defineConfig(async () => ({
  plugins: [sveltekit()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: "ws", host, port: 1421 } : undefined,
    watch: { ignored: ["**/src-tauri/**"] },
  },
}));
```

`tsconfig.json`:
```json
{
  "extends": "./.svelte-kit/tsconfig.json",
  "compilerOptions": {
    "allowJs": true,
    "checkJs": true,
    "esModuleInterop": true,
    "forceConsistentCasingInFileNames": true,
    "resolveJsonModule": true,
    "skipLibCheck": true,
    "sourceMap": true,
    "strict": true
  }
}
```

`.gitignore`:
```
node_modules
/build
/.svelte-kit
/src-tauri/target
/src-tauri/gen/schemas
```

- [ ] **Step 2: Write minimal frontend shell**

`src/app.html`:
```html
<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>ShowTrack</title>
    %sveltekit.head%
  </head>
  <body style="background:#0e0f13" data-sveltekit-preload-data="hover">
    <div style="display: contents">%sveltekit.body%</div>
  </body>
</html>
```

`src/app.css` (design tokens used by every later UI task):
```css
:root {
  --bg: #0e0f13; --panel: #161821; --panel2: #1f2233; --border: #2a2d3e;
  --text: #e8e9ee; --dim: #9a9db0; --accent: #ffd166; --accent-ink: #14151a;
  --green: #3ddc84; --red: #ff5470; --radius: 10px;
}
* { box-sizing: border-box; }
body {
  margin: 0; background: var(--bg); color: var(--text);
  font-family: "Segoe UI", system-ui, sans-serif; font-size: 14px; line-height: 1.45;
}
a { color: inherit; text-decoration: none; }
h1 { font-size: 20px; margin: 0 0 16px; }
h2 { font-size: 15px; color: var(--dim); margin: 24px 0 10px; text-transform: uppercase; letter-spacing: .06em; }
button {
  font: inherit; cursor: pointer; border: 0; border-radius: 8px;
  background: var(--panel2); color: var(--text); padding: 6px 12px;
}
button:hover { filter: brightness(1.15); }
button.primary { background: var(--accent); color: var(--accent-ink); font-weight: 600; }
button:disabled { opacity: .5; cursor: default; }
input, select {
  font: inherit; background: var(--panel); color: var(--text);
  border: 1px solid var(--border); border-radius: 8px; padding: 7px 10px; outline: none;
}
input:focus, select:focus { border-color: var(--accent); }
.panel { background: var(--panel); border: 1px solid var(--border); border-radius: var(--radius); padding: 16px; }
.grid-posters { display: grid; grid-template-columns: repeat(auto-fill, minmax(150px, 1fr)); gap: 16px; }
.chips { display: flex; gap: 8px; margin-bottom: 16px; flex-wrap: wrap; }
.chip { padding: 4px 12px; border-radius: 999px; background: var(--panel2); color: var(--dim); cursor: pointer; }
.chip.active { background: var(--accent); color: var(--accent-ink); font-weight: 600; }
.dim { color: var(--dim); }
.banner { background: #3a2f10; border: 1px solid #6b5a1e; color: #ffd166; border-radius: 8px; padding: 8px 12px; margin-bottom: 16px; display: flex; justify-content: space-between; gap: 12px; }
```

`src/routes/+layout.ts`:
```ts
export const ssr = false;
export const prerender = false;
```

`src/routes/+layout.svelte` (placeholder; replaced in Task 11):
```svelte
<script lang="ts">
  import "../app.css";
  let { children } = $props();
</script>
{@render children()}
```

`src/routes/+page.svelte` (placeholder; replaced in Task 13):
```svelte
<h1>ShowTrack</h1>
```

- [ ] **Step 3: Write src-tauri skeleton**

`src-tauri/Cargo.toml`:
```toml
[package]
name = "showtrack"
version = "0.1.0"
description = "Personal TV show and movie tracker"
authors = ["Nokama"]
edition = "2021"

[lib]
name = "showtrack_lib"
crate-type = ["staticlib", "cdylib", "rlib"]

[build-dependencies]
tauri-build = { version = "2", features = [] }

[dependencies]
tauri = { version = "2", features = [] }
tauri-plugin-dialog = "2"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
rusqlite = { version = "0.31", features = ["bundled"] }
reqwest = { version = "0.12", features = ["json"] }
chrono = { version = "0.4", features = ["serde"] }
thiserror = "2"
async-trait = "0.1"
tokio = { version = "1", features = ["sync", "fs", "macros", "rt", "time"] }

[profile.release]
opt-level = "z"
lto = true
codegen-units = 1
strip = true
```

`src-tauri/build.rs`:
```rust
fn main() {
    tauri_build::build()
}
```

`src-tauri/tauri.conf.json`:
```json
{
  "$schema": "https://schema.tauri.app/config/2",
  "productName": "ShowTrack",
  "version": "0.1.0",
  "identifier": "com.nokama.showtrack",
  "build": {
    "beforeDevCommand": "pnpm dev",
    "devUrl": "http://localhost:1420",
    "beforeBuildCommand": "pnpm build",
    "frontendDist": "../build"
  },
  "app": {
    "windows": [
      {
        "title": "ShowTrack",
        "width": 1400,
        "height": 900,
        "visible": false,
        "backgroundColor": "#0e0f13"
      }
    ],
    "security": { "csp": null }
  },
  "bundle": {
    "active": true,
    "targets": "all",
    "icon": [
      "icons/32x32.png",
      "icons/128x128.png",
      "icons/128x128@2x.png",
      "icons/icon.icns",
      "icons/icon.ico"
    ]
  }
}
```

`src-tauri/capabilities/default.json`:
```json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "default",
  "description": "Capability for the main window",
  "windows": ["main"],
  "permissions": ["core:default", "dialog:default"]
}
```

`src-tauri/src/main.rs`:
```rust
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    showtrack_lib::run()
}
```

`src-tauri/src/lib.rs` (minimal for now; full wiring lands in Task 10):
```rust
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

- [ ] **Step 4: Generate icons**

Run (PowerShell, from repo root) — draws a yellow rounded square with "ST", then generates all icon sizes:
```powershell
Add-Type -AssemblyName System.Drawing
$bmp = New-Object System.Drawing.Bitmap(1024,1024)
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.SmoothingMode = "AntiAlias"
$g.Clear([System.Drawing.Color]::Transparent)
$path = New-Object System.Drawing.Drawing2D.GraphicsPath
$path.AddArc(0,0,300,300,180,90); $path.AddArc(724,0,300,300,270,90)
$path.AddArc(724,724,300,300,0,90); $path.AddArc(0,724,300,300,90,90); $path.CloseFigure()
$g.FillPath((New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(255,255,209,102))), $path)
$font = New-Object System.Drawing.Font("Segoe UI", 340, [System.Drawing.FontStyle]::Bold)
$fmt = New-Object System.Drawing.StringFormat
$fmt.Alignment = "Center"; $fmt.LineAlignment = "Center"
$g.DrawString("ST", $font, [System.Drawing.Brushes]::Black, (New-Object System.Drawing.RectangleF(0,20,1024,1024)), $fmt)
$g.Dispose(); $bmp.Save("app-icon.png"); $bmp.Dispose()
```
Then: `pnpm install` (first) and `pnpm tauri icon` (reads `./app-icon.png`, writes `src-tauri/icons/`).

- [ ] **Step 5: Verify everything compiles**

Run from repo root:
```
pnpm install
pnpm check
pnpm build
```
Expected: `pnpm check` reports 0 errors 0 warnings; `pnpm build` writes `build/` with `index.html`.

Run from `src-tauri/`:
```
cargo check
```
Expected: compiles cleanly (first run downloads crates; takes a while).

- [ ] **Step 6: Commit**

```
git add -A
git commit -m "chore: scaffold Tauri 2 + SvelteKit skeleton"
```

---

### Task 2: Rust foundation — error, models, db open/migrate, settings

**Files:**
- Create: `src-tauri/src/error.rs`, `src-tauri/src/models.rs`, `src-tauri/src/db.rs`
- Modify: `src-tauri/src/lib.rs` (add `mod` declarations)

**Interfaces:**
- Produces (used by every later Rust task):
  - `error::{AppError, Result<T>}` — `AppError::msg(impl Into<String>)` constructor
  - `db::open(path: &Path) -> Result<Connection>`, `db::open_memory() -> Result<Connection>` (schema applied, `foreign_keys=ON`)
  - `models::*` DTOs exactly as defined below
  - Settings helpers live in Task 6's `queries.rs`; this task embeds them in `db.rs` tests only via raw SQL.

- [ ] **Step 1: Write failing test for db open/migrate**

`src-tauri/src/db.rs` (tests first — module body in Step 3):
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrates_to_v1_with_foreign_keys() {
        let conn = open_memory().unwrap();
        let v: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(v, 1);
        let fk: i64 = conn.query_row("PRAGMA foreign_keys", [], |r| r.get(0)).unwrap();
        assert_eq!(fk, 1);
        // all five tables exist
        let n: i64 = conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN
             ('shows','episodes','episode_watches','movies','settings')", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 5);
    }

    #[test]
    fn cascade_deletes_watches_with_show() {
        let conn = open_memory().unwrap();
        conn.execute_batch(
            "INSERT INTO shows(tmdb_id,name,my_state,added_at) VALUES(1,'X','watching','2026-01-01T00:00:00Z');
             INSERT INTO episodes(tmdb_id,show_tmdb_id,season_number,episode_number) VALUES(10,1,1,1);
             INSERT INTO episode_watches(episode_tmdb_id,watched_at,source) VALUES(10,NULL,'import');
             DELETE FROM shows WHERE tmdb_id=1;").unwrap();
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM episode_watches", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

From `src-tauri/`: `cargo test db::` — Expected: FAIL to compile (`open_memory` not defined).

- [ ] **Step 3: Implement error.rs, db.rs, models.rs**

`src-tauri/src/error.rs`:
```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("db: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("http: {0}")]
    Http(#[from] reqwest::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Msg(String),
}

impl AppError {
    pub fn msg(s: impl Into<String>) -> Self {
        AppError::Msg(s.into())
    }
}

pub type Result<T> = std::result::Result<T, AppError>;
```

`src-tauri/src/db.rs`:
```rust
use std::path::Path;

use rusqlite::Connection;

use crate::error::Result;

const SCHEMA_V1: &str = "
CREATE TABLE shows (
  tmdb_id INTEGER PRIMARY KEY,
  tvdb_id INTEGER,
  imdb_id TEXT,
  name TEXT NOT NULL,
  year INTEGER,
  poster_path TEXT,
  backdrop_path TEXT,
  overview TEXT,
  tmdb_status TEXT,
  my_state TEXT NOT NULL DEFAULT 'watching'
    CHECK (my_state IN ('watching','stopped','plan_to_watch')),
  is_favorite INTEGER NOT NULL DEFAULT 0,
  added_at TEXT NOT NULL,
  last_refreshed_at TEXT
);
CREATE TABLE episodes (
  tmdb_id INTEGER PRIMARY KEY,
  show_tmdb_id INTEGER NOT NULL REFERENCES shows(tmdb_id) ON DELETE CASCADE,
  season_number INTEGER NOT NULL,
  episode_number INTEGER NOT NULL,
  name TEXT,
  air_date TEXT,
  runtime INTEGER,
  still_path TEXT,
  overview TEXT
);
CREATE INDEX idx_episodes_show ON episodes(show_tmdb_id, season_number, episode_number);
CREATE INDEX idx_episodes_air ON episodes(air_date);
CREATE TABLE episode_watches (
  episode_tmdb_id INTEGER PRIMARY KEY REFERENCES episodes(tmdb_id) ON DELETE CASCADE,
  watched_at TEXT,
  source TEXT NOT NULL DEFAULT 'app' CHECK (source IN ('app','import'))
);
CREATE TABLE movies (
  tmdb_id INTEGER PRIMARY KEY,
  tvdb_id INTEGER,
  imdb_id TEXT,
  tvtime_uuid TEXT,
  title TEXT NOT NULL,
  year INTEGER,
  release_date TEXT,
  runtime INTEGER,
  poster_path TEXT,
  backdrop_path TEXT,
  overview TEXT,
  my_state TEXT NOT NULL DEFAULT 'watchlist' CHECK (my_state IN ('watched','watchlist')),
  watched_at TEXT,
  rewatch_count INTEGER NOT NULL DEFAULT 0,
  is_favorite INTEGER NOT NULL DEFAULT 0,
  added_at TEXT NOT NULL
);
CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
";

pub fn open(path: &Path) -> Result<Connection> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let conn = Connection::open(path)?;
    let _mode: String = conn.query_row("PRAGMA journal_mode=WAL", [], |r| r.get(0))?;
    init(&conn)?;
    Ok(conn)
}

pub fn open_memory() -> Result<Connection> {
    let conn = Connection::open_in_memory()?;
    init(&conn)?;
    Ok(conn)
}

fn init(conn: &Connection) -> Result<()> {
    conn.pragma_update(None, "foreign_keys", "ON")?;
    let v: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if v < 1 {
        conn.execute_batch(SCHEMA_V1)?;
        conn.pragma_update(None, "user_version", 1)?;
    }
    Ok(())
}
```
(keep the `#[cfg(test)] mod tests` from Step 1 at the bottom of `db.rs`)

`src-tauri/src/models.rs` — every DTO that crosses IPC. Later tasks use these names verbatim:
```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NextEpisode {
    pub episode_id: i64,
    pub season_number: i64,
    pub episode_number: i64,
    pub name: Option<String>,
    pub air_date: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShowCard {
    pub tmdb_id: i64,
    pub name: String,
    pub poster_path: Option<String>,
    pub my_state: String,
    pub is_favorite: bool,
    pub watched: i64,
    pub total_aired: i64,
    pub next: Option<NextEpisode>,
    pub next_air_date: Option<String>,
    pub last_activity: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShowRow {
    pub tmdb_id: i64,
    pub tvdb_id: Option<i64>,
    pub imdb_id: Option<String>,
    pub name: String,
    pub year: Option<i64>,
    pub poster_path: Option<String>,
    pub backdrop_path: Option<String>,
    pub overview: Option<String>,
    pub tmdb_status: Option<String>,
    pub my_state: String,
    pub is_favorite: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EpisodeRow {
    pub tmdb_id: i64,
    pub season_number: i64,
    pub episode_number: i64,
    pub name: Option<String>,
    pub air_date: Option<String>,
    pub runtime: Option<i64>,
    pub still_path: Option<String>,
    pub overview: Option<String>,
    pub watched: bool,
    pub watched_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeasonDetail {
    pub season_number: i64,
    pub episodes: Vec<EpisodeRow>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShowDetail {
    pub show: ShowRow,
    pub seasons: Vec<SeasonDetail>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MovieRow {
    pub tmdb_id: i64,
    pub imdb_id: Option<String>,
    pub title: String,
    pub year: Option<i64>,
    pub release_date: Option<String>,
    pub runtime: Option<i64>,
    pub poster_path: Option<String>,
    pub backdrop_path: Option<String>,
    pub overview: Option<String>,
    pub my_state: String,
    pub watched_at: Option<String>,
    pub rewatch_count: i64,
    pub is_favorite: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpcomingItem {
    pub episode_id: i64,
    pub show_tmdb_id: i64,
    pub show_name: String,
    pub poster_path: Option<String>,
    pub season_number: i64,
    pub episode_number: i64,
    pub episode_name: Option<String>,
    pub air_date: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchItem {
    pub tmdb_id: i64,
    pub media_type: String, // "tv" | "movie"
    pub title: String,
    pub year: Option<i64>,
    pub poster_path: Option<String>,
    pub overview: Option<String>,
    pub in_library: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportProgress {
    pub stage: String, // "shows" | "movies"
    pub done: usize,
    pub total: usize,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportFailure {
    pub kind: String, // "show" | "movie"
    pub title: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportAnomaly {
    pub title: String,
    pub tmdb_id: i64,
    pub expected: i64,
    pub actual: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpecialsNote {
    pub title: String,
    pub watched_specials: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    pub shows_ok: i64,
    pub movies_ok: i64,
    pub failures: Vec<ImportFailure>,
    pub anomalies: Vec<ImportAnomaly>,
    pub specials_note: Vec<SpecialsNote>,
    pub finished_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshSummary {
    pub refreshed: i64,
    pub failed: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheInfo {
    pub path: String,
    pub bytes: u64,
}
```

Add to the top of `src-tauri/src/lib.rs`:
```rust
pub mod db;
pub mod error;
pub mod models;
```

- [ ] **Step 4: Run tests to verify they pass**

From `src-tauri/`: `cargo test db::` — Expected: 2 passed.

- [ ] **Step 5: Commit**

```
git add -A
git commit -m "feat: sqlite schema, error type, IPC models"
```

---

### Task 3: TV Time export parser

**Files:**
- Create: `src-tauri/src/export.rs`, `src-tauri/tests/fixtures/sample_export.html`
- Modify: `src-tauri/src/lib.rs` (add `pub mod export;`)

**Interfaces:**
- Produces:
  - `export::parse_export(html: &str) -> Result<TvTimeExport>`
  - `export::TvTimeExport { export_date: chrono::NaiveDate, shows: Vec<ExportShow>, movies: Vec<ExportMovie> }`
  - `export::ExportShow { title: String, tvdb_id: i64, imdb_id: Option<String>, status: String, is_favorite: bool, watched: i64, total: i64, total_specials: i64, watched_specials: i64, unwatched_regular_eps: Vec<SeasonEp>, watched_no_date: i64 }`
  - `export::SeasonEp { season: i64, episode: i64 }`
  - `export::ExportMovie { title: String, tvdb_id: i64, imdb_id: Option<String>, uuid: String, year: Option<i64>, is_watched: bool, is_favorite: bool, watched_at: Option<String>, rewatch_count: i64 }`

- [ ] **Step 1: Create the fixture**

`src-tauri/tests/fixtures/sample_export.html` (mimics the real file's structure: one `var SHOWS` line, one `var MOVIES` line, date in `<title>`):
```html
<!DOCTYPE html>
<html lang="en">
<head>
  <title>TV Time Out by Refract — Export Summary 2026-07-01</title>
</head>
<body>
  <script>
    var SHOWS  = [{"title":"Fully Watched","tvdbId":323168,"imdbId":null,"status":"up_to_date","isFavorite":false,"watched":49,"total":49,"pct":100,"totalSpecials":3,"watchedSpecials":0,"pctSpecials":0,"ghostEntry":false,"allMissingDates":false,"noEpisodeData":false,"unwatchedRegularEps":[],"watchedNoDate":0,"rowClass":"row-green","tvtHref":"https://app.tvtime.com/series/323168","tvdbHref":"https://www.thetvdb.com/series/323168","imdbHref":null,"flaggedEps":[]},{"title":"Partially Watched","tvdbId":253463,"imdbId":null,"status":"stopped","isFavorite":true,"watched":26,"total":32,"pct":81,"totalSpecials":1,"watchedSpecials":2,"pctSpecials":0,"ghostEntry":false,"allMissingDates":false,"noEpisodeData":false,"unwatchedRegularEps":[{"season":7,"episode":1},{"season":7,"episode":2}],"watchedNoDate":0,"rowClass":"","tvtHref":"https://app.tvtime.com/series/253463","tvdbHref":"https://www.thetvdb.com/series/253463","imdbHref":null,"flaggedEps":[]}];
    var MOVIES = [{"title":"Watched Movie","tvdbId":1674,"imdbId":"tt5294550","uuid":"fee9ddd4-c517-48f1-9ef4-7437bc4c57c3","year":2017,"isWatched":true,"isFavorite":false,"watchedAt":"2024-06-09T15:05:22Z","rewatchCount":2,"rowClass":"row-green","tvtHref":"https://app.tvtime.com/movie/fee9ddd4","tvdbHref":"https://www.thetvdb.com/movies/1674","imdbHref":"https://www.imdb.com/title/tt5294550"},{"title":"Watchlist Movie","tvdbId":15763,"imdbId":"tt0439572","uuid":"fadbb018-b0e3-45be-aca4-59a817539996","year":null,"isWatched":false,"isFavorite":false,"watchedAt":null,"rewatchCount":0,"rowClass":"","tvtHref":"https://app.tvtime.com/movie/fadbb018","tvdbHref":"https://www.thetvdb.com/movies/15763","imdbHref":"https://www.imdb.com/title/tt0439572"}];
  </script>
</body>
</html>
```

- [ ] **Step 2: Write failing tests**

Bottom of `src-tauri/src/export.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("../tests/fixtures/sample_export.html");

    #[test]
    fn parses_fixture() {
        let ex = parse_export(FIXTURE).unwrap();
        assert_eq!(ex.export_date, chrono::NaiveDate::from_ymd_opt(2026, 7, 1).unwrap());
        assert_eq!(ex.shows.len(), 2);
        assert_eq!(ex.movies.len(), 2);

        let s = &ex.shows[1];
        assert_eq!(s.title, "Partially Watched");
        assert_eq!(s.tvdb_id, 253463);
        assert_eq!(s.status, "stopped");
        assert!(s.is_favorite);
        assert_eq!(s.watched, 26);
        assert_eq!(s.total, 32);
        assert_eq!(s.watched_specials, 2);
        assert_eq!(s.unwatched_regular_eps.len(), 2);
        assert_eq!(s.unwatched_regular_eps[0].season, 7);
        assert_eq!(s.unwatched_regular_eps[0].episode, 1);

        let m = &ex.movies[0];
        assert_eq!(m.title, "Watched Movie");
        assert_eq!(m.imdb_id.as_deref(), Some("tt5294550"));
        assert!(m.is_watched);
        assert_eq!(m.watched_at.as_deref(), Some("2024-06-09T15:05:22Z"));
        assert_eq!(m.rewatch_count, 2);
        assert!(!ex.movies[1].is_watched);
        assert_eq!(ex.movies[1].year, None);
    }

    #[test]
    fn rejects_html_without_data() {
        assert!(parse_export("<html><body>nope</body></html>").is_err());
    }

    /// Integration check against the real export. Run manually:
    /// cargo test real_export -- --ignored --nocapture
    #[test]
    #[ignore]
    fn real_export() {
        let html = std::fs::read_to_string(
            r"%USERPROFILE%\Downloads\tvtime-summary-2026-07-01.html").unwrap();
        let ex = parse_export(&html).unwrap();
        assert_eq!(ex.shows.len(), 171);
        assert_eq!(ex.movies.len(), 594);
        assert!(ex.shows.iter().all(|s| s.tvdb_id > 0));
        assert!(ex.movies.iter().all(|m| m.imdb_id.is_some()));
    }
}
```

- [ ] **Step 3: Run tests to verify they fail**

From `src-tauri/`: `cargo test export::` — Expected: FAIL to compile (`parse_export` not defined).

- [ ] **Step 4: Implement the parser**

Top of `src-tauri/src/export.rs`:
```rust
use chrono::NaiveDate;
use serde::Deserialize;

use crate::error::{AppError, Result};

#[derive(Debug, Clone, Deserialize)]
pub struct SeasonEp {
    pub season: i64,
    pub episode: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportShow {
    pub title: String,
    pub tvdb_id: i64,
    #[serde(default)]
    pub imdb_id: Option<String>,
    pub status: String,
    pub is_favorite: bool,
    pub watched: i64,
    pub total: i64,
    #[serde(default)]
    pub total_specials: i64,
    #[serde(default)]
    pub watched_specials: i64,
    #[serde(default)]
    pub unwatched_regular_eps: Vec<SeasonEp>,
    #[serde(default)]
    pub watched_no_date: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportMovie {
    pub title: String,
    pub tvdb_id: i64,
    #[serde(default)]
    pub imdb_id: Option<String>,
    pub uuid: String,
    #[serde(default)]
    pub year: Option<i64>,
    pub is_watched: bool,
    pub is_favorite: bool,
    #[serde(default)]
    pub watched_at: Option<String>,
    #[serde(default)]
    pub rewatch_count: i64,
}

#[derive(Debug, Clone)]
pub struct TvTimeExport {
    pub export_date: NaiveDate,
    pub shows: Vec<ExportShow>,
    pub movies: Vec<ExportMovie>,
}

/// Grab the `[...]` JSON array from a `var NAME = [...];` line.
fn extract_array<'a>(line: &'a str) -> Option<&'a str> {
    let start = line.find('[')?;
    let end = line.rfind(']')?;
    if end <= start {
        return None;
    }
    Some(&line[start..=end])
}

pub fn parse_export(html: &str) -> Result<TvTimeExport> {
    let mut shows_json: Option<&str> = None;
    let mut movies_json: Option<&str> = None;
    for line in html.lines() {
        let t = line.trim_start();
        if t.starts_with("var SHOWS") {
            shows_json = extract_array(line);
        } else if t.starts_with("var MOVIES") {
            movies_json = extract_array(line);
        }
    }
    let shows_json = shows_json.ok_or_else(|| AppError::msg("no `var SHOWS` array found — is this a TV Time Out export?"))?;
    let movies_json = movies_json.ok_or_else(|| AppError::msg("no `var MOVIES` array found — is this a TV Time Out export?"))?;

    let marker = "Export Summary ";
    let pos = html.find(marker).ok_or_else(|| AppError::msg("no export date found"))?;
    let date_str = html.get(pos + marker.len()..pos + marker.len() + 10)
        .ok_or_else(|| AppError::msg("truncated export date"))?;
    let export_date = NaiveDate::parse_from_str(date_str, "%Y-%m-%d")
        .map_err(|e| AppError::msg(format!("bad export date {date_str:?}: {e}")))?;

    Ok(TvTimeExport {
        export_date,
        shows: serde_json::from_str(shows_json)?,
        movies: serde_json::from_str(movies_json)?,
    })
}
```

Add `pub mod export;` to `src-tauri/src/lib.rs`.

- [ ] **Step 5: Run tests to verify they pass**

From `src-tauri/`: `cargo test export::` — Expected: 2 passed, 1 ignored.
Then run the real-file check once: `cargo test real_export -- --ignored` — Expected: 1 passed.

- [ ] **Step 6: Commit**

```
git add -A
git commit -m "feat: TV Time export parser with fixture and real-file test"
```

---

### Task 4: TMDB client (trait, DTOs, parse functions, HTTP impl)

**Files:**
- Create: `src-tauri/src/tmdb.rs`
- Modify: `src-tauri/src/lib.rs` (add `pub mod tmdb;`)

**Interfaces:**
- Consumes: `error::{AppError, Result}`
- Produces (used by import/refresh/commands):
  - `tmdb::ShowMeta { tmdb_id: i64, name: String, year: Option<i64>, poster_path: Option<String>, backdrop_path: Option<String>, overview: Option<String>, status: Option<String>, season_numbers: Vec<i64> }`
  - `tmdb::EpisodeMeta { tmdb_id: i64, season_number: i64, episode_number: i64, name: Option<String>, air_date: Option<chrono::NaiveDate>, runtime: Option<i64>, still_path: Option<String>, overview: Option<String> }`
  - `tmdb::MovieMeta { tmdb_id: i64, imdb_id: Option<String>, title: String, year: Option<i64>, release_date: Option<String>, runtime: Option<i64>, poster_path: Option<String>, backdrop_path: Option<String>, overview: Option<String> }`
  - `tmdb::ShowFull { meta: ShowMeta, episodes: Vec<EpisodeMeta> }`
  - `tmdb::SearchResultItem { tmdb_id: i64, media_type: String, title: String, year: Option<i64>, poster_path: Option<String>, overview: Option<String> }`
  - trait `tmdb::Tmdb` (async_trait, `Send + Sync`):
    - `async fn find_show(&self, tvdb_id: i64) -> Result<Option<i64>>`
    - `async fn find_movie(&self, imdb_id: &str) -> Result<Option<i64>>`
    - `async fn show_with_episodes(&self, tmdb_id: i64) -> Result<ShowFull>`
    - `async fn movie(&self, tmdb_id: i64) -> Result<MovieMeta>`
    - `async fn search(&self, query: &str) -> Result<Vec<SearchResultItem>>`
    - `async fn validate(&self) -> Result<bool>`
  - `tmdb::TmdbClient::new(http: reqwest::Client, key: String) -> TmdbClient` (implements `Tmdb`)

- [ ] **Step 1: Write failing tests for the pure JSON parse functions**

Bottom of `src-tauri/src/tmdb.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_find_results() {
        let v = json!({"tv_results":[{"id":1396}],"movie_results":[]});
        assert_eq!(parse_find(&v, "tv_results"), Some(1396));
        assert_eq!(parse_find(&v, "movie_results"), None);
    }

    #[test]
    fn parses_tv_base() {
        let v = json!({
            "id": 1396, "name": "Breaking Bad", "first_air_date": "2008-01-20",
            "poster_path": "/p.jpg", "backdrop_path": "/b.jpg", "overview": "Chemistry.",
            "status": "Ended",
            "seasons": [{"season_number": 0}, {"season_number": 1}, {"season_number": 2}]
        });
        let m = parse_tv_base(&v).unwrap();
        assert_eq!(m.tmdb_id, 1396);
        assert_eq!(m.name, "Breaking Bad");
        assert_eq!(m.year, Some(2008));
        assert_eq!(m.status.as_deref(), Some("Ended"));
        assert_eq!(m.season_numbers, vec![0, 1, 2]);
    }

    #[test]
    fn parses_season_episodes_with_empty_air_date() {
        let v = json!({"episodes":[
            {"id": 62085, "season_number": 1, "episode_number": 1, "name": "Pilot",
             "air_date": "2008-01-20", "runtime": 58, "still_path": "/s.jpg", "overview": "x"},
            {"id": 62086, "season_number": 1, "episode_number": 2, "name": "TBA",
             "air_date": "", "runtime": null, "still_path": null, "overview": ""}
        ]});
        let eps = parse_season_episodes(&v);
        assert_eq!(eps.len(), 2);
        assert_eq!(eps[0].air_date, chrono::NaiveDate::from_ymd_opt(2008, 1, 20));
        assert_eq!(eps[1].air_date, None);
        assert_eq!(eps[1].runtime, None);
    }

    #[test]
    fn parses_movie() {
        let v = json!({"id": 27205, "imdb_id": "tt1375666", "title": "Inception",
            "release_date": "2010-07-15", "runtime": 148,
            "poster_path": "/i.jpg", "backdrop_path": null, "overview": "Dreams."});
        let m = parse_movie(&v).unwrap();
        assert_eq!(m.tmdb_id, 27205);
        assert_eq!(m.year, Some(2010));
        assert_eq!(m.release_date.as_deref(), Some("2010-07-15"));
    }

    #[test]
    fn parses_search_skipping_people() {
        let v = json!({"results":[
            {"media_type":"tv","id":1,"name":"Show","first_air_date":"2020-05-05","poster_path":null,"overview":"o"},
            {"media_type":"movie","id":2,"title":"Film","release_date":"1999-01-01","poster_path":"/f.jpg","overview":"o"},
            {"media_type":"person","id":3,"name":"Actor"}
        ]});
        let r = parse_search(&v);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].media_type, "tv");
        assert_eq!(r[0].title, "Show");
        assert_eq!(r[0].year, Some(2020));
        assert_eq!(r[1].media_type, "movie");
        assert_eq!(r[1].title, "Film");
    }

    /// Live smoke test — needs a real key. Run manually:
    /// TMDB_KEY=... cargo test tmdb_live -- --ignored --nocapture   (PowerShell: $env:TMDB_KEY="...")
    #[tokio::test]
    #[ignore]
    async fn tmdb_live() {
        let key = std::env::var("TMDB_KEY").expect("set TMDB_KEY");
        let c = TmdbClient::new(reqwest::Client::new(), key);
        assert!(c.validate().await.unwrap());
        // Breaking Bad: TVDB 81189 -> TMDB 1396
        assert_eq!(c.find_show(81189).await.unwrap(), Some(1396));
        let full = c.show_with_episodes(1396).await.unwrap();
        assert_eq!(full.meta.name, "Breaking Bad");
        assert!(full.episodes.len() >= 62);
        let m = c.movie(27205).await.unwrap();
        assert_eq!(m.title, "Inception");
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

From `src-tauri/`: `cargo test tmdb::` — Expected: FAIL to compile.

- [ ] **Step 3: Implement tmdb.rs**

```rust
use async_trait::async_trait;
use chrono::NaiveDate;
use serde_json::Value;
use tokio::sync::Semaphore;

use crate::error::{AppError, Result};

const API: &str = "https://api.themoviedb.org/3";

#[derive(Debug, Clone)]
pub struct ShowMeta {
    pub tmdb_id: i64,
    pub name: String,
    pub year: Option<i64>,
    pub poster_path: Option<String>,
    pub backdrop_path: Option<String>,
    pub overview: Option<String>,
    pub status: Option<String>,
    pub season_numbers: Vec<i64>,
}

#[derive(Debug, Clone)]
pub struct EpisodeMeta {
    pub tmdb_id: i64,
    pub season_number: i64,
    pub episode_number: i64,
    pub name: Option<String>,
    pub air_date: Option<NaiveDate>,
    pub runtime: Option<i64>,
    pub still_path: Option<String>,
    pub overview: Option<String>,
}

#[derive(Debug, Clone)]
pub struct MovieMeta {
    pub tmdb_id: i64,
    pub imdb_id: Option<String>,
    pub title: String,
    pub year: Option<i64>,
    pub release_date: Option<String>,
    pub runtime: Option<i64>,
    pub poster_path: Option<String>,
    pub backdrop_path: Option<String>,
    pub overview: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ShowFull {
    pub meta: ShowMeta,
    pub episodes: Vec<EpisodeMeta>,
}

#[derive(Debug, Clone)]
pub struct SearchResultItem {
    pub tmdb_id: i64,
    pub media_type: String,
    pub title: String,
    pub year: Option<i64>,
    pub poster_path: Option<String>,
    pub overview: Option<String>,
}

#[async_trait]
pub trait Tmdb: Send + Sync {
    async fn find_show(&self, tvdb_id: i64) -> Result<Option<i64>>;
    async fn find_movie(&self, imdb_id: &str) -> Result<Option<i64>>;
    async fn show_with_episodes(&self, tmdb_id: i64) -> Result<ShowFull>;
    async fn movie(&self, tmdb_id: i64) -> Result<MovieMeta>;
    async fn search(&self, query: &str) -> Result<Vec<SearchResultItem>>;
    async fn validate(&self) -> Result<bool>;
}

pub struct TmdbClient {
    http: reqwest::Client,
    key: String,
    sem: Semaphore,
}

impl TmdbClient {
    pub fn new(http: reqwest::Client, key: String) -> Self {
        Self { http, key, sem: Semaphore::new(4) }
    }

    /// GET an API path (path may already contain a query string). Adds auth,
    /// throttles to 4 in-flight, retries once after 500ms on HTTP 429.
    async fn get_json(&self, path_and_query: &str) -> Result<Value> {
        let _permit = self.sem.acquire().await.map_err(|e| AppError::msg(e.to_string()))?;
        let sep = if path_and_query.contains('?') { '&' } else { '?' };
        // v4 tokens are JWTs (contain '.'), v3 keys are plain hex
        let (url, bearer) = if self.key.contains('.') {
            (format!("{API}{path_and_query}"), true)
        } else {
            (format!("{API}{path_and_query}{sep}api_key={}", self.key), false)
        };
        for attempt in 0..2 {
            let mut req = self.http.get(&url);
            if bearer {
                req = req.bearer_auth(&self.key);
            }
            let resp = req.send().await?;
            if resp.status().as_u16() == 429 && attempt == 0 {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                continue;
            }
            if resp.status().as_u16() == 404 {
                return Err(AppError::msg("not found on TMDB (404)"));
            }
            let resp = resp.error_for_status()?;
            return Ok(resp.json().await?);
        }
        unreachable!()
    }
}

fn s(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(|x| x.as_str()).filter(|x| !x.is_empty()).map(str::to_string)
}

fn i(v: &Value, key: &str) -> Option<i64> {
    v.get(key).and_then(|x| x.as_i64())
}

fn year_of(date: Option<&str>) -> Option<i64> {
    date.and_then(|d| d.get(..4)).and_then(|y| y.parse().ok())
}

pub fn parse_find(v: &Value, list: &str) -> Option<i64> {
    v.get(list)?.as_array()?.first().and_then(|r| r.get("id")).and_then(|x| x.as_i64())
}

pub fn parse_tv_base(v: &Value) -> Result<ShowMeta> {
    Ok(ShowMeta {
        tmdb_id: i(v, "id").ok_or_else(|| AppError::msg("tv: missing id"))?,
        name: s(v, "name").ok_or_else(|| AppError::msg("tv: missing name"))?,
        year: year_of(s(v, "first_air_date").as_deref()),
        poster_path: s(v, "poster_path"),
        backdrop_path: s(v, "backdrop_path"),
        overview: s(v, "overview"),
        status: s(v, "status"),
        season_numbers: v.get("seasons").and_then(|x| x.as_array()).map(|arr| {
            arr.iter().filter_map(|sn| i(sn, "season_number")).collect()
        }).unwrap_or_default(),
    })
}

/// Parse the `episodes` array of one season object.
pub fn parse_season_episodes(v: &Value) -> Vec<EpisodeMeta> {
    let Some(arr) = v.get("episodes").and_then(|x| x.as_array()) else {
        return Vec::new();
    };
    arr.iter().filter_map(|e| {
        Some(EpisodeMeta {
            tmdb_id: i(e, "id")?,
            season_number: i(e, "season_number")?,
            episode_number: i(e, "episode_number")?,
            name: s(e, "name"),
            air_date: s(e, "air_date").and_then(|d| NaiveDate::parse_from_str(&d, "%Y-%m-%d").ok()),
            runtime: i(e, "runtime"),
            still_path: s(e, "still_path"),
            overview: s(e, "overview"),
        })
    }).collect()
}

pub fn parse_movie(v: &Value) -> Result<MovieMeta> {
    let release_date = s(v, "release_date");
    Ok(MovieMeta {
        tmdb_id: i(v, "id").ok_or_else(|| AppError::msg("movie: missing id"))?,
        imdb_id: s(v, "imdb_id"),
        title: s(v, "title").ok_or_else(|| AppError::msg("movie: missing title"))?,
        year: year_of(release_date.as_deref()),
        release_date,
        runtime: i(v, "runtime"),
        poster_path: s(v, "poster_path"),
        backdrop_path: s(v, "backdrop_path"),
        overview: s(v, "overview"),
    })
}

pub fn parse_search(v: &Value) -> Vec<SearchResultItem> {
    let Some(arr) = v.get("results").and_then(|x| x.as_array()) else {
        return Vec::new();
    };
    arr.iter().filter_map(|r| {
        let media_type = s(r, "media_type")?;
        let (title, date) = match media_type.as_str() {
            "tv" => (s(r, "name")?, s(r, "first_air_date")),
            "movie" => (s(r, "title")?, s(r, "release_date")),
            _ => return None,
        };
        Some(SearchResultItem {
            tmdb_id: i(r, "id")?,
            media_type,
            title,
            year: year_of(date.as_deref()),
            poster_path: s(r, "poster_path"),
            overview: s(r, "overview"),
        })
    }).collect()
}

#[async_trait]
impl Tmdb for TmdbClient {
    async fn find_show(&self, tvdb_id: i64) -> Result<Option<i64>> {
        let v = self.get_json(&format!("/find/{tvdb_id}?external_source=tvdb_id")).await?;
        Ok(parse_find(&v, "tv_results"))
    }

    async fn find_movie(&self, imdb_id: &str) -> Result<Option<i64>> {
        let v = self.get_json(&format!("/find/{imdb_id}?external_source=imdb_id")).await?;
        Ok(parse_find(&v, "movie_results"))
    }

    async fn show_with_episodes(&self, tmdb_id: i64) -> Result<ShowFull> {
        let base = self.get_json(&format!("/tv/{tmdb_id}")).await?;
        let meta = parse_tv_base(&base)?;
        let mut episodes = Vec::new();
        // append_to_response accepts at most 20 sub-requests per call
        for chunk in meta.season_numbers.chunks(20) {
            let append = chunk.iter().map(|n| format!("season/{n}"))
                .collect::<Vec<_>>().join(",");
            let v = self.get_json(&format!("/tv/{tmdb_id}?append_to_response={append}")).await?;
            for n in chunk {
                if let Some(season) = v.get(format!("season/{n}")) {
                    episodes.extend(parse_season_episodes(season));
                }
            }
        }
        Ok(ShowFull { meta, episodes })
    }

    async fn movie(&self, tmdb_id: i64) -> Result<MovieMeta> {
        let v = self.get_json(&format!("/movie/{tmdb_id}")).await?;
        parse_movie(&v)
    }

    async fn search(&self, query: &str) -> Result<Vec<SearchResultItem>> {
        let q: String = url_encode(query);
        let v = self.get_json(&format!("/search/multi?include_adult=false&query={q}")).await?;
        Ok(parse_search(&v))
    }

    async fn validate(&self) -> Result<bool> {
        match self.get_json("/configuration").await {
            Ok(_) => Ok(true),
            Err(AppError::Http(e)) if e.status().map(|s| s.as_u16() == 401).unwrap_or(false) => Ok(false),
            Err(e) => Err(e),
        }
    }
}

/// Minimal percent-encoding for a query value (no extra crate needed).
fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}
```

Add `pub mod tmdb;` to `src-tauri/src/lib.rs`.

Note: `#[tokio::test]` and `tokio::time::sleep` rely on the tokio `rt`/`time`/`macros` features, already present in Cargo.toml from Task 1.

- [ ] **Step 4: Run tests to verify they pass**

From `src-tauri/`: `cargo test tmdb::` — Expected: 5 passed, 1 ignored.

- [ ] **Step 5: Commit**

```
git add -A
git commit -m "feat: TMDB client with trait, parse fns, throttling"
```

---

### Task 5: Import marking logic (pure functions)

**Files:**
- Create: `src-tauri/src/import.rs` (pure half; the engine half arrives in Task 7)
- Modify: `src-tauri/src/lib.rs` (add `pub mod import;`)

**Interfaces:**
- Consumes: `tmdb::EpisodeMeta`, `export::SeasonEp`
- Produces:
  - `import::episodes_to_mark(episodes: &[EpisodeMeta], unwatched: &[SeasonEp], cutoff: NaiveDate) -> Vec<i64>`
  - `import::map_status(tvtime_status: &str) -> &'static str`

- [ ] **Step 1: Write failing tests**

`src-tauri/src/import.rs`:
```rust
use std::collections::HashSet;

use chrono::NaiveDate;

use crate::export::SeasonEp;
use crate::tmdb::EpisodeMeta;

/// TV Time state -> ShowTrack my_state.
pub fn map_status(tvtime_status: &str) -> &'static str {
    match tvtime_status {
        "stopped" => "stopped",
        "not_started_yet" => "plan_to_watch",
        _ => "watching", // up_to_date, continuing, unknown
    }
}

/// Which episode tmdb_ids should be marked watched on import:
/// regular seasons only (season > 0), aired on/before the export date,
/// and not listed in the export's unwatched list.
pub fn episodes_to_mark(
    episodes: &[EpisodeMeta],
    unwatched: &[SeasonEp],
    cutoff: NaiveDate,
) -> Vec<i64> {
    let skip: HashSet<(i64, i64)> = unwatched.iter().map(|u| (u.season, u.episode)).collect();
    episodes.iter()
        .filter(|e| e.season_number > 0)
        .filter(|e| e.air_date.map(|d| d <= cutoff).unwrap_or(false))
        .filter(|e| !skip.contains(&(e.season_number, e.episode_number)))
        .map(|e| e.tmdb_id)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ep(id: i64, season: i64, num: i64, air: Option<&str>) -> EpisodeMeta {
        EpisodeMeta {
            tmdb_id: id,
            season_number: season,
            episode_number: num,
            name: None,
            air_date: air.map(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").unwrap()),
            runtime: None,
            still_path: None,
            overview: None,
        }
    }

    fn cutoff() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 7, 1).unwrap()
    }

    #[test]
    fn marks_aired_regular_episodes() {
        let eps = vec![ep(1, 1, 1, Some("2020-01-01")), ep(2, 1, 2, Some("2020-01-08"))];
        assert_eq!(episodes_to_mark(&eps, &[], cutoff()), vec![1, 2]);
    }

    #[test]
    fn skips_specials_unaired_future_and_unwatched() {
        let eps = vec![
            ep(1, 0, 1, Some("2020-01-01")),  // special -> skip
            ep(2, 1, 1, Some("2020-01-01")),  // keep
            ep(3, 1, 2, None),                // no air date -> skip
            ep(4, 1, 3, Some("2026-07-02")),  // after cutoff -> skip
            ep(5, 1, 4, Some("2026-07-01")),  // on cutoff -> keep
            ep(6, 7, 1, Some("2025-04-10")),  // in unwatched list -> skip
        ];
        let unwatched = vec![SeasonEp { season: 7, episode: 1 }];
        assert_eq!(episodes_to_mark(&eps, &unwatched, cutoff()), vec![2, 5]);
    }

    #[test]
    fn maps_tvtime_status() {
        assert_eq!(map_status("up_to_date"), "watching");
        assert_eq!(map_status("continuing"), "watching");
        assert_eq!(map_status("stopped"), "stopped");
        assert_eq!(map_status("not_started_yet"), "plan_to_watch");
        assert_eq!(map_status("unknown"), "watching");
    }
}
```

- [ ] **Step 2: Run tests to verify they fail, then pass**

The code and tests land together here (the functions are 20 lines); temporarily comment out the function bodies if you want to observe the red state, then restore. From `src-tauri/` run: `cargo test import::` — Expected: 3 passed. Add `pub mod import;` to `lib.rs` first.

- [ ] **Step 3: Commit**

```
git add -A
git commit -m "feat: import marking rules (cutoff, specials, unwatched list)"
```

---

### Task 6: Library queries (all SQL)

**Files:**
- Create: `src-tauri/src/queries.rs`
- Modify: `src-tauri/src/lib.rs` (add `pub mod queries;`)

**Interfaces:**
- Consumes: `db`, `models::*`, `tmdb::{ShowMeta, EpisodeMeta, MovieMeta}`
- Produces (exact signatures; commands/import/refresh call these):
  - `setting(conn, key: &str) -> Result<Option<String>>` / `set_setting(conn, key: &str, value: &str) -> Result<()>`
  - `upsert_show_meta(conn, m: &ShowMeta, now: &str) -> Result<()>` — insert or metadata-only update (never touches `my_state`/`is_favorite`/`added_at`)
  - `set_show_user(conn, id: i64, state: &str, favorite: bool, tvdb_id: Option<i64>, imdb_id: Option<&str>) -> Result<()>`
  - `set_show_state(conn, id: i64, state: &str) -> Result<()>` / `toggle_show_favorite(conn, id: i64) -> Result<bool>`
  - `upsert_episodes(conn, show_id: i64, eps: &[EpisodeMeta]) -> Result<()>`
  - `mark_episodes_import(conn, ids: &[i64]) -> Result<()>` (INSERT OR IGNORE, NULL date, source 'import')
  - `set_episode_watched(conn, ep_id: i64, watched: bool, now: &str) -> Result<()>`
  - `mark_season(conn, show_id: i64, season: i64, watched: bool, today: &str, now: &str) -> Result<()>`
  - `mark_all_aired(conn, show_id: i64, today: &str, now: &str) -> Result<()>`
  - `watched_regular_count(conn, show_id: i64) -> Result<i64>`
  - `show_card(conn, id: i64, today: &str) -> Result<ShowCard>` / `list_show_cards(conn, today: &str) -> Result<Vec<ShowCard>>`
  - `show_detail(conn, id: i64) -> Result<ShowDetail>`
  - `list_upcoming(conn, today: &str) -> Result<Vec<UpcomingItem>>`
  - `upsert_movie_meta(conn, m: &MovieMeta, now: &str) -> Result<()>`
  - `set_movie_user(conn, id: i64, state: &str, watched_at: Option<&str>, rewatch: i64, favorite: bool, tvdb_id: Option<i64>, uuid: Option<&str>) -> Result<()>`
  - `set_movie_state(conn, id: i64, state: &str, now: &str) -> Result<()>` / `movie_rewatch(conn, id: i64) -> Result<i64>` / `toggle_movie_favorite(conn, id: i64) -> Result<bool>`
  - `list_movies(conn) -> Result<Vec<MovieRow>>`
  - `shows_to_refresh(conn) -> Result<Vec<i64>>` (watching + not Ended/Canceled)
  - `library_ids(conn) -> Result<(HashSet<i64>, HashSet<i64>)>` (show ids, movie ids)

- [ ] **Step 1: Write failing tests**

Bottom of `src-tauri/src/queries.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::tmdb::{EpisodeMeta, MovieMeta, ShowMeta};
    use chrono::NaiveDate;

    const NOW: &str = "2026-07-02T10:00:00Z";
    const TODAY: &str = "2026-07-02";

    fn show_meta(id: i64, name: &str) -> ShowMeta {
        ShowMeta {
            tmdb_id: id, name: name.into(), year: Some(2020),
            poster_path: Some("/p.jpg".into()), backdrop_path: None,
            overview: Some("o".into()), status: Some("Returning Series".into()),
            season_numbers: vec![1],
        }
    }

    fn ep(id: i64, season: i64, num: i64, air: &str) -> EpisodeMeta {
        EpisodeMeta {
            tmdb_id: id, season_number: season, episode_number: num,
            name: Some(format!("E{num}")),
            air_date: NaiveDate::parse_from_str(air, "%Y-%m-%d").ok(),
            runtime: Some(42), still_path: None, overview: None,
        }
    }

    fn movie_meta(id: i64, title: &str) -> MovieMeta {
        MovieMeta {
            tmdb_id: id, imdb_id: Some("tt1".into()), title: title.into(),
            year: Some(2017), release_date: Some("2017-05-01".into()), runtime: Some(120),
            poster_path: None, backdrop_path: None, overview: None,
        }
    }

    fn seed(conn: &rusqlite::Connection) {
        upsert_show_meta(conn, &show_meta(1, "Alpha"), NOW).unwrap();
        set_show_user(conn, 1, "watching", false, Some(999), None).unwrap();
        upsert_episodes(conn, 1, &[
            ep(10, 1, 1, "2026-06-01"),
            ep(11, 1, 2, "2026-06-08"),
            ep(12, 1, 3, "2026-07-10"), // future
            ep(13, 0, 1, "2026-06-01"), // special
        ]).unwrap();
    }

    #[test]
    fn settings_roundtrip() {
        let conn = db::open_memory().unwrap();
        assert_eq!(setting(&conn, "k").unwrap(), None);
        set_setting(&conn, "k", "v1").unwrap();
        set_setting(&conn, "k", "v2").unwrap();
        assert_eq!(setting(&conn, "k").unwrap(), Some("v2".into()));
    }

    #[test]
    fn upsert_show_preserves_user_state_and_watches() {
        let conn = db::open_memory().unwrap();
        seed(&conn);
        set_show_state(&conn, 1, "stopped").unwrap();
        set_episode_watched(&conn, 10, true, NOW).unwrap();
        // metadata refresh must not clobber state or cascade-delete watches
        upsert_show_meta(&conn, &show_meta(1, "Alpha Renamed"), NOW).unwrap();
        upsert_episodes(&conn, 1, &[ep(10, 1, 1, "2026-06-01")]).unwrap();
        let card = show_card(&conn, 1, TODAY).unwrap();
        assert_eq!(card.name, "Alpha Renamed");
        assert_eq!(card.my_state, "stopped");
        assert_eq!(card.watched, 1);
    }

    #[test]
    fn card_progress_next_and_upcoming() {
        let conn = db::open_memory().unwrap();
        seed(&conn);
        set_episode_watched(&conn, 10, true, NOW).unwrap();
        let card = show_card(&conn, 1, TODAY).unwrap();
        assert_eq!(card.watched, 1);
        assert_eq!(card.total_aired, 2); // E3 future, special excluded
        let next = card.next.unwrap();
        assert_eq!(next.episode_id, 11);
        assert_eq!(card.next_air_date.as_deref(), Some("2026-07-10"));
        assert_eq!(card.last_activity.as_deref(), Some(NOW));

        let up = list_upcoming(&conn, TODAY).unwrap();
        assert_eq!(up.len(), 1);
        assert_eq!(up[0].episode_id, 12);
        assert_eq!(up[0].show_name, "Alpha");

        // caught up: mark E2 too -> no next
        set_episode_watched(&conn, 11, true, NOW).unwrap();
        let card = show_card(&conn, 1, TODAY).unwrap();
        assert!(card.next.is_none());
    }

    #[test]
    fn import_marks_are_ignored_if_already_watched() {
        let conn = db::open_memory().unwrap();
        seed(&conn);
        set_episode_watched(&conn, 10, true, NOW).unwrap();
        mark_episodes_import(&conn, &[10, 11]).unwrap();
        // 10 keeps its app watch date; 11 is import (NULL date)
        let d = show_detail(&conn, 1).unwrap();
        let s1 = &d.seasons.iter().find(|s| s.season_number == 1).unwrap().episodes;
        assert_eq!(s1[0].watched_at.as_deref(), Some(NOW));
        assert!(s1[1].watched && s1[1].watched_at.is_none());
    }

    #[test]
    fn season_and_all_marks_only_aired() {
        let conn = db::open_memory().unwrap();
        seed(&conn);
        mark_season(&conn, 1, 1, true, TODAY, NOW).unwrap();
        assert_eq!(watched_regular_count(&conn, 1).unwrap(), 2); // not the future E3
        mark_season(&conn, 1, 1, false, TODAY, NOW).unwrap();
        assert_eq!(watched_regular_count(&conn, 1).unwrap(), 0);
        mark_all_aired(&conn, 1, TODAY, NOW).unwrap();
        // regular aired (2) + aired special (1)
        let d = show_detail(&conn, 1).unwrap();
        let watched: usize = d.seasons.iter().flat_map(|s| &s.episodes).filter(|e| e.watched).count();
        assert_eq!(watched, 3);
    }

    #[test]
    fn show_detail_groups_seasons() {
        let conn = db::open_memory().unwrap();
        seed(&conn);
        let d = show_detail(&conn, 1).unwrap();
        assert_eq!(d.show.name, "Alpha");
        assert_eq!(d.seasons.len(), 2); // season 0 and 1
        assert_eq!(d.seasons[0].season_number, 0);
        assert_eq!(d.seasons[1].episodes.len(), 3);
    }

    #[test]
    fn movie_lifecycle() {
        let conn = db::open_memory().unwrap();
        upsert_movie_meta(&conn, &movie_meta(50, "Film"), NOW).unwrap();
        set_movie_user(&conn, 50, "watched", Some("2024-06-09T15:05:22Z"), 2, true, Some(7), Some("uuid-1")).unwrap();
        let rows = list_movies(&conn).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].my_state, "watched");
        assert_eq!(rows[0].rewatch_count, 2);
        assert!(rows[0].is_favorite);

        assert_eq!(movie_rewatch(&conn, 50).unwrap(), 3);
        set_movie_state(&conn, 50, "watchlist", NOW).unwrap();
        let rows = list_movies(&conn).unwrap();
        assert_eq!(rows[0].my_state, "watchlist");
        assert!(rows[0].watched_at.is_none());
        // back to watched stamps now
        set_movie_state(&conn, 50, "watched", NOW).unwrap();
        assert_eq!(list_movies(&conn).unwrap()[0].watched_at.as_deref(), Some(NOW));
        assert!(!toggle_movie_favorite(&conn, 50).unwrap());
    }

    #[test]
    fn refresh_targets_and_library_ids() {
        let conn = db::open_memory().unwrap();
        seed(&conn); // status Returning Series, watching
        upsert_show_meta(&conn, &ShowMeta { status: Some("Ended".into()), ..show_meta(2, "Done") }, NOW).unwrap();
        set_show_user(&conn, 2, "watching", false, None, None).unwrap();
        upsert_show_meta(&conn, &show_meta(3, "Dropped"), NOW).unwrap();
        set_show_user(&conn, 3, "stopped", false, None, None).unwrap();
        assert_eq!(shows_to_refresh(&conn).unwrap(), vec![1]);

        upsert_movie_meta(&conn, &movie_meta(50, "Film"), NOW).unwrap();
        let (sids, mids) = library_ids(&conn).unwrap();
        assert!(sids.contains(&1) && sids.contains(&2) && sids.contains(&3));
        assert!(mids.contains(&50));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

From `src-tauri/`: `cargo test queries::` — Expected: FAIL to compile.

- [ ] **Step 3: Implement queries.rs**

```rust
use std::collections::HashSet;

use rusqlite::{params, Connection, OptionalExtension};

use crate::error::Result;
use crate::models::*;
use crate::tmdb::{EpisodeMeta, MovieMeta, ShowMeta};

// ---------- settings ----------

pub fn setting(conn: &Connection, key: &str) -> Result<Option<String>> {
    Ok(conn.query_row("SELECT value FROM settings WHERE key=?1", [key], |r| r.get(0)).optional()?)
}

pub fn set_setting(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO settings(key,value) VALUES(?1,?2)
         ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        params![key, value])?;
    Ok(())
}

// ---------- shows ----------

pub fn upsert_show_meta(conn: &Connection, m: &ShowMeta, now: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO shows(tmdb_id,name,year,poster_path,backdrop_path,overview,tmdb_status,added_at,last_refreshed_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?8)
         ON CONFLICT(tmdb_id) DO UPDATE SET
           name=excluded.name, year=excluded.year, poster_path=excluded.poster_path,
           backdrop_path=excluded.backdrop_path, overview=excluded.overview,
           tmdb_status=excluded.tmdb_status, last_refreshed_at=excluded.last_refreshed_at",
        params![m.tmdb_id, m.name, m.year, m.poster_path, m.backdrop_path, m.overview, m.status, now])?;
    Ok(())
}

pub fn set_show_user(conn: &Connection, id: i64, state: &str, favorite: bool,
                     tvdb_id: Option<i64>, imdb_id: Option<&str>) -> Result<()> {
    conn.execute(
        "UPDATE shows SET my_state=?2, is_favorite=?3,
           tvdb_id=COALESCE(?4,tvdb_id), imdb_id=COALESCE(?5,imdb_id) WHERE tmdb_id=?1",
        params![id, state, favorite as i64, tvdb_id, imdb_id])?;
    Ok(())
}

pub fn set_show_state(conn: &Connection, id: i64, state: &str) -> Result<()> {
    conn.execute("UPDATE shows SET my_state=?2 WHERE tmdb_id=?1", params![id, state])?;
    Ok(())
}

pub fn toggle_show_favorite(conn: &Connection, id: i64) -> Result<bool> {
    conn.execute("UPDATE shows SET is_favorite=1-is_favorite WHERE tmdb_id=?1", [id])?;
    Ok(conn.query_row("SELECT is_favorite FROM shows WHERE tmdb_id=?1", [id],
        |r| r.get::<_, i64>(0))? != 0)
}

pub fn upsert_episodes(conn: &Connection, show_id: i64, eps: &[EpisodeMeta]) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    {
        let mut st = tx.prepare(
            "INSERT INTO episodes(tmdb_id,show_tmdb_id,season_number,episode_number,name,air_date,runtime,still_path,overview)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)
             ON CONFLICT(tmdb_id) DO UPDATE SET
               show_tmdb_id=excluded.show_tmdb_id, season_number=excluded.season_number,
               episode_number=excluded.episode_number, name=excluded.name, air_date=excluded.air_date,
               runtime=excluded.runtime, still_path=excluded.still_path, overview=excluded.overview")?;
        for e in eps {
            st.execute(params![
                e.tmdb_id, show_id, e.season_number, e.episode_number, e.name,
                e.air_date.map(|d| d.to_string()), e.runtime, e.still_path, e.overview])?;
        }
    }
    tx.commit()?;
    Ok(())
}

// ---------- watches ----------

pub fn mark_episodes_import(conn: &Connection, ids: &[i64]) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    {
        let mut st = tx.prepare(
            "INSERT OR IGNORE INTO episode_watches(episode_tmdb_id,watched_at,source)
             VALUES(?1,NULL,'import')")?;
        for id in ids {
            st.execute([id])?;
        }
    }
    tx.commit()?;
    Ok(())
}

pub fn set_episode_watched(conn: &Connection, ep_id: i64, watched: bool, now: &str) -> Result<()> {
    if watched {
        conn.execute(
            "INSERT OR IGNORE INTO episode_watches(episode_tmdb_id,watched_at,source) VALUES(?1,?2,'app')",
            params![ep_id, now])?;
    } else {
        conn.execute("DELETE FROM episode_watches WHERE episode_tmdb_id=?1", [ep_id])?;
    }
    Ok(())
}

pub fn mark_season(conn: &Connection, show_id: i64, season: i64, watched: bool,
                   today: &str, now: &str) -> Result<()> {
    if watched {
        conn.execute(
            "INSERT OR IGNORE INTO episode_watches(episode_tmdb_id,watched_at,source)
             SELECT tmdb_id, ?4, 'app' FROM episodes
             WHERE show_tmdb_id=?1 AND season_number=?2 AND air_date IS NOT NULL AND air_date<=?3",
            params![show_id, season, today, now])?;
    } else {
        conn.execute(
            "DELETE FROM episode_watches WHERE episode_tmdb_id IN
             (SELECT tmdb_id FROM episodes WHERE show_tmdb_id=?1 AND season_number=?2)",
            params![show_id, season])?;
    }
    Ok(())
}

pub fn mark_all_aired(conn: &Connection, show_id: i64, today: &str, now: &str) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO episode_watches(episode_tmdb_id,watched_at,source)
         SELECT tmdb_id, ?3, 'app' FROM episodes
         WHERE show_tmdb_id=?1 AND air_date IS NOT NULL AND air_date<=?2",
        params![show_id, today, now])?;
    Ok(())
}

pub fn watched_regular_count(conn: &Connection, show_id: i64) -> Result<i64> {
    Ok(conn.query_row(
        "SELECT COUNT(*) FROM episode_watches w
         JOIN episodes e ON e.tmdb_id=w.episode_tmdb_id
         WHERE e.show_tmdb_id=?1 AND e.season_number>0",
        [show_id], |r| r.get(0))?)
}

// ---------- cards / detail / upcoming ----------

pub fn show_card(conn: &Connection, id: i64, today: &str) -> Result<ShowCard> {
    let (name, poster_path, my_state, is_favorite) = conn.query_row(
        "SELECT name, poster_path, my_state, is_favorite FROM shows WHERE tmdb_id=?1",
        [id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?,
                      r.get::<_, String>(2)?, r.get::<_, i64>(3)?)))?;
    let watched = watched_regular_count(conn, id)?;
    let total_aired: i64 = conn.query_row(
        "SELECT COUNT(*) FROM episodes
         WHERE show_tmdb_id=?1 AND season_number>0 AND air_date IS NOT NULL AND air_date<=?2",
        params![id, today], |r| r.get(0))?;
    let next = conn.query_row(
        "SELECT tmdb_id, season_number, episode_number, name, air_date FROM episodes e
         WHERE show_tmdb_id=?1 AND season_number>0 AND air_date IS NOT NULL AND air_date<=?2
           AND NOT EXISTS (SELECT 1 FROM episode_watches w WHERE w.episode_tmdb_id=e.tmdb_id)
         ORDER BY season_number, episode_number LIMIT 1",
        params![id, today],
        |r| Ok(NextEpisode {
            episode_id: r.get(0)?, season_number: r.get(1)?, episode_number: r.get(2)?,
            name: r.get(3)?, air_date: r.get(4)?,
        })).optional()?;
    let next_air_date: Option<String> = conn.query_row(
        "SELECT MIN(air_date) FROM episodes WHERE show_tmdb_id=?1 AND air_date>?2",
        params![id, today], |r| r.get(0))?;
    let last_activity: Option<String> = conn.query_row(
        "SELECT MAX(w.watched_at) FROM episode_watches w
         JOIN episodes e ON e.tmdb_id=w.episode_tmdb_id WHERE e.show_tmdb_id=?1",
        [id], |r| r.get(0))?;
    Ok(ShowCard {
        tmdb_id: id, name, poster_path, my_state,
        is_favorite: is_favorite != 0,
        watched, total_aired, next, next_air_date, last_activity,
    })
}

pub fn list_show_cards(conn: &Connection, today: &str) -> Result<Vec<ShowCard>> {
    let ids: Vec<i64> = conn.prepare("SELECT tmdb_id FROM shows ORDER BY name")?
        .query_map([], |r| r.get(0))?
        .collect::<std::result::Result<_, _>>()?;
    ids.into_iter().map(|id| show_card(conn, id, today)).collect()
}

pub fn show_detail(conn: &Connection, id: i64) -> Result<ShowDetail> {
    let show = conn.query_row(
        "SELECT tmdb_id, tvdb_id, imdb_id, name, year, poster_path, backdrop_path,
                overview, tmdb_status, my_state, is_favorite
         FROM shows WHERE tmdb_id=?1",
        [id],
        |r| Ok(ShowRow {
            tmdb_id: r.get(0)?, tvdb_id: r.get(1)?, imdb_id: r.get(2)?, name: r.get(3)?,
            year: r.get(4)?, poster_path: r.get(5)?, backdrop_path: r.get(6)?,
            overview: r.get(7)?, tmdb_status: r.get(8)?, my_state: r.get(9)?,
            is_favorite: r.get::<_, i64>(10)? != 0,
        }))?;
    let mut st = conn.prepare(
        "SELECT e.tmdb_id, e.season_number, e.episode_number, e.name, e.air_date,
                e.runtime, e.still_path, e.overview, w.watched_at,
                CASE WHEN w.episode_tmdb_id IS NULL THEN 0 ELSE 1 END
         FROM episodes e
         LEFT JOIN episode_watches w ON w.episode_tmdb_id=e.tmdb_id
         WHERE e.show_tmdb_id=?1
         ORDER BY e.season_number, e.episode_number")?;
    let rows = st.query_map([id], |r| {
        Ok(EpisodeRow {
            tmdb_id: r.get(0)?, season_number: r.get(1)?, episode_number: r.get(2)?,
            name: r.get(3)?, air_date: r.get(4)?, runtime: r.get(5)?,
            still_path: r.get(6)?, overview: r.get(7)?,
            watched_at: r.get(8)?, watched: r.get::<_, i64>(9)? != 0,
        })
    })?;
    let mut seasons: Vec<SeasonDetail> = Vec::new();
    for row in rows {
        let e = row?;
        match seasons.last_mut() {
            Some(s) if s.season_number == e.season_number => s.episodes.push(e),
            _ => seasons.push(SeasonDetail { season_number: e.season_number, episodes: vec![e] }),
        }
    }
    Ok(ShowDetail { show, seasons })
}

pub fn list_upcoming(conn: &Connection, today: &str) -> Result<Vec<UpcomingItem>> {
    let mut st = conn.prepare(
        "SELECT e.tmdb_id, e.show_tmdb_id, s.name, s.poster_path,
                e.season_number, e.episode_number, e.name, e.air_date
         FROM episodes e JOIN shows s ON s.tmdb_id=e.show_tmdb_id
         WHERE s.my_state='watching' AND e.air_date IS NOT NULL AND e.air_date>?1
         ORDER BY e.air_date, s.name, e.season_number, e.episode_number
         LIMIT 300")?;
    let rows = st.query_map([today], |r| {
        Ok(UpcomingItem {
            episode_id: r.get(0)?, show_tmdb_id: r.get(1)?, show_name: r.get(2)?,
            poster_path: r.get(3)?, season_number: r.get(4)?, episode_number: r.get(5)?,
            episode_name: r.get(6)?, air_date: r.get(7)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

// ---------- movies ----------

pub fn upsert_movie_meta(conn: &Connection, m: &MovieMeta, now: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO movies(tmdb_id,imdb_id,title,year,release_date,runtime,poster_path,backdrop_path,overview,added_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)
         ON CONFLICT(tmdb_id) DO UPDATE SET
           imdb_id=excluded.imdb_id, title=excluded.title, year=excluded.year,
           release_date=excluded.release_date, runtime=excluded.runtime,
           poster_path=excluded.poster_path, backdrop_path=excluded.backdrop_path,
           overview=excluded.overview",
        params![m.tmdb_id, m.imdb_id, m.title, m.year, m.release_date, m.runtime,
                m.poster_path, m.backdrop_path, m.overview, now])?;
    Ok(())
}

pub fn set_movie_user(conn: &Connection, id: i64, state: &str, watched_at: Option<&str>,
                      rewatch: i64, favorite: bool, tvdb_id: Option<i64>, uuid: Option<&str>) -> Result<()> {
    conn.execute(
        "UPDATE movies SET my_state=?2, watched_at=?3, rewatch_count=?4, is_favorite=?5,
           tvdb_id=COALESCE(?6,tvdb_id), tvtime_uuid=COALESCE(?7,tvtime_uuid) WHERE tmdb_id=?1",
        params![id, state, watched_at, rewatch, favorite as i64, tvdb_id, uuid])?;
    Ok(())
}

pub fn set_movie_state(conn: &Connection, id: i64, state: &str, now: &str) -> Result<()> {
    if state == "watched" {
        conn.execute(
            "UPDATE movies SET my_state='watched', watched_at=COALESCE(watched_at,?2) WHERE tmdb_id=?1",
            params![id, now])?;
    } else {
        conn.execute(
            "UPDATE movies SET my_state='watchlist', watched_at=NULL, rewatch_count=0 WHERE tmdb_id=?1",
            [id])?;
    }
    Ok(())
}

pub fn movie_rewatch(conn: &Connection, id: i64) -> Result<i64> {
    conn.execute("UPDATE movies SET rewatch_count=rewatch_count+1 WHERE tmdb_id=?1", [id])?;
    Ok(conn.query_row("SELECT rewatch_count FROM movies WHERE tmdb_id=?1", [id], |r| r.get(0))?)
}

pub fn toggle_movie_favorite(conn: &Connection, id: i64) -> Result<bool> {
    conn.execute("UPDATE movies SET is_favorite=1-is_favorite WHERE tmdb_id=?1", [id])?;
    Ok(conn.query_row("SELECT is_favorite FROM movies WHERE tmdb_id=?1", [id],
        |r| r.get::<_, i64>(0))? != 0)
}

pub fn list_movies(conn: &Connection) -> Result<Vec<MovieRow>> {
    let mut st = conn.prepare(
        "SELECT tmdb_id, imdb_id, title, year, release_date, runtime, poster_path,
                backdrop_path, overview, my_state, watched_at, rewatch_count, is_favorite
         FROM movies ORDER BY title")?;
    let rows = st.query_map([], |r| {
        Ok(MovieRow {
            tmdb_id: r.get(0)?, imdb_id: r.get(1)?, title: r.get(2)?, year: r.get(3)?,
            release_date: r.get(4)?, runtime: r.get(5)?, poster_path: r.get(6)?,
            backdrop_path: r.get(7)?, overview: r.get(8)?, my_state: r.get(9)?,
            watched_at: r.get(10)?, rewatch_count: r.get(11)?,
            is_favorite: r.get::<_, i64>(12)? != 0,
        })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

// ---------- refresh / search support ----------

pub fn shows_to_refresh(conn: &Connection) -> Result<Vec<i64>> {
    let mut st = conn.prepare(
        "SELECT tmdb_id FROM shows
         WHERE my_state='watching'
           AND (tmdb_status IS NULL OR tmdb_status NOT IN ('Ended','Canceled'))
         ORDER BY tmdb_id")?;
    let rows = st.query_map([], |r| r.get(0))?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

pub fn library_ids(conn: &Connection) -> Result<(HashSet<i64>, HashSet<i64>)> {
    let mut shows = HashSet::new();
    let mut st = conn.prepare("SELECT tmdb_id FROM shows")?;
    for id in st.query_map([], |r| r.get::<_, i64>(0))? {
        shows.insert(id?);
    }
    let mut movies = HashSet::new();
    let mut st = conn.prepare("SELECT tmdb_id FROM movies")?;
    for id in st.query_map([], |r| r.get::<_, i64>(0))? {
        movies.insert(id?);
    }
    Ok((shows, movies))
}
```

Add `pub mod queries;` to `src-tauri/src/lib.rs`.

- [ ] **Step 4: Run tests to verify they pass**

From `src-tauri/`: `cargo test queries::` — Expected: 8 passed.

- [ ] **Step 5: Commit**

```
git add -A
git commit -m "feat: all library SQL (shows, episodes, watches, movies, upcoming)"
```

---

### Task 7: Import engine

**Files:**
- Modify: `src-tauri/src/import.rs` (append engine below the pure functions from Task 5)

**Interfaces:**
- Consumes: `Tmdb` trait, `queries`, `export::TvTimeExport`, `models::{ImportProgress, ImportReport, …}`
- Produces:
  - `import::run_import<T: Tmdb + ?Sized>(tmdb: &T, db: Arc<Mutex<Connection>>, export: TvTimeExport, progress: impl FnMut(ImportProgress)) -> ImportReport` (async; never returns Err — failures go in the report)

- [ ] **Step 1: Write failing tests (FakeTmdb)**

Append to `src-tauri/src/import.rs` tests module (extend the existing `#[cfg(test)] mod tests`):
```rust
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;

    use crate::db;
    use crate::error::{AppError, Result as AResult};
    use crate::export::{ExportMovie, ExportShow, TvTimeExport};
    use crate::queries;
    use crate::tmdb::{MovieMeta, SearchResultItem, ShowFull, ShowMeta, Tmdb};

    struct FakeTmdb {
        tv_by_tvdb: HashMap<i64, i64>,
        movie_by_imdb: HashMap<String, i64>,
        shows: HashMap<i64, ShowFull>,
        movies: HashMap<i64, MovieMeta>,
    }

    #[async_trait]
    impl Tmdb for FakeTmdb {
        async fn find_show(&self, tvdb_id: i64) -> AResult<Option<i64>> {
            Ok(self.tv_by_tvdb.get(&tvdb_id).copied())
        }
        async fn find_movie(&self, imdb_id: &str) -> AResult<Option<i64>> {
            Ok(self.movie_by_imdb.get(imdb_id).copied())
        }
        async fn show_with_episodes(&self, tmdb_id: i64) -> AResult<ShowFull> {
            self.shows.get(&tmdb_id).cloned().ok_or_else(|| AppError::msg("fake: no show"))
        }
        async fn movie(&self, tmdb_id: i64) -> AResult<MovieMeta> {
            self.movies.get(&tmdb_id).cloned().ok_or_else(|| AppError::msg("fake: no movie"))
        }
        async fn search(&self, _q: &str) -> AResult<Vec<SearchResultItem>> {
            Ok(Vec::new())
        }
        async fn validate(&self) -> AResult<bool> {
            Ok(true)
        }
    }

    fn fake() -> FakeTmdb {
        let meta = |id: i64, name: &str| ShowMeta {
            tmdb_id: id, name: name.into(), year: Some(2020), poster_path: None,
            backdrop_path: None, overview: None, status: Some("Ended".into()),
            season_numbers: vec![1],
        };
        let mut shows = HashMap::new();
        // show 100: 3 aired episodes; export says watched=2 with S1E3 unwatched -> exact match
        shows.insert(100, ShowFull {
            meta: meta(100, "Exact"),
            episodes: vec![
                ep(1000, 1, 1, Some("2020-01-01")),
                ep(1001, 1, 2, Some("2020-01-08")),
                ep(1002, 1, 3, Some("2020-01-15")),
            ],
        });
        // show 200: 2 aired episodes; export claims watched=5 -> anomaly
        shows.insert(200, ShowFull {
            meta: meta(200, "Drifted"),
            episodes: vec![ep(2000, 1, 1, Some("2020-01-01")), ep(2001, 1, 2, Some("2020-01-08"))],
        });
        let movie = |id: i64, title: &str| MovieMeta {
            tmdb_id: id, imdb_id: None, title: title.into(), year: Some(2017),
            release_date: Some("2017-05-01".into()), runtime: Some(120),
            poster_path: None, backdrop_path: None, overview: None,
        };
        FakeTmdb {
            tv_by_tvdb: HashMap::from([(81189, 100), (99999, 200)]),
            movie_by_imdb: HashMap::from([("tt1".into(), 500), ("tt2".into(), 501)]),
            shows,
            movies: HashMap::from([(500, movie(500, "Seen")), (501, movie(501, "Unseen"))]),
        }
    }

    fn export_show(title: &str, tvdb: i64, watched: i64, total: i64,
                   unwatched: Vec<SeasonEp>, specials: i64) -> ExportShow {
        ExportShow {
            title: title.into(), tvdb_id: tvdb, imdb_id: None, status: "up_to_date".into(),
            is_favorite: false, watched, total, total_specials: 0,
            watched_specials: specials, unwatched_regular_eps: unwatched, watched_no_date: 0,
        }
    }

    fn export_movie(title: &str, imdb: &str, watched: bool) -> ExportMovie {
        ExportMovie {
            title: title.into(), tvdb_id: 1, imdb_id: Some(imdb.into()),
            uuid: "u".into(), year: Some(2017), is_watched: watched, is_favorite: watched,
            watched_at: watched.then(|| "2024-06-09T15:05:22Z".to_string()),
            rewatch_count: if watched { 1 } else { 0 },
        }
    }

    fn test_export() -> TvTimeExport {
        TvTimeExport {
            export_date: cutoff(),
            shows: vec![
                export_show("Exact", 81189, 2, 3, vec![SeasonEp { season: 1, episode: 3 }], 2),
                export_show("Drifted", 99999, 5, 5, vec![], 0),
                export_show("Missing", 11111, 1, 1, vec![], 0), // no TMDB match -> failure
            ],
            movies: vec![
                export_movie("Seen", "tt1", true),
                export_movie("Unseen", "tt2", false),
                export_movie("Ghost", "tt404", true), // no TMDB match -> failure
            ],
        }
    }

    #[tokio::test]
    async fn import_end_to_end() {
        let db = Arc::new(Mutex::new(db::open_memory().unwrap()));
        let tmdb = fake();
        let mut events = 0usize;
        let report = run_import(&tmdb, db.clone(), test_export(), |_p| events += 1).await;

        assert_eq!(report.shows_ok, 2);
        assert_eq!(report.movies_ok, 2);
        assert_eq!(report.failures.len(), 2);
        assert!(report.failures.iter().any(|f| f.kind == "show" && f.title == "Missing"));
        assert!(report.failures.iter().any(|f| f.kind == "movie" && f.title == "Ghost"));
        assert_eq!(report.anomalies.len(), 1);
        assert_eq!(report.anomalies[0].title, "Drifted");
        assert_eq!(report.anomalies[0].expected, 5);
        assert_eq!(report.anomalies[0].actual, 2);
        assert_eq!(report.specials_note.len(), 1);
        assert_eq!(report.specials_note[0].title, "Exact");
        assert!(events >= 6); // one per item minimum

        let conn = db.lock().unwrap();
        // Exact: S1E1+S1E2 watched (import, no date), S1E3 not
        assert_eq!(queries::watched_regular_count(&conn, 100).unwrap(), 2);
        let d = queries::show_detail(&conn, 100).unwrap();
        assert!(d.seasons[0].episodes[0].watched && d.seasons[0].episodes[0].watched_at.is_none());
        assert_eq!(d.show.my_state, "watching");
        // movies
        let movies = queries::list_movies(&conn).unwrap();
        assert_eq!(movies.len(), 2);
        let seen = movies.iter().find(|m| m.title == "Seen").unwrap();
        assert_eq!(seen.my_state, "watched");
        assert_eq!(seen.watched_at.as_deref(), Some("2024-06-09T15:05:22Z"));
        assert_eq!(seen.rewatch_count, 1);
        assert!(seen.is_favorite);
        assert_eq!(movies.iter().find(|m| m.title == "Unseen").unwrap().my_state, "watchlist");
    }

    #[tokio::test]
    async fn import_is_idempotent_and_preserves_app_watches() {
        let db = Arc::new(Mutex::new(db::open_memory().unwrap()));
        let tmdb = fake();
        let _ = run_import(&tmdb, db.clone(), test_export(), |_| {}).await;
        {
            let conn = db.lock().unwrap();
            // user marks S1E3 in-app after first import
            queries::set_episode_watched(&conn, 1002, true, "2026-07-02T10:00:00Z").unwrap();
        }
        let report2 = run_import(&tmdb, db.clone(), test_export(), |_| {}).await;
        assert_eq!(report2.shows_ok, 2);
        let conn = db.lock().unwrap();
        assert_eq!(queries::watched_regular_count(&conn, 100).unwrap(), 3);
        let d = queries::show_detail(&conn, 100).unwrap();
        // the app-sourced watch kept its date
        assert_eq!(d.seasons[0].episodes[2].watched_at.as_deref(), Some("2026-07-02T10:00:00Z"));
        // no duplicate movies
        assert_eq!(queries::list_movies(&conn).unwrap().len(), 2);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

From `src-tauri/`: `cargo test import::` — Expected: FAIL to compile (`run_import` not defined).

- [ ] **Step 3: Implement the engine**

Append to `src-tauri/src/import.rs` (above the tests):
```rust
use std::sync::{Arc, Mutex};

use rusqlite::Connection;

use crate::error::{AppError, Result};
use crate::export::{ExportMovie, ExportShow, TvTimeExport};
use crate::models::*;
use crate::queries;
use crate::tmdb::Tmdb;

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// Run a DB closure on a blocking thread (never blocks the async runtime).
pub async fn with_db<R: Send + 'static>(
    db: &Arc<Mutex<Connection>>,
    f: impl FnOnce(&Connection) -> Result<R> + Send + 'static,
) -> Result<R> {
    let db = db.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let conn = db.lock().map_err(|_| AppError::msg("db lock poisoned"))?;
        f(&conn)
    })
    .await
    .map_err(|e| AppError::msg(format!("join: {e}")))?
}

async fn import_one_show<T: Tmdb + ?Sized>(
    tmdb: &T,
    db: &Arc<Mutex<Connection>>,
    s: &ExportShow,
    cutoff: chrono::NaiveDate,
) -> Result<Option<ImportAnomaly>> {
    let tmdb_id = tmdb.find_show(s.tvdb_id).await?
        .ok_or_else(|| AppError::msg(format!("TVDB id {} not found on TMDB", s.tvdb_id)))?;
    let full = tmdb.show_with_episodes(tmdb_id).await?;
    let mark = episodes_to_mark(&full.episodes, &s.unwatched_regular_eps, cutoff);
    let actual = mark.len() as i64;
    let expected = s.watched;
    let state = map_status(&s.status).to_string();
    let (favorite, tvdb_id, imdb_id) = (s.is_favorite, s.tvdb_id, s.imdb_id.clone());
    let title = s.title.clone();
    with_db(db, move |conn| {
        let now = now_rfc3339();
        queries::upsert_show_meta(conn, &full.meta, &now)?;
        queries::set_show_user(conn, full.meta.tmdb_id, &state, favorite, Some(tvdb_id), imdb_id.as_deref())?;
        queries::upsert_episodes(conn, full.meta.tmdb_id, &full.episodes)?;
        queries::mark_episodes_import(conn, &mark)?;
        Ok(())
    })
    .await?;
    Ok((actual != expected).then_some(ImportAnomaly { title, tmdb_id, expected, actual }))
}

async fn import_one_movie<T: Tmdb + ?Sized>(
    tmdb: &T,
    db: &Arc<Mutex<Connection>>,
    m: &ExportMovie,
) -> Result<()> {
    let imdb = m.imdb_id.as_deref()
        .ok_or_else(|| AppError::msg("movie has no IMDb id in export"))?;
    let tmdb_id = tmdb.find_movie(imdb).await?
        .ok_or_else(|| AppError::msg(format!("IMDb id {imdb} not found on TMDB")))?;
    let meta = tmdb.movie(tmdb_id).await?;
    let state = if m.is_watched { "watched" } else { "watchlist" };
    let (watched_at, rewatch, favorite) = (m.watched_at.clone(), m.rewatch_count, m.is_favorite);
    let (tvdb_id, uuid) = (m.tvdb_id, m.uuid.clone());
    with_db(db, move |conn| {
        let now = now_rfc3339();
        queries::upsert_movie_meta(conn, &meta, &now)?;
        queries::set_movie_user(conn, meta.tmdb_id, state, watched_at.as_deref(),
                                rewatch, favorite, Some(tvdb_id), Some(&uuid))?;
        Ok(())
    })
    .await
}

/// Serial import; per-item failures land in the report, never abort the run.
pub async fn run_import<T: Tmdb + ?Sized>(
    tmdb: &T,
    db: Arc<Mutex<Connection>>,
    export: TvTimeExport,
    mut progress: impl FnMut(ImportProgress),
) -> ImportReport {
    let mut report = ImportReport::default();
    let total = export.shows.len();
    for (i, s) in export.shows.iter().enumerate() {
        progress(ImportProgress {
            stage: "shows".into(), done: i, total, label: s.title.clone(),
        });
        match import_one_show(tmdb, &db, s, export.export_date).await {
            Ok(anomaly) => {
                report.shows_ok += 1;
                report.anomalies.extend(anomaly);
                if s.watched_specials > 0 {
                    report.specials_note.push(SpecialsNote {
                        title: s.title.clone(),
                        watched_specials: s.watched_specials,
                    });
                }
            }
            Err(e) => report.failures.push(ImportFailure {
                kind: "show".into(), title: s.title.clone(), reason: e.to_string(),
            }),
        }
    }
    let total = export.movies.len();
    for (i, m) in export.movies.iter().enumerate() {
        progress(ImportProgress {
            stage: "movies".into(), done: i, total, label: m.title.clone(),
        });
        match import_one_movie(tmdb, &db, m).await {
            Ok(()) => report.movies_ok += 1,
            Err(e) => report.failures.push(ImportFailure {
                kind: "movie".into(), title: m.title.clone(), reason: e.to_string(),
            }),
        }
    }
    report.finished_at = now_rfc3339();
    report
}
```

Note: `tauri::async_runtime::spawn_blocking` needs no extra features. The tests use `#[tokio::test]`, which runs outside Tauri's runtime — `tauri::async_runtime` falls back to its own global runtime in that case; if a test panics with "no tauri runtime", replace `tauri::async_runtime::spawn_blocking` with `tokio::task::spawn_blocking` inside `with_db` (both are fine at app runtime because Tauri's async runtime IS tokio).

- [ ] **Step 4: Run tests to verify they pass**

From `src-tauri/`: `cargo test import::` — Expected: 5 passed (3 pure + 2 engine).

- [ ] **Step 5: Commit**

```
git add -A
git commit -m "feat: import engine with report, anomalies, idempotent re-import"
```

---

### Task 8: Refresh engine

**Files:**
- Create: `src-tauri/src/refresh.rs`
- Modify: `src-tauri/src/lib.rs` (add `pub mod refresh;`)

**Interfaces:**
- Consumes: `Tmdb`, `queries`, `import::with_db`
- Produces: `refresh::run_refresh<T: Tmdb + ?Sized>(tmdb: &T, db: Arc<Mutex<Connection>>) -> models::RefreshSummary`

- [ ] **Step 1: Write failing test**

`src-tauri/src/refresh.rs`:
```rust
use std::sync::{Arc, Mutex};

use rusqlite::Connection;

use crate::import::with_db;
use crate::models::RefreshSummary;
use crate::queries;
use crate::tmdb::Tmdb;

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// Re-fetch metadata + episodes for every watching, still-airing show.
pub async fn run_refresh<T: Tmdb + ?Sized>(
    tmdb: &T,
    db: Arc<Mutex<Connection>>,
) -> RefreshSummary {
    let ids = match with_db(&db, |conn| queries::shows_to_refresh(conn)).await {
        Ok(ids) => ids,
        Err(e) => return RefreshSummary { refreshed: 0, failed: vec![e.to_string()] },
    };
    let mut summary = RefreshSummary { refreshed: 0, failed: Vec::new() };
    for id in ids {
        match tmdb.show_with_episodes(id).await {
            Ok(full) => {
                let res = with_db(&db, move |conn| {
                    let now = now_rfc3339();
                    queries::upsert_show_meta(conn, &full.meta, &now)?;
                    queries::upsert_episodes(conn, full.meta.tmdb_id, &full.episodes)?;
                    Ok(())
                })
                .await;
                match res {
                    Ok(()) => summary.refreshed += 1,
                    Err(e) => summary.failed.push(format!("show {id}: {e}")),
                }
            }
            Err(e) => summary.failed.push(format!("show {id}: {e}")),
        }
    }
    summary
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    use async_trait::async_trait;

    use crate::db;
    use crate::error::{AppError, Result as AResult};
    use crate::tmdb::{EpisodeMeta, MovieMeta, SearchResultItem, ShowFull, ShowMeta};

    struct OneShow(ShowFull);

    #[async_trait]
    impl Tmdb for OneShow {
        async fn find_show(&self, _t: i64) -> AResult<Option<i64>> { Ok(None) }
        async fn find_movie(&self, _i: &str) -> AResult<Option<i64>> { Ok(None) }
        async fn show_with_episodes(&self, tmdb_id: i64) -> AResult<ShowFull> {
            if tmdb_id == self.0.meta.tmdb_id { Ok(self.0.clone()) }
            else { Err(AppError::msg("boom")) }
        }
        async fn movie(&self, _t: i64) -> AResult<MovieMeta> { Err(AppError::msg("no")) }
        async fn search(&self, _q: &str) -> AResult<Vec<SearchResultItem>> { Ok(Vec::new()) }
        async fn validate(&self) -> AResult<bool> { Ok(true) }
    }

    fn meta(id: i64, status: &str) -> ShowMeta {
        ShowMeta {
            tmdb_id: id, name: format!("S{id}"), year: None, poster_path: None,
            backdrop_path: None, overview: None, status: Some(status.into()),
            season_numbers: vec![1],
        }
    }

    fn ep(id: i64, num: i64, air: &str) -> EpisodeMeta {
        EpisodeMeta {
            tmdb_id: id, season_number: 1, episode_number: num, name: None,
            air_date: chrono::NaiveDate::parse_from_str(air, "%Y-%m-%d").ok(),
            runtime: None, still_path: None, overview: None,
        }
    }

    #[tokio::test]
    async fn refreshes_airing_adds_new_episode_and_reports_failures() {
        let db = Arc::new(Mutex::new(db::open_memory().unwrap()));
        {
            let conn = db.lock().unwrap();
            // airing show 1 with one episode, watching
            queries::upsert_show_meta(&conn, &meta(1, "Returning Series"), "t").unwrap();
            queries::set_show_user(&conn, 1, "watching", false, None, None).unwrap();
            queries::upsert_episodes(&conn, 1, &[ep(10, 1, "2026-06-01")]).unwrap();
            // airing show 2, watching, but the fake API errors on it
            queries::upsert_show_meta(&conn, &meta(2, "Returning Series"), "t").unwrap();
            queries::set_show_user(&conn, 2, "watching", false, None, None).unwrap();
            // ended show 3 must not be touched
            queries::upsert_show_meta(&conn, &meta(3, "Ended"), "t").unwrap();
            queries::set_show_user(&conn, 3, "watching", false, None, None).unwrap();
        }
        // fake returns show 1 with a NEW episode 11
        let tmdb = OneShow(ShowFull {
            meta: meta(1, "Returning Series"),
            episodes: vec![ep(10, 1, "2026-06-01"), ep(11, 2, "2026-07-20")],
        });
        let summary = run_refresh(&tmdb, db.clone()).await;
        assert_eq!(summary.refreshed, 1);
        assert_eq!(summary.failed.len(), 1);
        assert!(summary.failed[0].starts_with("show 2"));
        let conn = db.lock().unwrap();
        let up = queries::list_upcoming(&conn, "2026-07-02").unwrap();
        assert_eq!(up.len(), 1);
        assert_eq!(up[0].episode_id, 11);
    }
}
```

- [ ] **Step 2: Run tests**

From `src-tauri/`: first confirm the red state by running before creating the file's impl half (compile error), then with the code above: `cargo test refresh::` — Expected: 1 passed. Add `pub mod refresh;` to `lib.rs`.

- [ ] **Step 3: Commit**

```
git add -A
git commit -m "feat: refresh engine for airing shows"
```

---

### Task 9: Image cache + poster:// protocol

**Files:**
- Create: `src-tauri/src/images.rs`
- Modify: `src-tauri/src/lib.rs` (add `pub mod images;`)

**Interfaces:**
- Consumes: `AppState.http` (Task 10 defines `AppState`; until then the fetch fn takes `reqwest::Client` directly)
- Produces:
  - `images::valid_rel(rel: &str) -> bool` — accepts only `w<digits>/<name>.<ext>` or `original/<name>.<ext>`, rejects traversal
  - `images::fetch_cached(root: &Path, http: &reqwest::Client, rel: &str) -> Result<Vec<u8>>` (async)
  - `images::serve<R: tauri::Runtime>(app: tauri::AppHandle<R>, request: tauri::http::Request<Vec<u8>>, responder: tauri::UriSchemeResponder)` (async fn; registered in Task 10)
  - `images::dir_size(root: &Path) -> u64`

- [ ] **Step 1: Write failing tests for path validation**

Bottom of `src-tauri/src/images.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_rel_paths() {
        assert!(valid_rel("w342/abc.jpg"));
        assert!(valid_rel("original/x-1_2.png"));
        assert!(!valid_rel("w342/../secret.txt"));
        assert!(!valid_rel("../etc/passwd"));
        assert!(!valid_rel("w342/a/b.jpg"));
        assert!(!valid_rel("c:/windows/x.jpg"));
        assert!(!valid_rel("w342/noext"));
        assert!(!valid_rel(""));
    }
}
```

- [ ] **Step 2: Run to verify fail, then implement**

`cargo test images::` fails to compile; then implement above the tests:
```rust
use std::path::{Path, PathBuf};

use crate::error::{AppError, Result};

/// Only `w<digits>/<file>.<ext>` or `original/<file>.<ext>`; file is a single
/// path segment of [A-Za-z0-9._-] with an extension. Blocks traversal.
pub fn valid_rel(rel: &str) -> bool {
    let Some((size, file)) = rel.split_once('/') else { return false; };
    let size_ok = size == "original"
        || (size.len() > 1 && size.starts_with('w') && size[1..].chars().all(|c| c.is_ascii_digit()));
    let file_ok = !file.is_empty()
        && file.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        && !file.contains("..")
        && file.rsplit_once('.').map(|(stem, ext)| !stem.is_empty() && !ext.is_empty()).unwrap_or(false);
    size_ok && file_ok
}

fn content_type(rel: &str) -> &'static str {
    match rel.rsplit_once('.').map(|(_, e)| e) {
        Some("png") => "image/png",
        Some("svg") => "image/svg+xml",
        Some("webp") => "image/webp",
        _ => "image/jpeg",
    }
}

/// Return the image bytes, downloading into `root/<rel>` on first use.
pub async fn fetch_cached(root: &Path, http: &reqwest::Client, rel: &str) -> Result<Vec<u8>> {
    if !valid_rel(rel) {
        return Err(AppError::msg(format!("bad image path {rel:?}")));
    }
    let file: PathBuf = root.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
    if let Ok(bytes) = tokio::fs::read(&file).await {
        return Ok(bytes);
    }
    let url = format!("https://image.tmdb.org/t/p/{rel}");
    let resp = http.get(&url).send().await?.error_for_status()?;
    let bytes = resp.bytes().await?.to_vec();
    if let Some(dir) = file.parent() {
        tokio::fs::create_dir_all(dir).await?;
    }
    tokio::fs::write(&file, &bytes).await?;
    Ok(bytes)
}

/// poster:// protocol handler body (registered in lib.rs).
pub async fn serve<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    request: tauri::http::Request<Vec<u8>>,
    responder: tauri::UriSchemeResponder,
) {
    use tauri::Manager;
    let rel = request.uri().path().trim_start_matches('/').to_string();
    let root = app.path().app_data_dir().map(|d| d.join("images"));
    let result = match root {
        Ok(root) => {
            let http = app.state::<crate::AppState>().http.clone();
            fetch_cached(&root, &http, &rel).await
        }
        Err(e) => Err(AppError::Msg(e.to_string())),
    };
    let resp = match result {
        Ok(bytes) => tauri::http::Response::builder()
            .status(200)
            .header("Content-Type", content_type(&rel))
            .header("Cache-Control", "max-age=31536000")
            .body(bytes)
            .unwrap(),
        Err(_) => tauri::http::Response::builder().status(404).body(Vec::new()).unwrap(),
    };
    responder.respond(resp);
}

/// Total bytes under the cache dir (for the Settings screen).
pub fn dir_size(root: &Path) -> u64 {
    fn walk(dir: &Path, acc: &mut u64) {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for e in entries.flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(&p, acc);
                } else if let Ok(md) = e.metadata() {
                    *acc += md.len();
                }
            }
        }
    }
    let mut acc = 0;
    walk(root, &mut acc);
    acc
}
```

Add `pub mod images;` to `lib.rs`. NOTE: `images::serve` references `crate::AppState`, which Task 10 defines — if implementing this task in isolation, add a temporary `pub struct AppState { pub http: reqwest::Client }` placeholder in `lib.rs` (Task 10 replaces it with the full struct; the field name `http` is stable).

Run: `cargo test images::` — Expected: 1 passed. `cargo check` — Expected: clean.

- [ ] **Step 3: Commit**

```
git add -A
git commit -m "feat: disk image cache and poster protocol handler"
```

---

### Task 10: Commands + full Tauri wiring

**Files:**
- Create: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/lib.rs` (full rewrite of `run()`, final `AppState`)

**Interfaces:**
- Consumes: everything produced by Tasks 2–9
- Produces (the complete IPC surface; the frontend `api.ts` in Task 11 mirrors it 1:1):
  - `settings_all() -> HashMap<String,String>`, `settings_set(key, value)`
  - `tmdb_validate_key(key) -> bool` (stores key in settings on success)
  - `import_run(path)` — spawns background job; events `import:progress` (ImportProgress) and `import:done` (ImportReport); errors if busy/no key
  - `import_last_report() -> Option<ImportReport>`
  - `refresh_run() -> RefreshSummary` (awaited inline; errors if busy or no key)
  - `shows_list() -> Vec<ShowCard>`, `show_detail(id) -> ShowDetail`
  - `show_set_state(id, my_state)`, `show_toggle_favorite(id) -> bool`
  - `episode_mark(id, watched)`, `season_mark(show_id, season, watched)`, `show_mark_all(id)`
  - `show_mark_next(id) -> ShowCard`
  - `upcoming_list() -> Vec<UpcomingItem>`, `movies_list() -> Vec<MovieRow>`
  - `movie_set_state(id, my_state)`, `movie_rewatch(id) -> i64`, `movie_toggle_favorite(id) -> bool`
  - `search_tmdb(query) -> Vec<SearchItem>`
  - `show_add(tmdb_id, my_state)`, `movie_add(tmdb_id, my_state)`
  - `cache_info() -> CacheInfo`
  - `AppState { db: Arc<Mutex<Connection>>, http: reqwest::Client, busy: Arc<AtomicBool> }`

- [ ] **Step 1: Implement commands.rs**

```rust
use std::collections::HashMap;
use std::sync::atomic::Ordering;

use tauri::{AppHandle, Emitter, Manager, State};

use crate::import::with_db;
use crate::models::*;
use crate::tmdb::{Tmdb, TmdbClient};
use crate::{export, images, import, queries, refresh, AppState};

type CmdResult<T> = std::result::Result<T, String>;

fn today_local() -> String {
    chrono::Local::now().date_naive().to_string()
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// Build a TMDB client from the stored key, or explain what's missing.
async fn client(state: &State<'_, AppState>) -> CmdResult<TmdbClient> {
    let key = with_db(&state.db, |c| queries::setting(c, "tmdb_key")).await
        .map_err(|e| e.to_string())?
        .ok_or("TMDB API key not set — add it in Settings")?;
    Ok(TmdbClient::new(state.http.clone(), key))
}

struct BusyGuard(std::sync::Arc<std::sync::atomic::AtomicBool>);

impl BusyGuard {
    fn acquire(state: &AppState) -> CmdResult<Self> {
        if state.busy.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_err() {
            return Err("another job is already running".into());
        }
        Ok(BusyGuard(state.busy.clone()))
    }
}

impl Drop for BusyGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

// ---------- settings ----------

#[tauri::command]
pub async fn settings_all(state: State<'_, AppState>) -> CmdResult<HashMap<String, String>> {
    with_db(&state.db, |c| {
        let mut st = c.prepare("SELECT key, value FROM settings")?;
        let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<std::result::Result<HashMap<_, _>, _>>()?)
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn settings_set(state: State<'_, AppState>, key: String, value: String) -> CmdResult<()> {
    with_db(&state.db, move |c| queries::set_setting(c, &key, &value)).await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn tmdb_validate_key(state: State<'_, AppState>, key: String) -> CmdResult<bool> {
    let c = TmdbClient::new(state.http.clone(), key.clone());
    let ok = c.validate().await.map_err(|e| e.to_string())?;
    if ok {
        with_db(&state.db, move |c| queries::set_setting(c, "tmdb_key", &key)).await
            .map_err(|e| e.to_string())?;
    }
    Ok(ok)
}

// ---------- import ----------

#[tauri::command]
pub async fn import_run(app: AppHandle, state: State<'_, AppState>, path: String) -> CmdResult<()> {
    let guard = BusyGuard::acquire(&state)?;
    let tmdb = client(&state).await?;
    let html = tokio::fs::read_to_string(&path).await.map_err(|e| e.to_string())?;
    let parsed = export::parse_export(&html).map_err(|e| e.to_string())?;
    let db = state.db.clone();
    tauri::async_runtime::spawn(async move {
        let _guard = guard; // released (and busy cleared) when the job ends, even on panic-free error paths
        let emitter = app.clone();
        let report = import::run_import(&tmdb, db.clone(), parsed, move |p| {
            let _ = emitter.emit("import:progress", &p);
        })
        .await;
        if let Ok(json) = serde_json::to_string(&report) {
            let _ = with_db(&db, move |c| queries::set_setting(c, "last_import_report", &json)).await;
        }
        let _ = app.emit("import:done", &report);
    });
    Ok(())
}

#[tauri::command]
pub async fn import_last_report(state: State<'_, AppState>) -> CmdResult<Option<ImportReport>> {
    let json = with_db(&state.db, |c| queries::setting(c, "last_import_report")).await
        .map_err(|e| e.to_string())?;
    Ok(json.and_then(|j| serde_json::from_str(&j).ok()))
}

// ---------- refresh ----------

#[tauri::command]
pub async fn refresh_run(state: State<'_, AppState>) -> CmdResult<RefreshSummary> {
    let _guard = BusyGuard::acquire(&state)?;
    let tmdb = client(&state).await?;
    Ok(refresh::run_refresh(&tmdb, state.db.clone()).await)
}

// ---------- shows ----------

#[tauri::command]
pub async fn shows_list(state: State<'_, AppState>) -> CmdResult<Vec<ShowCard>> {
    with_db(&state.db, |c| queries::list_show_cards(c, &today_local())).await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn show_detail(state: State<'_, AppState>, id: i64) -> CmdResult<ShowDetail> {
    with_db(&state.db, move |c| queries::show_detail(c, id)).await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn show_set_state(state: State<'_, AppState>, id: i64, my_state: String) -> CmdResult<()> {
    with_db(&state.db, move |c| queries::set_show_state(c, id, &my_state)).await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn show_toggle_favorite(state: State<'_, AppState>, id: i64) -> CmdResult<bool> {
    with_db(&state.db, move |c| queries::toggle_show_favorite(c, id)).await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn episode_mark(state: State<'_, AppState>, id: i64, watched: bool) -> CmdResult<()> {
    with_db(&state.db, move |c| queries::set_episode_watched(c, id, watched, &now_rfc3339())).await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn season_mark(state: State<'_, AppState>, show_id: i64, season: i64, watched: bool) -> CmdResult<()> {
    with_db(&state.db, move |c| {
        queries::mark_season(c, show_id, season, watched, &today_local(), &now_rfc3339())
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn show_mark_all(state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    with_db(&state.db, move |c| queries::mark_all_aired(c, id, &today_local(), &now_rfc3339())).await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn show_mark_next(state: State<'_, AppState>, id: i64) -> CmdResult<ShowCard> {
    with_db(&state.db, move |c| {
        let today = today_local();
        let card = queries::show_card(c, id, &today)?;
        if let Some(next) = card.next {
            queries::set_episode_watched(c, next.episode_id, true, &now_rfc3339())?;
        }
        queries::show_card(c, id, &today)
    })
    .await
    .map_err(|e| e.to_string())
}

// ---------- upcoming / movies ----------

#[tauri::command]
pub async fn upcoming_list(state: State<'_, AppState>) -> CmdResult<Vec<UpcomingItem>> {
    with_db(&state.db, |c| queries::list_upcoming(c, &today_local())).await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn movies_list(state: State<'_, AppState>) -> CmdResult<Vec<MovieRow>> {
    with_db(&state.db, |c| queries::list_movies(c)).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn movie_set_state(state: State<'_, AppState>, id: i64, my_state: String) -> CmdResult<()> {
    with_db(&state.db, move |c| queries::set_movie_state(c, id, &my_state, &now_rfc3339())).await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn movie_rewatch(state: State<'_, AppState>, id: i64) -> CmdResult<i64> {
    with_db(&state.db, move |c| queries::movie_rewatch(c, id)).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn movie_toggle_favorite(state: State<'_, AppState>, id: i64) -> CmdResult<bool> {
    with_db(&state.db, move |c| queries::toggle_movie_favorite(c, id)).await
        .map_err(|e| e.to_string())
}

// ---------- search / add ----------

#[tauri::command]
pub async fn search_tmdb(state: State<'_, AppState>, query: String) -> CmdResult<Vec<SearchItem>> {
    let tmdb = client(&state).await?;
    let results = tmdb.search(&query).await.map_err(|e| e.to_string())?;
    let (show_ids, movie_ids) = with_db(&state.db, |c| queries::library_ids(c)).await
        .map_err(|e| e.to_string())?;
    Ok(results.into_iter().map(|r| {
        let in_library = match r.media_type.as_str() {
            "tv" => show_ids.contains(&r.tmdb_id),
            _ => movie_ids.contains(&r.tmdb_id),
        };
        SearchItem {
            tmdb_id: r.tmdb_id, media_type: r.media_type, title: r.title,
            year: r.year, poster_path: r.poster_path, overview: r.overview, in_library,
        }
    }).collect())
}

#[tauri::command]
pub async fn show_add(state: State<'_, AppState>, tmdb_id: i64, my_state: String) -> CmdResult<()> {
    let tmdb = client(&state).await?;
    let full = tmdb.show_with_episodes(tmdb_id).await.map_err(|e| e.to_string())?;
    with_db(&state.db, move |c| {
        queries::upsert_show_meta(c, &full.meta, &now_rfc3339())?;
        queries::set_show_user(c, full.meta.tmdb_id, &my_state, false, None, None)?;
        queries::upsert_episodes(c, full.meta.tmdb_id, &full.episodes)?;
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn movie_add(state: State<'_, AppState>, tmdb_id: i64, my_state: String) -> CmdResult<()> {
    let tmdb = client(&state).await?;
    let meta = tmdb.movie(tmdb_id).await.map_err(|e| e.to_string())?;
    with_db(&state.db, move |c| {
        let now = now_rfc3339();
        queries::upsert_movie_meta(c, &meta, &now)?;
        let watched_at = (my_state == "watched").then(|| now.clone());
        queries::set_movie_user(c, meta.tmdb_id, &my_state, watched_at.as_deref(), 0, false, None, None)?;
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())
}

// ---------- misc ----------

#[tauri::command]
pub async fn cache_info(app: AppHandle) -> CmdResult<CacheInfo> {
    let root = app.path().app_data_dir().map_err(|e| e.to_string())?.join("images");
    let path = root.display().to_string();
    let bytes = tauri::async_runtime::spawn_blocking(move || images::dir_size(&root)).await
        .map_err(|e| e.to_string())?;
    Ok(CacheInfo { path, bytes })
}
```

- [ ] **Step 2: Rewrite lib.rs with full wiring**

`src-tauri/src/lib.rs`:
```rust
pub mod commands;
pub mod db;
pub mod error;
pub mod export;
pub mod images;
pub mod import;
pub mod models;
pub mod queries;
pub mod refresh;
pub mod tmdb;

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use tauri::Manager;

pub struct AppState {
    pub db: Arc<Mutex<rusqlite::Connection>>,
    pub http: reqwest::Client,
    pub busy: Arc<AtomicBool>,
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .register_asynchronous_uri_scheme_protocol("poster", |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            tauri::async_runtime::spawn(async move {
                images::serve(app, request, responder).await;
            });
        })
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            let conn = db::open(&data_dir.join("showtrack.db"))?;
            app.manage(AppState {
                db: Arc::new(Mutex::new(conn)),
                http: reqwest::Client::new(),
                busy: Arc::new(AtomicBool::new(false)),
            });
            // safety net: show the window even if page load never fires
            if let Some(w) = app.get_webview_window("main") {
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_secs(3));
                    let _ = w.show();
                });
            }
            Ok(())
        })
        .on_page_load(|webview, payload| {
            if matches!(payload.event(), tauri::webview::PageLoadEvent::Finished) {
                let _ = webview.window().show();
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::settings_all,
            commands::settings_set,
            commands::tmdb_validate_key,
            commands::import_run,
            commands::import_last_report,
            commands::refresh_run,
            commands::shows_list,
            commands::show_detail,
            commands::show_set_state,
            commands::show_toggle_favorite,
            commands::episode_mark,
            commands::season_mark,
            commands::show_mark_all,
            commands::show_mark_next,
            commands::upcoming_list,
            commands::movies_list,
            commands::movie_set_state,
            commands::movie_rewatch,
            commands::movie_toggle_favorite,
            commands::search_tmdb,
            commands::show_add,
            commands::movie_add,
            commands::cache_info
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

- [ ] **Step 3: Verify**

From `src-tauri/`: `cargo test` — Expected: all prior tests still pass (≈17), none broken. Then `cargo check` — Expected: clean, no warnings about unused items (commands reference everything).

- [ ] **Step 4: Commit**

```
git add -A
git commit -m "feat: full command surface and tauri wiring with poster protocol"
```

---

### Task 11: Frontend foundation (types, api, layout, shared components)

**Files:**
- Create: `src/lib/types.ts`, `src/lib/api.ts`, `src/lib/session.svelte.ts`, `src/lib/ui/PosterImg.svelte`, `src/lib/ui/ProgressBar.svelte`
- Modify: `src/routes/+layout.svelte`

**Interfaces:**
- Consumes: the Task 10 command surface
- Produces: `api` object + `imgUrl()` (every page uses these), `session` store, `PosterImg`/`ProgressBar` components, sidebar layout.

- [ ] **Step 1: Write types.ts (mirrors models.rs exactly)**

`src/lib/types.ts`:
```ts
export interface NextEpisode {
  episodeId: number;
  seasonNumber: number;
  episodeNumber: number;
  name: string | null;
  airDate: string | null;
}

export interface ShowCard {
  tmdbId: number;
  name: string;
  posterPath: string | null;
  myState: "watching" | "stopped" | "plan_to_watch";
  isFavorite: boolean;
  watched: number;
  totalAired: number;
  next: NextEpisode | null;
  nextAirDate: string | null;
  lastActivity: string | null;
}

export interface ShowRow {
  tmdbId: number;
  tvdbId: number | null;
  imdbId: string | null;
  name: string;
  year: number | null;
  posterPath: string | null;
  backdropPath: string | null;
  overview: string | null;
  tmdbStatus: string | null;
  myState: "watching" | "stopped" | "plan_to_watch";
  isFavorite: boolean;
}

export interface EpisodeRow {
  tmdbId: number;
  seasonNumber: number;
  episodeNumber: number;
  name: string | null;
  airDate: string | null;
  runtime: number | null;
  stillPath: string | null;
  overview: string | null;
  watched: boolean;
  watchedAt: string | null;
}

export interface SeasonDetail {
  seasonNumber: number;
  episodes: EpisodeRow[];
}

export interface ShowDetail {
  show: ShowRow;
  seasons: SeasonDetail[];
}

export interface MovieRow {
  tmdbId: number;
  imdbId: string | null;
  title: string;
  year: number | null;
  releaseDate: string | null;
  runtime: number | null;
  posterPath: string | null;
  backdropPath: string | null;
  overview: string | null;
  myState: "watched" | "watchlist";
  watchedAt: string | null;
  rewatchCount: number;
  isFavorite: boolean;
}

export interface UpcomingItem {
  episodeId: number;
  showTmdbId: number;
  showName: string;
  posterPath: string | null;
  seasonNumber: number;
  episodeNumber: number;
  episodeName: string | null;
  airDate: string;
}

export interface SearchItem {
  tmdbId: number;
  mediaType: "tv" | "movie";
  title: string;
  year: number | null;
  posterPath: string | null;
  overview: string | null;
  inLibrary: boolean;
}

export interface ImportProgress {
  stage: "shows" | "movies";
  done: number;
  total: number;
  label: string;
}

export interface ImportFailure {
  kind: "show" | "movie";
  title: string;
  reason: string;
}

export interface ImportAnomaly {
  title: string;
  tmdbId: number;
  expected: number;
  actual: number;
}

export interface SpecialsNote {
  title: string;
  watchedSpecials: number;
}

export interface ImportReport {
  showsOk: number;
  moviesOk: number;
  failures: ImportFailure[];
  anomalies: ImportAnomaly[];
  specialsNote: SpecialsNote[];
  finishedAt: string;
}

export interface RefreshSummary {
  refreshed: number;
  failed: string[];
}

export interface CacheInfo {
  path: string;
  bytes: number;
}
```

- [ ] **Step 2: Write api.ts**

`src/lib/api.ts`:
```ts
import { invoke } from "@tauri-apps/api/core";
import type {
  CacheInfo, ImportReport, MovieRow, RefreshSummary, SearchItem,
  ShowCard, ShowDetail, UpcomingItem,
} from "./types";

export const api = {
  settingsAll: () => invoke<Record<string, string>>("settings_all"),
  settingsSet: (key: string, value: string) => invoke<void>("settings_set", { key, value }),
  tmdbValidateKey: (key: string) => invoke<boolean>("tmdb_validate_key", { key }),
  importRun: (path: string) => invoke<void>("import_run", { path }),
  importLastReport: () => invoke<ImportReport | null>("import_last_report"),
  refreshRun: () => invoke<RefreshSummary>("refresh_run"),
  showsList: () => invoke<ShowCard[]>("shows_list"),
  showDetail: (id: number) => invoke<ShowDetail>("show_detail", { id }),
  showSetState: (id: number, myState: string) => invoke<void>("show_set_state", { id, myState }),
  showToggleFavorite: (id: number) => invoke<boolean>("show_toggle_favorite", { id }),
  episodeMark: (id: number, watched: boolean) => invoke<void>("episode_mark", { id, watched }),
  seasonMark: (showId: number, season: number, watched: boolean) =>
    invoke<void>("season_mark", { showId, season, watched }),
  showMarkAll: (id: number) => invoke<void>("show_mark_all", { id }),
  showMarkNext: (id: number) => invoke<ShowCard>("show_mark_next", { id }),
  upcomingList: () => invoke<UpcomingItem[]>("upcoming_list"),
  moviesList: () => invoke<MovieRow[]>("movies_list"),
  movieSetState: (id: number, myState: string) => invoke<void>("movie_set_state", { id, myState }),
  movieRewatch: (id: number) => invoke<number>("movie_rewatch", { id }),
  movieToggleFavorite: (id: number) => invoke<boolean>("movie_toggle_favorite", { id }),
  searchTmdb: (query: string) => invoke<SearchItem[]>("search_tmdb", { query }),
  showAdd: (tmdbId: number, myState: string) => invoke<void>("show_add", { tmdbId, myState }),
  movieAdd: (tmdbId: number, myState: string) => invoke<void>("movie_add", { tmdbId, myState }),
  cacheInfo: () => invoke<CacheInfo>("cache_info"),
};

/** Build a poster:// URL (Windows WebView2 exposes custom schemes as http://<scheme>.localhost). */
export function imgUrl(size: "w185" | "w342" | "w780" | "original", path: string | null | undefined): string | null {
  if (!path) return null;
  const rel = `${size}${path}`; // TMDB paths start with '/'
  return navigator.userAgent.includes("Windows")
    ? `http://poster.localhost/${rel}`
    : `poster://localhost/${rel}`;
}
```

- [ ] **Step 3: Write session store + shared components**

`src/lib/session.svelte.ts`:
```ts
import { api } from "./api";

export const session = $state({
  ready: false,
  hasKey: false,
  banner: "" as string,
});

export async function loadSession() {
  const s = await api.settingsAll();
  session.hasKey = !!s["tmdb_key"];
  session.ready = true;
}
```

`src/lib/ui/PosterImg.svelte`:
```svelte
<script lang="ts">
  import { imgUrl } from "$lib/api";
  let {
    path,
    size = "w342",
    alt = "",
  }: { path: string | null | undefined; size?: "w185" | "w342" | "w780" | "original"; alt?: string } = $props();
  const url = $derived(imgUrl(size, path));
</script>

{#if url}
  <img src={url} {alt} loading="lazy" />
{:else}
  <div class="ph">{alt.slice(0, 2)}</div>
{/if}

<style>
  img, .ph {
    width: 100%; aspect-ratio: 2 / 3; object-fit: cover; border-radius: 8px;
    background: var(--panel2); display: block; border: 0;
  }
  .ph { display: flex; align-items: center; justify-content: center; color: var(--dim); font-size: 22px; }
</style>
```

`src/lib/ui/ProgressBar.svelte`:
```svelte
<script lang="ts">
  let { value, max }: { value: number; max: number } = $props();
  const pct = $derived(max > 0 ? Math.min(100, Math.round((value / max) * 100)) : 0);
</script>

<div class="row">
  <div class="bar"><div class="fill" style={`width:${pct}%`}></div></div>
  <span class="label">{value}/{max}</span>
</div>

<style>
  .row { display: flex; align-items: center; gap: 8px; }
  .bar { flex: 1; height: 6px; border-radius: 3px; background: var(--panel2); overflow: hidden; }
  .fill { height: 100%; background: var(--accent); border-radius: 3px; }
  .label { font-size: 12px; color: var(--dim); white-space: nowrap; }
</style>
```

- [ ] **Step 4: Write the sidebar layout**

`src/routes/+layout.svelte`:
```svelte
<script lang="ts">
  import "../app.css";
  import { onMount } from "svelte";
  import { page } from "$app/state";
  import { loadSession, session } from "$lib/session.svelte";

  let { children } = $props();

  const links = [
    { href: "/", label: "Shows" },
    { href: "/upcoming", label: "Upcoming" },
    { href: "/movies", label: "Movies" },
    { href: "/search", label: "Search" },
    { href: "/settings", label: "Settings" },
  ];

  onMount(() => {
    loadSession();
  });
</script>

<div class="shell">
  <nav>
    <div class="logo">Show<span>Track</span></div>
    {#each links as l (l.href)}
      <a href={l.href} class:active={page.url.pathname === l.href}>{l.label}</a>
    {/each}
  </nav>
  <main>
    {#if session.banner}
      <div class="banner">
        <span>{session.banner}</span>
        <button onclick={() => (session.banner = "")}>✕</button>
      </div>
    {/if}
    {#if session.ready}
      {@render children()}
    {/if}
  </main>
</div>

<style>
  .shell { display: flex; min-height: 100vh; }
  nav {
    width: 190px; flex-shrink: 0; padding: 20px 12px; border-right: 1px solid var(--border);
    display: flex; flex-direction: column; gap: 4px; position: sticky; top: 0; height: 100vh;
  }
  .logo { font-size: 18px; font-weight: 700; margin: 0 8px 20px; }
  .logo span { color: var(--accent); }
  nav a { padding: 8px 12px; border-radius: 8px; color: var(--dim); }
  nav a:hover { background: var(--panel); color: var(--text); }
  nav a.active { background: var(--panel2); color: var(--text); font-weight: 600; }
  main { flex: 1; padding: 24px 28px; max-width: 1400px; }
</style>
```

- [ ] **Step 5: Verify and commit**

Run from repo root: `pnpm check` — Expected: 0 errors, 0 warnings. `pnpm build` — Expected: clean.

```
git add -A
git commit -m "feat: frontend foundation (types, api, session, layout, shared components)"
```

---

### Task 12: Onboarding + Settings screen (key, import wizard, refresh, cache)

**Files:**
- Create: `src/routes/settings/+page.svelte`, `src/lib/ui/Onboarding.svelte`
- Modify: `src/routes/+layout.svelte` (onboarding gate)

- [ ] **Step 1: Onboarding component**

`src/lib/ui/Onboarding.svelte`:
```svelte
<script lang="ts">
  import { api } from "$lib/api";
  import { session } from "$lib/session.svelte";

  let key = $state("");
  let checking = $state(false);
  let error = $state("");

  async function validate() {
    checking = true;
    error = "";
    try {
      if (await api.tmdbValidateKey(key.trim())) {
        session.hasKey = true;
      } else {
        error = "TMDB rejected that key (401). Double-check it and try again.";
      }
    } catch (e) {
      error = `Could not reach TMDB: ${e}`;
    } finally {
      checking = false;
    }
  }
</script>

<div class="wrap">
  <div class="panel box">
    <h1>Welcome to Show<span style="color:var(--accent)">Track</span></h1>
    <p class="dim">
      ShowTrack needs a free TMDB API key for posters, episode data and search.
      Create an account at
      <a class="link" href="https://www.themoviedb.org/settings/api" target="_blank">themoviedb.org/settings/api</a>
      and paste your key (v3) or Read Access Token (v4) below.
    </p>
    <input placeholder="TMDB API key or token" bind:value={key} />
    {#if error}<p class="err">{error}</p>{/if}
    <button class="primary" onclick={validate} disabled={checking || key.trim() === ""}>
      {checking ? "Checking…" : "Validate & save"}
    </button>
    <p class="dim">Next step: import your TV Time export from the Settings page.</p>
  </div>
</div>

<style>
  .wrap { display: flex; align-items: center; justify-content: center; min-height: 80vh; }
  .box { max-width: 460px; display: flex; flex-direction: column; gap: 14px; }
  .link { color: var(--accent); }
  .err { color: var(--red); margin: 0; }
</style>
```

- [ ] **Step 2: Gate the layout**

In `src/routes/+layout.svelte`, replace the `{#if session.ready} {@render children()} {/if}` block with:
```svelte
    {#if session.ready}
      {#if session.hasKey}
        {@render children()}
      {:else}
        <Onboarding />
      {/if}
    {/if}
```
and add to its `<script>`: `import Onboarding from "$lib/ui/Onboarding.svelte";`

- [ ] **Step 3: Settings page**

`src/routes/settings/+page.svelte`:
```svelte
<script lang="ts">
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { open } from "@tauri-apps/plugin-dialog";
  import { api } from "$lib/api";
  import { session } from "$lib/session.svelte";
  import type { CacheInfo, ImportProgress, ImportReport, RefreshSummary } from "$lib/types";

  let key = $state("");
  let keyStatus = $state("");
  let importing = $state(false);
  let progress = $state<ImportProgress | null>(null);
  let report = $state<ImportReport | null>(null);
  let refreshing = $state(false);
  let refreshResult = $state<RefreshSummary | null>(null);
  let cache = $state<CacheInfo | null>(null);
  let importError = $state("");

  onMount(() => {
    api.importLastReport().then((r) => (report = r));
    api.cacheInfo().then((c) => (cache = c));
    const unProgress = listen<ImportProgress>("import:progress", (e) => (progress = e.payload));
    const unDone = listen<ImportReport>("import:done", (e) => {
      report = e.payload;
      importing = false;
      progress = null;
    });
    return () => {
      unProgress.then((f) => f());
      unDone.then((f) => f());
    };
  });

  async function saveKey() {
    keyStatus = "checking…";
    try {
      const ok = await api.tmdbValidateKey(key.trim());
      keyStatus = ok ? "✓ valid, saved" : "✗ rejected by TMDB";
      if (ok) session.hasKey = true;
    } catch (e) {
      keyStatus = `✗ ${e}`;
    }
  }

  async function runImport() {
    importError = "";
    const path = await open({
      title: "Pick your TV Time export",
      filters: [{ name: "HTML", extensions: ["html"] }],
    });
    if (typeof path !== "string") return;
    try {
      importing = true;
      await api.importRun(path);
    } catch (e) {
      importing = false;
      importError = String(e);
    }
  }

  async function runRefresh() {
    refreshing = true;
    refreshResult = null;
    try {
      refreshResult = await api.refreshRun();
    } catch (e) {
      refreshResult = { refreshed: 0, failed: [String(e)] };
    } finally {
      refreshing = false;
    }
  }

  const mb = (b: number) => (b / 1024 / 1024).toFixed(1);
</script>

<h1>Settings</h1>

<div class="panel section">
  <h2>TMDB API key</h2>
  <div class="row">
    <input style="flex:1" placeholder="paste a new key to replace the stored one" bind:value={key} />
    <button class="primary" onclick={saveKey} disabled={key.trim() === ""}>Validate & save</button>
  </div>
  {#if keyStatus}<p class="dim">{keyStatus}</p>{/if}
</div>

<div class="panel section">
  <h2>Import from TV Time</h2>
  <p class="dim">
    Re-runnable: it only adds and updates, never deletes. Episodes marked in ShowTrack keep their dates.
  </p>
  <button class="primary" onclick={runImport} disabled={importing}>
    {importing ? "Importing…" : "Pick export file & import"}
  </button>
  {#if importError}<p style="color:var(--red)">{importError}</p>{/if}
  {#if progress}
    <p>{progress.stage}: {progress.done + 1}/{progress.total} — {progress.label}</p>
    <div class="pbar"><div style={`width:${((progress.done + 1) / Math.max(progress.total, 1)) * 100}%`}></div></div>
  {/if}
  {#if report}
    <div class="report">
      <h2>Last import — {report.finishedAt}</h2>
      <p>✓ {report.showsOk} shows, {report.moviesOk} movies imported.</p>
      {#if report.failures.length}
        <h2>Failures ({report.failures.length})</h2>
        <ul>
          {#each report.failures as f}
            <li><b>{f.title}</b> ({f.kind}) — {f.reason}</li>
          {/each}
        </ul>
        <p class="dim">Add these manually via Search, or re-run the import to retry.</p>
      {/if}
      {#if report.anomalies.length}
        <h2>Count mismatches ({report.anomalies.length})</h2>
        <ul>
          {#each report.anomalies as a}
            <li>
              <a class="alink" href={`/show/${a.tmdbId}`}><b>{a.title}</b></a>
              — TV Time says {a.expected} watched, ShowTrack marked {a.actual}. Review its episodes.
            </li>
          {/each}
        </ul>
      {/if}
      {#if report.specialsNote.length}
        <h2>Watched specials to re-tick manually</h2>
        <ul>
          {#each report.specialsNote as s}
            <li><b>{s.title}</b> — {s.watchedSpecials} special(s) were watched on TV Time</li>
          {/each}
        </ul>
      {/if}
    </div>
  {/if}
</div>

<div class="panel section">
  <h2>Refresh airing shows</h2>
  <button onclick={runRefresh} disabled={refreshing}>{refreshing ? "Refreshing…" : "Refresh now"}</button>
  {#if refreshResult}
    <p class="dim">
      {refreshResult.refreshed} refreshed{refreshResult.failed.length
        ? `, ${refreshResult.failed.length} failed: ${refreshResult.failed.join("; ")}`
        : ""}
    </p>
  {/if}
</div>

<div class="panel section">
  <h2>Image cache</h2>
  {#if cache}<p class="dim">{cache.path} — {mb(cache.bytes)} MB</p>{/if}
</div>

<style>
  .section { margin-bottom: 16px; }
  .section h2 { margin-top: 0; }
  .row { display: flex; gap: 8px; }
  .pbar { height: 8px; border-radius: 4px; background: var(--panel2); overflow: hidden; }
  .pbar div { height: 100%; background: var(--accent); transition: width 0.2s; }
  .report ul { margin: 4px 0 8px; padding-left: 20px; }
  .alink { color: var(--accent); }
</style>
```

- [ ] **Step 4: Verify and commit**

`pnpm check` → 0 errors. `pnpm build` → clean.
Manual smoke (optional at this point, full smoke happens in Task 18): `pnpm tauri dev`, expect onboarding on first launch; paste your real TMDB key; expect it to validate; go to Settings and run the import against `%USERPROFILE%\Downloads\tvtime-summary-2026-07-01.html`; expect live progress then a report.

```
git add -A
git commit -m "feat: onboarding gate and settings with import wizard"
```

---

### Task 13: Shows screen (Watch Next)

**Files:**
- Create: `src/lib/ui/ShowCard.svelte`
- Modify: `src/routes/+page.svelte` (replace placeholder)

- [ ] **Step 1: ShowCard tile component**

`src/lib/ui/ShowCard.svelte`:
```svelte
<script lang="ts">
  import { api } from "$lib/api";
  import type { ShowCard } from "$lib/types";
  import PosterImg from "./PosterImg.svelte";
  import ProgressBar from "./ProgressBar.svelte";

  let { card, onupdate }: { card: ShowCard; onupdate: (c: ShowCard) => void } = $props();
  let marking = $state(false);

  const se = (s: number, e: number) =>
    `S${String(s).padStart(2, "0")}E${String(e).padStart(2, "0")}`;

  async function markNext() {
    if (marking || !card.next) return;
    marking = true;
    try {
      onupdate(await api.showMarkNext(card.tmdbId));
    } finally {
      marking = false;
    }
  }
</script>

<div class="card">
  <a href={`/show/${card.tmdbId}`}>
    <PosterImg path={card.posterPath} alt={card.name} />
  </a>
  <div class="body">
    <a class="name" href={`/show/${card.tmdbId}`} title={card.name}>{card.name}</a>
    <ProgressBar value={card.watched} max={card.totalAired} />
    {#if card.next}
      <div class="next" title={card.next.name ?? ""}>
        <span class="dim">{se(card.next.seasonNumber, card.next.episodeNumber)}</span>
        {card.next.name ?? ""}
      </div>
      <button class="primary" onclick={markNext} disabled={marking}>✓ Watched</button>
    {:else if card.nextAirDate}
      <div class="next dim">Next airs {card.nextAirDate}</div>
    {:else}
      <div class="next dim">Caught up</div>
    {/if}
  </div>
</div>

<style>
  .card { display: flex; flex-direction: column; }
  .body { display: flex; flex-direction: column; gap: 6px; padding: 8px 2px; }
  .name { font-weight: 600; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .next { font-size: 12px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; min-height: 16px; }
</style>
```

- [ ] **Step 2: Shows page**

`src/routes/+page.svelte`:
```svelte
<script lang="ts">
  import { onMount } from "svelte";
  import { api } from "$lib/api";
  import type { ShowCard } from "$lib/types";
  import ShowCardTile from "$lib/ui/ShowCard.svelte";

  let cards = $state<ShowCard[]>([]);
  let loaded = $state(false);
  let filter = $state<"watching" | "plan_to_watch" | "stopped" | "all">("watching");
  let query = $state("");

  onMount(async () => {
    cards = await api.showsList();
    loaded = true;
  });

  const chips = [
    ["watching", "Watching"],
    ["plan_to_watch", "Plan to watch"],
    ["stopped", "Stopped"],
    ["all", "All"],
  ] as const;

  const byActivity = (a: ShowCard, b: ShowCard) =>
    (b.lastActivity ?? "").localeCompare(a.lastActivity ?? "") || a.name.localeCompare(b.name);

  const visible = $derived(
    cards.filter((c) => query.trim() === "" || c.name.toLowerCase().includes(query.trim().toLowerCase())),
  );
  const watchNext = $derived(
    visible.filter((c) => c.myState === "watching" && c.next).toSorted(byActivity),
  );
  const upToDate = $derived(
    visible.filter((c) => c.myState === "watching" && !c.next).toSorted(byActivity),
  );
  const others = $derived(
    visible
      .filter((c) => (filter === "all" ? true : c.myState === filter))
      .toSorted((a, b) => a.name.localeCompare(b.name)),
  );

  function replace(updated: ShowCard) {
    cards = cards.map((c) => (c.tmdbId === updated.tmdbId ? updated : c));
  }
</script>

<h1>Shows</h1>

<div class="chips">
  {#each chips as [value, label] (value)}
    <button class="chip" class:active={filter === value} onclick={() => (filter = value)}>{label}</button>
  {/each}
  <input style="margin-left:auto" placeholder="Filter…" bind:value={query} />
</div>

{#if !loaded}
  <p class="dim">Loading…</p>
{:else if cards.length === 0}
  <p class="dim">No shows yet — run the TV Time import from Settings, or add shows via Search.</p>
{:else if filter === "watching"}
  <h2>Watch next ({watchNext.length})</h2>
  <div class="grid-posters">
    {#each watchNext as card (card.tmdbId)}
      <ShowCardTile {card} onupdate={replace} />
    {/each}
  </div>
  {#if upToDate.length}
    <h2>Up to date ({upToDate.length})</h2>
    <div class="grid-posters">
      {#each upToDate as card (card.tmdbId)}
        <ShowCardTile {card} onupdate={replace} />
      {/each}
    </div>
  {/if}
{:else}
  <div class="grid-posters">
    {#each others as card (card.tmdbId)}
      <ShowCardTile {card} onupdate={replace} />
    {/each}
  </div>
{/if}
```

- [ ] **Step 3: Verify and commit**

`pnpm check` → 0 errors (note: `toSorted` needs TS lib ES2023 — the SvelteKit default targets it; if `pnpm check` complains, replace `.toSorted(fn)` with `.slice().sort(fn)`). `pnpm build` → clean.

```
git add -A
git commit -m "feat: shows screen with watch-next cards and one-tap advance"
```

---

### Task 14: Show detail screen

**Files:**
- Create: `src/routes/show/[id]/+page.svelte`

- [ ] **Step 1: Implement the page**

```svelte
<script lang="ts">
  import { onMount } from "svelte";
  import { page } from "$app/state";
  import { api, imgUrl } from "$lib/api";
  import type { ShowDetail } from "$lib/types";
  import PosterImg from "$lib/ui/PosterImg.svelte";

  let detail = $state<ShowDetail | null>(null);
  let error = $state("");

  const id = $derived(Number(page.params.id));

  async function load() {
    try {
      detail = await api.showDetail(id);
    } catch (e) {
      error = String(e);
    }
  }

  onMount(load);

  const se = (s: number, e: number) => `S${String(s).padStart(2, "0")}E${String(e).padStart(2, "0")}`;

  async function toggleEpisode(epId: number, watched: boolean) {
    if (!detail) return;
    // optimistic
    for (const s of detail.seasons)
      for (const e of s.episodes)
        if (e.tmdbId === epId) {
          e.watched = watched;
          e.watchedAt = watched ? new Date().toISOString() : null;
        }
    await api.episodeMark(epId, watched);
  }

  async function markSeason(season: number, watched: boolean) {
    await api.seasonMark(id, season, watched);
    await load();
  }

  async function markAll() {
    await api.showMarkAll(id);
    await load();
  }

  async function setState(myState: string) {
    if (!detail) return;
    detail.show.myState = myState as ShowDetail["show"]["myState"];
    await api.showSetState(id, myState);
  }

  async function toggleFav() {
    if (!detail) return;
    detail.show.isFavorite = await api.showToggleFavorite(id);
  }

  const seasonName = (n: number) => (n === 0 ? "Specials" : `Season ${n}`);
  const watchedIn = (eps: { watched: boolean }[]) => eps.filter((e) => e.watched).length;
</script>

{#if error}
  <p style="color:var(--red)">{error}</p>
{:else if !detail}
  <p class="dim">Loading…</p>
{:else}
  {#if detail.show.backdropPath}
    <div class="backdrop" style={`background-image:url(${imgUrl("w780", detail.show.backdropPath)})`}></div>
  {/if}
  <div class="head">
    <div class="poster"><PosterImg path={detail.show.posterPath} alt={detail.show.name} /></div>
    <div class="info">
      <h1>
        {detail.show.name}
        {#if detail.show.year}<span class="dim">({detail.show.year})</span>{/if}
        <button class="fav" class:on={detail.show.isFavorite} onclick={toggleFav} title="Favorite">★</button>
      </h1>
      {#if detail.show.tmdbStatus}<p class="dim">{detail.show.tmdbStatus}</p>{/if}
      {#if detail.show.overview}<p class="ov">{detail.show.overview}</p>{/if}
      <div class="actions">
        <select value={detail.show.myState} onchange={(e) => setState(e.currentTarget.value)}>
          <option value="watching">Watching</option>
          <option value="plan_to_watch">Plan to watch</option>
          <option value="stopped">Stopped</option>
        </select>
        <button onclick={markAll}>Mark all aired watched</button>
      </div>
    </div>
  </div>

  {#each detail.seasons as season (season.seasonNumber)}
    <details class="panel season" open={season.seasonNumber > 0}>
      <summary>
        <b>{seasonName(season.seasonNumber)}</b>
        <span class="dim">{watchedIn(season.episodes)}/{season.episodes.length}</span>
        <button
          onclick={(e) => {
            e.preventDefault();
            markSeason(season.seasonNumber, watchedIn(season.episodes) < season.episodes.length);
          }}
        >
          {watchedIn(season.episodes) < season.episodes.length ? "Mark season watched" : "Unmark season"}
        </button>
      </summary>
      {#each season.episodes as ep (ep.tmdbId)}
        <label class="ep">
          <input
            type="checkbox"
            checked={ep.watched}
            onchange={(e) => toggleEpisode(ep.tmdbId, e.currentTarget.checked)}
          />
          <span class="dim senum">{se(ep.seasonNumber, ep.episodeNumber)}</span>
          <span class="ename">{ep.name ?? "TBA"}</span>
          <span class="dim">{ep.airDate ?? ""}</span>
        </label>
      {/each}
    </details>
  {/each}
{/if}

<style>
  .backdrop {
    height: 180px; border-radius: var(--radius); background-size: cover;
    background-position: center 20%; opacity: 0.45; margin-bottom: -60px;
    mask-image: linear-gradient(to bottom, black, transparent);
  }
  .head { display: flex; gap: 20px; margin-bottom: 24px; position: relative; }
  .poster { width: 150px; flex-shrink: 0; }
  .info { flex: 1; min-width: 0; }
  .ov { max-width: 720px; color: var(--dim); }
  .actions { display: flex; gap: 8px; margin-top: 10px; }
  .fav { background: none; font-size: 20px; color: var(--dim); padding: 0 6px; }
  .fav.on { color: var(--accent); }
  .season { margin-bottom: 10px; padding: 10px 14px; }
  summary { cursor: pointer; display: flex; align-items: center; gap: 12px; }
  summary button { margin-left: auto; font-size: 12px; }
  .ep { display: flex; align-items: center; gap: 10px; padding: 6px 4px; border-top: 1px solid var(--border); cursor: pointer; }
  .ep:hover { background: rgba(255, 255, 255, 0.03); }
  .senum { font-family: monospace; }
  .ename { flex: 1; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .ep input { accent-color: var(--accent); width: 16px; height: 16px; }
</style>
```

- [ ] **Step 2: Verify and commit**

`pnpm check` → 0 errors. `pnpm build` → clean.

```
git add -A
git commit -m "feat: show detail with per-episode and per-season marking"
```

---

### Task 15: Upcoming screen

**Files:**
- Create: `src/routes/upcoming/+page.svelte`

- [ ] **Step 1: Implement the page**

```svelte
<script lang="ts">
  import { onMount } from "svelte";
  import { api } from "$lib/api";
  import type { UpcomingItem } from "$lib/types";
  import PosterImg from "$lib/ui/PosterImg.svelte";

  let items = $state<UpcomingItem[]>([]);
  let loaded = $state(false);

  onMount(async () => {
    items = await api.upcomingList();
    loaded = true;
  });

  const groups = $derived.by(() => {
    const map = new Map<string, UpcomingItem[]>();
    for (const it of items) {
      const list = map.get(it.airDate) ?? [];
      list.push(it);
      map.set(it.airDate, list);
    }
    return [...map.entries()];
  });

  function daysUntil(date: string): string {
    const today = new Date();
    today.setHours(0, 0, 0, 0);
    const d = new Date(`${date}T00:00:00`);
    const diff = Math.round((d.getTime() - today.getTime()) / 86400000);
    if (diff === 1) return "tomorrow";
    if (diff < 7) return `in ${diff} days`;
    if (diff < 30) return `in ${Math.round(diff / 7)} week${diff >= 14 ? "s" : ""}`;
    return `in ${Math.round(diff / 30)} month${diff >= 60 ? "s" : ""}`;
  }

  const se = (s: number, e: number) => `S${String(s).padStart(2, "0")}E${String(e).padStart(2, "0")}`;
</script>

<h1>Upcoming</h1>

{#if !loaded}
  <p class="dim">Loading…</p>
{:else if groups.length === 0}
  <p class="dim">Nothing scheduled for the shows you're watching.</p>
{:else}
  {#each groups as [date, dayItems] (date)}
    <h2>{date} · {daysUntil(date)}</h2>
    <div class="day">
      {#each dayItems as it (it.episodeId)}
        <a class="panel item" href={`/show/${it.showTmdbId}`}>
          <div class="mini"><PosterImg path={it.posterPath} size="w185" alt={it.showName} /></div>
          <div class="txt">
            <b>{it.showName}</b>
            <span class="dim">{se(it.seasonNumber, it.episodeNumber)} — {it.episodeName ?? "TBA"}</span>
          </div>
        </a>
      {/each}
    </div>
  {/each}
{/if}

<style>
  .day { display: flex; flex-direction: column; gap: 8px; margin-bottom: 8px; }
  .item { display: flex; gap: 12px; padding: 10px; align-items: center; }
  .item:hover { border-color: var(--accent); }
  .mini { width: 46px; flex-shrink: 0; }
  .txt { display: flex; flex-direction: column; gap: 2px; min-width: 0; }
</style>
```

- [ ] **Step 2: Verify and commit**

`pnpm check` → 0 errors. `pnpm build` → clean.

```
git add -A
git commit -m "feat: upcoming screen grouped by air date"
```

---

### Task 16: Movies screen with detail modal

**Files:**
- Create: `src/routes/movies/+page.svelte`

- [ ] **Step 1: Implement the page**

```svelte
<script lang="ts">
  import { onMount } from "svelte";
  import { api } from "$lib/api";
  import type { MovieRow } from "$lib/types";
  import PosterImg from "$lib/ui/PosterImg.svelte";

  let movies = $state<MovieRow[]>([]);
  let loaded = $state(false);
  let tab = $state<"watched" | "watchlist">("watched");
  let sort = $state<"watchedAt" | "title" | "year">("watchedAt");
  let query = $state("");
  let selected = $state<MovieRow | null>(null);

  onMount(async () => {
    movies = await api.moviesList();
    loaded = true;
  });

  const visible = $derived.by(() => {
    const q = query.trim().toLowerCase();
    const list = movies.filter((m) => m.myState === tab && (q === "" || m.title.toLowerCase().includes(q)));
    const cmp: Record<string, (a: MovieRow, b: MovieRow) => number> = {
      watchedAt: (a, b) => (b.watchedAt ?? "").localeCompare(a.watchedAt ?? ""),
      title: (a, b) => a.title.localeCompare(b.title),
      year: (a, b) => (b.year ?? 0) - (a.year ?? 0),
    };
    return list.slice().sort(cmp[sort]);
  });

  function replace(updated: MovieRow) {
    movies = movies.map((m) => (m.tmdbId === updated.tmdbId ? updated : m));
    if (selected?.tmdbId === updated.tmdbId) selected = updated;
  }

  async function setState(m: MovieRow, myState: "watched" | "watchlist") {
    const watchedAt = myState === "watched" ? (m.watchedAt ?? new Date().toISOString()) : null;
    replace({ ...m, myState, watchedAt, rewatchCount: myState === "watched" ? m.rewatchCount : 0 });
    await api.movieSetState(m.tmdbId, myState);
  }

  async function rewatch(m: MovieRow) {
    replace({ ...m, rewatchCount: await api.movieRewatch(m.tmdbId) });
  }

  async function toggleFav(m: MovieRow) {
    replace({ ...m, isFavorite: await api.movieToggleFavorite(m.tmdbId) });
  }
</script>

<h1>Movies</h1>

<div class="chips">
  <button class="chip" class:active={tab === "watched"} onclick={() => (tab = "watched")}>
    Watched ({movies.filter((m) => m.myState === "watched").length})
  </button>
  <button class="chip" class:active={tab === "watchlist"} onclick={() => (tab = "watchlist")}>
    Watchlist ({movies.filter((m) => m.myState === "watchlist").length})
  </button>
  <select bind:value={sort} style="margin-left:auto">
    <option value="watchedAt">By date watched</option>
    <option value="title">By title</option>
    <option value="year">By year</option>
  </select>
  <input placeholder="Filter…" bind:value={query} />
</div>

{#if !loaded}
  <p class="dim">Loading…</p>
{:else if visible.length === 0}
  <p class="dim">Nothing here yet.</p>
{:else}
  <div class="grid-posters">
    {#each visible as m (m.tmdbId)}
      <button class="cell" onclick={() => (selected = m)}>
        <PosterImg path={m.posterPath} alt={m.title} />
        <span class="t" title={m.title}>{m.title}</span>
        <span class="dim">{m.year ?? ""}{m.isFavorite ? " ★" : ""}</span>
      </button>
    {/each}
  </div>
{/if}

{#if selected}
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="overlay" onclick={() => (selected = null)} role="presentation">
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <div class="panel modal" onclick={(e) => e.stopPropagation()} role="presentation">
      <div class="mposter"><PosterImg path={selected.posterPath} alt={selected.title} /></div>
      <div class="mbody">
        <h1>
          {selected.title}
          {#if selected.year}<span class="dim">({selected.year})</span>{/if}
        </h1>
        {#if selected.runtime}<p class="dim">{selected.runtime} min</p>{/if}
        {#if selected.overview}<p class="dim">{selected.overview}</p>{/if}
        {#if selected.myState === "watched"}
          <p>
            Watched {selected.watchedAt ? selected.watchedAt.slice(0, 10) : ""}
            {#if selected.rewatchCount > 0}· {selected.rewatchCount} rewatch{selected.rewatchCount > 1 ? "es" : ""}{/if}
          </p>
          <div class="mactions">
            <button onclick={() => rewatch(selected!)}>+ Rewatch</button>
            <button onclick={() => setState(selected!, "watchlist")}>Move to watchlist</button>
          </div>
        {:else}
          <div class="mactions">
            <button class="primary" onclick={() => setState(selected!, "watched")}>✓ Watched now</button>
          </div>
        {/if}
        <div class="mactions">
          <button class:favon={selected.isFavorite} onclick={() => toggleFav(selected!)}>
            {selected.isFavorite ? "★ Favorite" : "☆ Favorite"}
          </button>
          <button onclick={() => (selected = null)}>Close</button>
        </div>
      </div>
    </div>
  </div>
{/if}

<style>
  .cell { background: none; padding: 0; text-align: left; display: flex; flex-direction: column; gap: 4px; }
  .t { font-weight: 600; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .overlay {
    position: fixed; inset: 0; background: rgba(0, 0, 0, 0.6);
    display: flex; align-items: center; justify-content: center; z-index: 10;
  }
  .modal { display: flex; gap: 18px; max-width: 640px; width: 90%; }
  .mposter { width: 180px; flex-shrink: 0; }
  .mbody { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 8px; }
  .mactions { display: flex; gap: 8px; }
  .favon { color: var(--accent); }
</style>
```

- [ ] **Step 2: Verify and commit**

`pnpm check` → 0 errors. `pnpm build` → clean.

```
git add -A
git commit -m "feat: movies screen with watched/watchlist tabs and detail modal"
```

---

### Task 17: Search & add screen

**Files:**
- Create: `src/routes/search/+page.svelte`

- [ ] **Step 1: Implement the page**

```svelte
<script lang="ts">
  import { api } from "$lib/api";
  import type { SearchItem } from "$lib/types";
  import PosterImg from "$lib/ui/PosterImg.svelte";

  let query = $state("");
  let results = $state<SearchItem[]>([]);
  let searching = $state(false);
  let error = $state("");
  let addedIds = $state<Set<number>>(new Set());
  let timer: ReturnType<typeof setTimeout> | undefined;

  $effect(() => {
    const q = query.trim();
    clearTimeout(timer);
    if (q.length < 2) {
      results = [];
      return;
    }
    timer = setTimeout(async () => {
      searching = true;
      error = "";
      try {
        results = await api.searchTmdb(q);
      } catch (e) {
        error = String(e);
      } finally {
        searching = false;
      }
    }, 350);
    return () => clearTimeout(timer);
  });

  async function add(item: SearchItem, myState: string) {
    try {
      if (item.mediaType === "tv") await api.showAdd(item.tmdbId, myState);
      else await api.movieAdd(item.tmdbId, myState);
      addedIds = new Set(addedIds).add(item.tmdbId);
    } catch (e) {
      error = String(e);
    }
  }
</script>

<h1>Search</h1>

<input class="big" placeholder="Search shows and movies on TMDB…" bind:value={query} />
{#if error}<p style="color:var(--red)">{error}</p>{/if}
{#if searching}<p class="dim">Searching…</p>{/if}

<div class="grid-posters" style="margin-top:16px">
  {#each results as r (`${r.mediaType}-${r.tmdbId}`)}
    <div class="cell">
      <PosterImg path={r.posterPath} alt={r.title} />
      <span class="t" title={r.title}>{r.title}</span>
      <span class="dim">{r.mediaType === "tv" ? "Show" : "Movie"}{r.year ? ` · ${r.year}` : ""}</span>
      {#if r.inLibrary || addedIds.has(r.tmdbId)}
        <span class="in">✓ In library</span>
      {:else if r.mediaType === "tv"}
        <div class="row">
          <button class="primary" onclick={() => add(r, "watching")}>Watching</button>
          <button onclick={() => add(r, "plan_to_watch")}>Plan</button>
        </div>
      {:else}
        <div class="row">
          <button class="primary" onclick={() => add(r, "watched")}>Watched</button>
          <button onclick={() => add(r, "watchlist")}>Watchlist</button>
        </div>
      {/if}
    </div>
  {/each}
</div>

<style>
  .big { width: 100%; font-size: 16px; padding: 12px 14px; }
  .cell { display: flex; flex-direction: column; gap: 4px; }
  .t { font-weight: 600; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .row { display: flex; gap: 6px; }
  .row button { flex: 1; font-size: 12px; padding: 5px 6px; }
  .in { color: var(--green); font-size: 12px; }
</style>
```

- [ ] **Step 2: Verify and commit**

`pnpm check` → 0 errors. `pnpm build` → clean.

```
git add -A
git commit -m "feat: TMDB search with add-to-library flows"
```

---

### Task 18: Launch refresh, README, final verification

**Files:**
- Modify: `src/routes/+layout.svelte` (auto-refresh + offline banner)
- Create: `README.md`

- [ ] **Step 1: Auto-refresh on launch**

In `src/routes/+layout.svelte`, replace the `onMount` block with:
```svelte
  onMount(async () => {
    await loadSession();
    if (session.hasKey) {
      try {
        const s = await api.refreshRun();
        if (s.failed.length > 0) {
          session.banner = `Refresh: ${s.refreshed} updated, ${s.failed.length} failed — showing cached data where needed.`;
        }
      } catch {
        // busy or offline — cached data is fine, note it quietly
        session.banner = "Could not refresh airing shows (offline?) — showing cached data.";
      }
    }
  });
```
and add `import { api } from "$lib/api";` to the layout's `<script>` imports.

- [ ] **Step 2: README**

`README.md`:
```markdown
# ShowTrack

Personal TV show + movie tracker (TV Time style). Tauri 2 + Rust + SvelteKit (Svelte 5).

- Spec: `docs/superpowers/specs/2026-07-02-showtrack-design.md`
- Data: SQLite in the app data dir; TMDB metadata cached locally (posters under `images/`).
- Setup: needs a free TMDB API key (asked on first run).
- Import: Settings → "Pick export file & import" with a TV Time Out (Refract) export HTML.

## Dev

    pnpm install
    pnpm tauri dev

## Tests

    cd src-tauri && cargo test            # unit + engine tests
    cargo test real_export -- --ignored   # parses the real export file
    $env:TMDB_KEY="..."; cargo test tmdb_live -- --ignored   # live TMDB smoke

## Release build

    pnpm tauri build --no-bundle          # exe at src-tauri/target/release/showtrack.exe

(Never bare `cargo build --release` — it skips the frontend embed.)
```

- [ ] **Step 3: Full verification suite**

Run and confirm each:
1. From `src-tauri/`: `cargo test` — Expected: all pass (≈18 tests).
2. From repo root: `pnpm check` — Expected: 0 errors, 0 warnings.
3. `pnpm build` — Expected: clean.
4. `Get-Process showtrack -ErrorAction SilentlyContinue | Stop-Process -Force` then `pnpm tauri build --no-bundle` — Expected: "Built application at: …\src-tauri\target\release\showtrack.exe".

- [ ] **Step 4: Manual smoke test (real data, real key)**

Launch `src-tauri\target\release\showtrack.exe` and walk through:
1. Window appears once, dark, no white flash. Onboarding asks for the TMDB key → paste real key → validates.
2. Settings → import `%USERPROFILE%\Downloads\tvtime-summary-2026-07-01.html` → progress ticks through 171 shows then 594 movies (a few minutes) → report shows ~171 shows / ~594 movies OK; note any failures/anomalies (a handful is expected — numbering drift).
3. Shows: Watch Next cards render with posters + progress; clicking ✓ advances the card. Black Mirror should appear with S07E01 as next (it had 6 unwatched S7 episodes in the export).
4. Show detail (e.g. Breaking Bad): 62/62 regular episodes checked, specials unchecked; season fold-outs work.
5. Upcoming lists future episodes of watching shows, grouped by date.
6. Movies: Watched tab ≈553 entries sorted by watch date; watchlist ≈41; modal actions work.
7. Search: find a show not in the library, add as Plan to watch → appears under Shows → Plan to watch chip.
8. Settings → cache info shows a growing MB figure (posters download on view).
9. Close and relaunch: launch refresh runs (banner only if offline/failed); data persists.

- [ ] **Step 5: Commit**

```
git add -A
git commit -m "feat: launch refresh, readme, final verification"
```

---

## Post-plan notes for the executor

- Task order is strict for Tasks 1–2 and 10–11 (wiring depends on everything before it; UI depends on the wiring). Tasks 3–9 can technically be built in any order but were sequenced parser → client → logic → SQL → engines → protocol for review sanity. Tasks 12–17 are mutually independent.
- The two `#[ignore]` tests (`real_export`, `tmdb_live`) are the reality checks — run them at Task 3/4 time, not just at the end.
- If TVDB→TMDB `/find` misses turn out to be common in the real import (report failures), a title+year `/search` fallback is the designed follow-up — do NOT bolt it on speculatively (YAGNI; the anomaly/failure report tells us if it's needed).
```
