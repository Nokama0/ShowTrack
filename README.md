<div align="center">

<img src="src-tauri/icons/128x128@2x.png" width="80" alt="ShowTrack" />

# ShowTrack

**Wait for the finale. Then binge.**

A local-first TV and movie tracker for people who watch a season at a time.

![Platform](https://img.shields.io/badge/Platform-Windows%20%26%20Android-FFCF6E?labelColor=0E0F13&style=flat)
![Your data](https://img.shields.io/badge/Your%20data-100%25%20local-FFCF6E?labelColor=0E0F13&style=flat)
![License](https://img.shields.io/badge/License-MIT-FFCF6E?labelColor=0E0F13&style=flat)

</div>

---

ShowTrack is built around one idea: you watch a show once its whole season has finished
airing. So the dashboard waits for the finale, then surfaces the season as ready to binge.
One tap marks it done, and the Airing page counts down to each finale rather than tracking
single episodes.

> "4 seasons are complete and waiting. Tap the ring when you finish one."

It runs on your Windows desktop and Android phone from one codebase, keeps everything in a
local database, and only reaches the internet to fetch show data from
[TheMovieDB](https://www.themoviedb.org). Nothing about your library leaves your devices.

<sub>Watch episode by episode instead? A single setting flips the dashboard to surface
anything with a new episode, no waiting for the finale.</sub>

## What it does

**Watching**
- A season-first dashboard with an "Up next" pick, complete seasons ready to binge, and one-tap season marks with undo
- A dedicated Anime tab, detected from TMDB and kept season-first
- An Airing page that groups shows by the month their current season becomes bingeable

**Your library**
- Movies with watch dates, rewatch counts, favorites and a watchlist
- Search TMDB to add anything; posters and metadata cache locally
- One-time import from a TV Time export
- Backup the whole library to one JSON file; restoring only ever adds, never deletes

**Across your devices**
- Two-way sync between desktop and phone over your home network, no cloud (see [below](#sync-your-phone))
- Turn off any page or dashboard section you do not use
- Custom window title, tile sizes, navigation order and more, all optional

## Get it

1. **Download** the latest [release](https://github.com/Nokama0/ShowTrack/releases/latest).
   - `ShowTrack-...-setup.exe` installs it (recommended).
   - `ShowTrack-...-portable.exe` is a single file, no install. It needs Microsoft Edge
     WebView2, which ships with Windows 11 and current Windows 10.
   - The first launch may show a Windows SmartScreen warning because the app is not
     code-signed. Choose **More info**, then **Run anyway**.
2. **Add a TMDB key.** ShowTrack asks for a free
   [TMDB API key](https://www.themoviedb.org/settings/api) on first run and validates it.
   All metadata and posters come from TMDB.
3. **Bring your history (optional).** Coming from TV Time? Export your data, then import it
   once from Settings.

Updates are built in: **Settings > Maintenance > Check for updates**. Installed copies
update through the installer; portable copies replace themselves in place.

## Sync your phone

Two-way sync over your local network. No account, no cloud, nothing leaves your Wi-Fi.

1. **On the desktop:** Settings > Phone sync, turn it on, then **Show pairing code**. The
   sync server runs only while the desktop app is open.
2. **On the phone:** Settings > Sync with desktop. Enter this PC's address (find it with
   `ipconfig`, for example `192.168.1.20:47810`) and the 6-digit code. The code is single
   use and valid for 5 minutes.
3. Pairing copies the whole desktop library to the phone. After that the phone syncs on its
   own each time you open it, or on demand with **Sync now**.

<sub>What happens under the hood is in <a href="#how-sync-works">How sync works</a>.</sub>

---

## Build it

Tauri 2 + Rust + SvelteKit (Svelte 5), one codebase for Windows and Android, pnpm workspace.

```sh
pnpm install
pnpm tauri dev
```

### Tests

```sh
cd src-tauri && cargo test                                              # unit + engine tests
TVTIME_EXPORT=path\to\export.html cargo test real_export -- --ignored   # parse a real export
$env:TMDB_KEY="..."; cargo test tmdb_live -- --ignored                  # live TMDB smoke test
```

### Release build

```sh
pnpm tauri build --no-bundle       # exe at src-tauri/target/release/showtrack.exe
pnpm tauri build --bundles nsis    # Windows installer under target/release/bundle/nsis/
```

Never run a bare `cargo build --release`; it skips the frontend embed. `.cargo/config.toml`
remaps this machine's paths out of release binaries so shipped exes carry no local
filesystem layout; adjust the prefixes if you build from a different setup.

### Android

```sh
# once per shell:
$env:JAVA_HOME    = "C:\Program Files\Android\Android Studio\jbr"
$env:ANDROID_HOME = "$env:LOCALAPPDATA\Android\Sdk"
$env:NDK_HOME     = "<newest dir under $env:ANDROID_HOME\ndk>"

pnpm tauri android dev                       # dev loop on the connected phone
pnpm tauri android build --target aarch64    # signed arm64 release APK
```

Universal builds are slow and wedge-prone, so target `aarch64`. The Gradle project is
`src-tauri/gen/android` (open it in Android Studio). Signing key material lives in
`src-tauri/gen/android/showtrack-release.keystore`, with its path and passwords in
`keystore.properties` beside it. Both are gitignored; keep them safe.

### How sync works

- **Change tracking.** Every mark or state change appends to the `oplog` table. Sync
  exchanges ops both ways with last-write-wins per field. Before applying anything the phone
  pushes, the desktop takes a rolling database backup (keeps 5) in `<app-data>/backups/`.
- **Security model.** The 6-digit code mints a 32-byte bearer token, compared in constant
  time. No TLS by design (home LAN, Wi-Fi link encryption); no mDNS, so the IP is entered by
  hand. The server binds to the chosen port (default 47810) only while the app is open and
  the toggle is on.

<details>
<summary><b>If you re-run <code>pnpm tauri android init</code></b></summary>

It regenerates `src-tauri/gen/android` and will overwrite these hand-edits, so re-apply them:

- `app/build.gradle.kts`: the `signingConfigs` and release `signingConfig` block
- `app/src/main/res/values/colors.xml`: `showtrackBg` (#07090F)
- `app/src/main/res/values/themes.xml` **and** `values-night/themes.xml`: the four dark-chrome items
- `app/src/main/java/com/nokama/showtrack/MainActivity.kt`: the system-bar insets listener
  (Android 15 forces edge-to-edge; without it the app draws under the status bar) plus the
  `onWebViewCreate` override that disables the native scrollbars (they draw above the tab bar)
- `app/src/main/AndroidManifest.xml`: `android:screenOrientation="portrait"` on the activity
- Launcher icon: the `res/mipmap-*` pngs, `res/mipmap-anydpi-v26/ic_launcher.xml`, and the
  `ic_launcher_background` color (#07090F) in `res/values/colors.xml`. Legacy pngs come from
  `src-tauri/icons/android/`; adaptive foregrounds are `icons/icon.png` scaled to 85% on a
  transparent canvas (tauri icon's own foreground is full-bleed on white, which clips badly)

</details>

## License and attribution

MIT. See [LICENSE](LICENSE).

This product uses the TMDB API but is not endorsed or certified by TMDB.
