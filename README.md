# ShowTrack

A personal TV show and movie tracker, built as a local-first replacement for TV Time.
Desktop (Windows) and Android from one codebase: Tauri 2 + Rust + SvelteKit (Svelte 5).

**Season-first by design.** ShowTrack assumes you watch whole seasons once the finale has
aired: the dashboard leads with complete seasons that are ready to watch, one tap marks a
season done, and the Airing page counts down to each finale instead of nagging about
single episodes.

## Features

- Season-first dashboard with an "Up next" pick, one-tap season marks and undo
- Separate season-first Anime tab (TMDB-based detection, one-time library scan)
- Airing page: shows in flight, grouped by the month their season becomes watchable
- Movie tracking with rewatches, favorites and watchlist
- TMDB search to add anything; metadata and posters cached locally in SQLite
- One-time import from a TV Time export (TV Time Out / Refract HTML)
- Two-way LAN sync with the Android app (pairing code, no cloud, see below)
- Library backup to a single JSON file, restore merges without ever deleting
- Feature toggles: turn off any page or dashboard section you do not use
- Everything stays on your machines; the only network dependency is TMDB

Bring your own free TMDB API key (themoviedb.org, Settings, API); the app asks on first
run and validates it.

This product uses the TMDB API but is not endorsed or certified by TMDB.

## Dev

    pnpm install
    pnpm tauri dev

## Tests

    cd src-tauri && cargo test            # unit + engine tests
    TVTIME_EXPORT=path\to\export.html cargo test real_export -- --ignored   # parses a real export
    $env:TMDB_KEY="..."; cargo test tmdb_live -- --ignored   # live TMDB smoke

## Release build

    pnpm tauri build --no-bundle          # exe at src-tauri/target/release/showtrack.exe
    pnpm tauri build --bundles nsis       # Windows installer under target/release/bundle/nsis/

(Never bare `cargo build --release` — it skips the frontend embed.)

Note: `.cargo/config.toml` remaps this machine's paths out of release binaries so shipped
exes carry no local filesystem layout; adjust the prefixes if you build from another setup.

## Phone sync

Two-way LAN sync between the desktop app and the Android app.

- Desktop: Settings, turn "Phone sync" on (default port 47810), click "Show pairing code".
  The server runs ONLY while the desktop app is open and the toggle is on.
- Phone: Settings, "Sync with desktop", enter this PC's address (find it with ipconfig,
  e.g. `192.168.1.20:47810`) plus the 6-digit code (one use, valid 5 minutes). Pairing
  copies the whole desktop library to the phone, then the phone syncs automatically on
  app open/foreground (and via "Sync now").
- Change tracking: every mark/state change appends to the `oplog` table; sync exchanges
  ops both ways with last-write-wins per field. The desktop takes a rolling DB backup
  (keep 5) in `<app-data>/backups/` before applying anything the phone pushes.
- Security model: 6-digit code mints a 32-byte bearer token (constant-time compared);
  no TLS by design (home LAN, Wi-Fi link encryption); no mDNS, manual IP entry.

## Android

    # once per shell:
    $env:JAVA_HOME = "C:\Program Files\Android\Android Studio\jbr"
    $env:ANDROID_HOME = "$env:LOCALAPPDATA\Android\Sdk"
    $env:NDK_HOME = "<newest dir under $env:ANDROID_HOME\ndk>"

    pnpm tauri android dev      # dev loop on the connected phone
    pnpm tauri android build --target aarch64   # signed release APK (arm64; universal builds are slow and wedge-prone)

Gradle project: `src-tauri/gen/android` (open in Android Studio). Signing key material lives
in `src-tauri/gen/android/showtrack-release.keystore`; its path + passwords in
`src-tauri/gen/android/keystore.properties`. Both are gitignored; protect both.

### If you ever re-run `pnpm tauri android init`

It regenerates `src-tauri/gen/android` and will OVERWRITE these hand-edits; re-apply them:

- `app/build.gradle.kts`: the `signingConfigs`/release `signingConfig` block
- `app/src/main/res/values/colors.xml`: `showtrackBg` (#07090F)
- `app/src/main/res/values/themes.xml` AND `values-night/themes.xml`: the four dark-chrome items
- `app/src/main/java/com/nokama/showtrack/MainActivity.kt`: the system-bar insets listener
  (Android 15 forces edge-to-edge; without it the app draws under the status bar)
- `app/src/main/AndroidManifest.xml`: `android:screenOrientation="portrait"` on the activity
- Launcher icon: `res/mipmap-*` pngs, `res/mipmap-anydpi-v26/ic_launcher.xml`, and the
  `ic_launcher_background` color (#07090F) in `res/values/colors.xml`. Legacy pngs come from
  `src-tauri/icons/android/`; adaptive foregrounds are `icons/icon.png` scaled to 85% on a
  transparent canvas (tauri icon's own foreground is full-bleed on white, which clips badly)

## License

MIT. See [LICENSE](LICENSE).
