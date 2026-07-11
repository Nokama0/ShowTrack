# ShowTrack — UI Scale (Zoom) Control (2026-07-03)

User request: a way to change the font size. Decision: scale the **whole UI** (text, posters,
spacing together), implemented as native webview zoom — identical semantics to browser Ctrl+=.

## Design

- **Storage**: key `ui_zoom` in the existing `settings` table; value is a percent integer as a
  string (`"80"`–`"150"`), default `100` when absent. Reuses the existing `settings_all` /
  `settings_set` commands — **no Rust changes**.
- **Application**: `getCurrentWebview().setZoom(pct / 100)` from `@tauri-apps/api/webview`.
  - `loadSession()` parses `ui_zoom` from the settings map into a new `session.zoom` field
    (clamped to 80–150; anything unparsable → 100).
  - `+layout.svelte` applies it in a `$effect` that reads `session.zoom` — covers both the
    launch application (after `loadSession`) and live changes from the Settings screen.
  - `setZoom` failures are caught and ignored (cosmetic feature; app stays usable at 100%).
- **Capability**: add `core:webview:allow-set-webview-zoom` to
  `src-tauri/capabilities/default.json`.
- **Settings UI**: new "UI scale" panel row on the Settings page — ▼ / `NNN%` / ▲ stepper,
  step 10, clamped 80–150 (buttons disable at the bounds). On click: update `session.zoom`
  (the layout `$effect` applies it instantly) and persist via `settings_set("ui_zoom", …)`.
- **Out of scope** (YAGNI): zoom hotkeys, per-screen scale, text-only font sizing.

## Testing

- Gates: `pnpm check` 0/0, `pnpm build` clean (frontend-only change).
- Manual: stepper changes scale live; value survives relaunch; bounds clamp; 100% default on
  fresh DB.
