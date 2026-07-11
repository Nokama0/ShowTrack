# ShowTrack — Android Port + LAN Sync (2026-07-09)

Port the existing app to Android via **Tauri 2's Android target in the same repo** (approach A),
in two phases with separate implementation plans:

- **Phase 1**: the Android app itself — builds, installs, fully usable standalone with a
  phone-fit UI. Deliverable: signed APK on the user's Samsung.
- **Phase 2**: two-way LAN sync between desktop and phone with pairing-code security.

Desktop behavior is unchanged except where noted (sync server, Settings additions).

## Phase 1 — Android app

### Toolchain / scaffold

- One-time machine setup: `rustup target add aarch64-linux-android armv7-linux-androideabi
  x86_64-linux-android i686-linux-android`; NDK + SDK via the installed Android Studio
  (SDK manager); env vars `JAVA_HOME` (Android Studio's JBR), `ANDROID_HOME`, `NDK_HOME`.
- `pnpm tauri android init` generates `src-tauri/gen/android` (Gradle project — committed,
  minus build outputs). Android Studio + the JetBrains MCP open this project for builds,
  emulator runs, and logcat; day-to-day dev loop is `pnpm tauri android dev` against the
  connected Samsung (ADB already working on this machine).
- Release: locally generated keystore (`showtrack-release.keystore`, NOT committed; path +
  passwords in `src-tauri/gen/android/keystore.properties`, gitignored), Gradle signing
  config, `pnpm tauri android build` → signed APK, installed via `adb install`.
- Identifier stays `com.nokama.showtrack`; icons already generated (android mipmaps exist).

### Portability fixes

- **TLS**: `reqwest = { version = "0.12", default-features = false, features = ["json",
  "rustls-tls"] }` on all platforms (native-tls doesn't build for Android). Desktop suite +
  `tmdb_live` re-verified after the switch.
- **Poster protocol origin**: `imgUrl()` currently branches on `userAgent.includes("Windows")`.
  Android WebView also serves custom schemes from `http://<scheme>.localhost`, so the branch
  becomes "Windows OR Android → `http://poster.localhost/...`". Verified on-device first
  (posters are load-bearing).
- **UI-scale setting**: hidden on Android (userAgent check); webview zoom stays desktop-only.
- Everything else (rusqlite bundled via NDK, tauri path APIs for app-data/cache dirs, dialog
  plugin file picker, refresh-on-launch, poster disk cache) works as-is.

### Mobile UI pass

All behind width media queries / a `isMobile` platform check — desktop rendering unchanged.
This also closes the known "dock hidden below 760px leaves no nav" gap:

- **Bottom tab bar** replaces the dock at ≤760px: fixed, 5 tabs (Shows · Airing · Movies ·
  Search · Settings), active = gold, `env(safe-area-inset-bottom)` padding; main content gets
  matching bottom padding.
- Shows: hero compacts (smaller Anton mark, actions full-width under the text); grids
  `minmax(112px, 1fr)` (~3 across); airing + caught sections flow inline after the grid
  (the `.cols` rail already stacks at this width).
- Detail: backdrop shorter (200px), poster+meta stacked and centered, episode rows ≥44px tall,
  season summary wraps cleanly.
- Ring: ≥44px effective touch target on mobile (larger button, same visual).
- Movies/Search: grid ~3 across; modal becomes a bottom sheet (full-width, slides up).
- Settings: single column; UI-scale panel hidden.
- Android system chrome: status + navigation bars `#07090f` (Gradle theme / MainActivity),
  dark splash screen, no white flash.

### Phase 1 data

Standalone: own SQLite + poster cache in Android app storage; TMDB key via existing
onboarding. TV Time import stays available (Android file picker) but is not the expected
bootstrap path — Phase 2's snapshot is.

## Phase 2 — two-way LAN sync

### Change tracking (shared schema, both platforms)

- Migration v2: `oplog(seq INTEGER PRIMARY KEY AUTOINCREMENT, ts TEXT NOT NULL, entity TEXT
  NOT NULL, entity_id TEXT NOT NULL, op TEXT NOT NULL, payload TEXT)` +
  `sync_state(key TEXT PRIMARY KEY, value TEXT NOT NULL)` (stores: device id, shared token,
  peer cursor, last-sync time; on desktop also: sync enabled flag, port).
- Every mutation path appends an op: episode watch/unwatch (ids), season mark/unmark
  (expanded to episode ids), show state/favorite, movie state/rewatch/favorite, show/movie add
  (payload = the TMDB id; the receiving side fetches metadata via its own TMDB client if the
  entity is unknown, else applies state only). Import appends ops normally (bootstrap uses
  snapshots, so this only matters for post-pairing imports).
- Applying ops is idempotent; conflicts resolve last-write-wins by op `ts` per
  (entity, entity_id). Ops carry the origin device id; devices ignore their own ops on pull.

### Desktop sync server

- Rust HTTP server (axum), bound `0.0.0.0:47810` (port configurable in Settings), running
  ONLY while the desktop app runs and the Settings "Phone sync" toggle is on.
- Endpoints (`/sync/v1/...`): `POST /pair` (body: 6-digit code currently displayed in desktop
  Settings; success mints + returns a 32-byte random token, stored both sides; code expires
  after one use or 5 minutes), `GET /snapshot` (full library JSON: shows, episodes, watches,
  movies, tmdb key, current seq), `GET /changes?since=<seq>`, `POST /changes` (op batch;
  desktop takes a rolling DB backup — keep 5 — before applying).
- Auth: every endpoint except `pair` requires `Authorization: Bearer <token>`, constant-time
  compare, 401 otherwise. No TLS (home-LAN threat model; the token gates writes; Wi-Fi is
  link-encrypted). No mDNS/UPnP/port-forwarding — manual IP entry on the phone.
- Desktop Settings panel: toggle, port, "Show pairing code" (generates + displays), paired
  device + last-sync display, unpair button.

### Phone sync client

- Settings → "Sync with desktop": IP(+port) entry → pair with code → snapshot bootstrap
  (replaces onboarding when it carries the TMDB key) → thereafter auto-sync on app
  open/foreground + manual "Sync now"; each sync = pull peer ops since cursor → apply →
  push own ops since peer's cursor → update cursors. Offline/unreachable desktop = quiet
  banner, app fully usable.

## Out of scope

iOS; more than two devices; internet (non-LAN) sync; background sync while the app is closed;
mDNS discovery; TLS on the sync link; Play Store distribution.

## Error handling

Existing patterns carry over (loadError, banners, safely-resync). Sync failures are per-batch
atomic (transaction), surfaced as banners, never block the UI. Pairing failures show the
reason (wrong code, expired, unreachable).

## Testing

- Phase 1: existing suite green on desktop after the TLS swap (`tmdb_live` re-run); frontend
  gates; on-device checklist (posters render via poster.localhost, onboarding + key entry,
  add/mark/undo, bottom nav, safe areas, back gesture, import via file picker).
- Phase 2: Rust unit tests — oplog append on every mutation path, idempotent re-apply, LWW
  both orders, pair/auth (bad code, expired code, missing/wrong token), snapshot round-trip
  into an empty DB, two-in-memory-DB full sync loop converges. On-device: pair, bootstrap,
  couch-mark → desktop shows it after sync, desktop-mark → phone shows it.

## Recorded deviations (accepted, Phase 2)

- The oplog stores ONLY local-origin ops; applying peer ops writes tables directly and never
  appends. The spec's `origin` device-id column is therefore unnecessary and was dropped —
  "devices ignore their own ops on pull" holds structurally.
- "Per-batch atomic" apply is relaxed to prefix-apply: ops apply in seq order, the first hard
  error stops the batch, and the cursor advances to the last success. Applying an `add` op can
  require a TMDB fetch, which cannot live inside a DB transaction; idempotent apply makes
  retries safe.
- LWW conflicts resolve per (entity, entity_id, field group) rather than per (entity,
  entity_id): watch/unwatch compete; add+state compete; favorite and rewatch each stand alone.
  This stops a state change from silently discarding a favorite toggle.
- `ep watch` ops carry the show id in the payload so a receiver missing the episode row
  (e.g. the peer refreshed a new season first) can fetch metadata and self-heal.
- Snapshot bootstrap merges instead of replacing: metadata upserts always; user state and
  watches apply only to entities the phone does not already have; the TMDB key transfers only
  when the phone has none. Phone-local data survives pairing.
- Migration v2 seeds the oplog from the existing library on Android only, so pre-pairing phone
  data flows to the desktop on first push. Desktop does not seed (snapshot covers that
  direction; 16k+ import watches would bloat the oplog for nothing).
- Ops are logged only for mutations that actually changed state (a re-import or redundant
  mark produces no ops).
- Sync modules compile on both platforms; the UI gates which commands are reachable.
- Pairing does not replace onboarding: the app requires a manually entered TMDB key to reach
  Settings, so a fresh phone install enters its key first and pairs second. The snapshot's
  key transfer applies only when a local key is absent after onboarding.

## Recorded deviations (accepted, Phase 1)

- TV Time import is desktop-only: Android's file picker returns content:// URIs the importer
  cannot read, and Phase 2's snapshot bootstrap is the intended phone on-ramp. The import
  panel is hidden on Android.
- Release APK is built arm64-only (`--target aarch64`) for the user's device; the universal
  4-ABI build is slower and has twice wedged silently on this machine.
- The on-device checklist was verified by controller screenshots (launch chrome, onboarding,
  posters via poster.localhost, search+add over rustls, dashboard, tab bar/safe areas).
