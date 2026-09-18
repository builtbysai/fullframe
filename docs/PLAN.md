# FullFrame — Architecture Plan

Research: `~/workspace/research_notes/screenshot-app-20260917/report.md` (2026-09-17).
Framework decision: **Tauri v2** (Rust backend + web frontend).

## Why Tauri v2

- 3–30 MB binaries, ~50–100 MB RAM — a tray-resident utility must be light.
- Every primitive exists as a maintained crate/plugin: `xcap` (capture, all
  three OSes), `tauri-plugin-global-shortcut`, `tauri-plugin-clipboard-manager`
  (image read/write), transparent always-on-top overlay windows (core API).
- Built-in per-OS installers + auto-updater.
- Proven by real screenshot apps on Tauri v2 (nimshot-app, defeye, Clippity).

## Repo layout

```
fullframe/
├── src-tauri/                 # Rust backend
│   ├── Cargo.toml
│   ├── tauri.conf.json
│   ├── build.rs
│   └── src/
│       └── main.rs            # commands: capture, overlay mgmt, hotkeys, tray
├── src/                       # frontend (vanilla JS, no build step)
│   ├── overlay.html / overlay.js    # region-select overlay (transparent window)
│   ├── editor.html / editor.js      # annotation editor (canvas)
│   ├── editor.css
│   └── main.html / main.js          # control window (modes, settings, history)
├── extension/                 # companion browser extension (MV3)
│   ├── manifest.json
│   ├── background.js          # CDP full-page capture
│   └── popup.html / popup.js
└── docs/PLAN.md               # this file
```

## Capture pipeline

1. **Hotkey** (`tauri-plugin-global-shortcut`, default `PrintScreen`-style combo,
   user-configurable) → Rust opens a transparent fullscreen overlay window on
   every monitor.
2. **Overlay** (frontend): crosshair + magnifier + live coordinates. Drag =
   region. Double-click or `Enter` = fullscreen. `Esc` = cancel.
3. **Capture** (Rust, `xcap`): grab the monitor in physical pixels, crop to the
   region × per-monitor scale factor. Selection math is done in physical pixels;
   the overlay works in logical pixels — multiply by `scale_factor`.
4. **Window mode**: `xcap::Window::all()` → overlay lists clickable windows;
   click captures that window (extended frame bounds on Windows to avoid black
   shadow boxes).
5. Result PNG bytes → editor window.

### HiDPI / multi-monitor rules (do not regress these)

- Capture in physical pixels; overlay coordinates in logical pixels.
- Virtual desktop offsets can be negative — merge with min/max bounds.
- Per-monitor scale factor applied per monitor, not globally.

## Annotation editor (canvas, `editor.js`)

Tools: move/select, rectangle, ellipse, arrow, line, pen, text, highlight,
blur, pixelate, crop, step-number badges. Undo/redo stack. After editing:
copy to clipboard (PNG), save as PNG/JPEG/WebP (auto-name
`fullframe-YYYYMMDD-HHMMSS`), pin to screen (frameless always-on-top window),
open containing folder.

## Long screenshots (the headline feature)

**Primary: CDP `Page.captureScreenshot` with `captureBeyondViewport: true`.**
The browser engine renders the whole page in one shot — no stitching, no seam
artifacts, sticky elements appear exactly once. Same mechanism as DevTools'
"Capture full size screenshot".

- v0.1: companion MV3 extension works standalone — click the toolbar button
  (or its hotkey) → `chrome.debugger` attach → `Page.getLayoutMetrics` →
  `Page.captureScreenshot({captureBeyondViewport: true, fromSurface: true})` →
  detach → PNG download. Guardrails: cap height (~16k px), fall back to JPEG
  for huge pages, pre-scroll once to trigger lazy-loaded images.
- v0.2: desktop app triggers the extension via Native Messaging
  (Tauri app registers a native-messaging host manifest; extension streams the
  PNG back into the editor).
- Fallback: scroll-and-stitch for Firefox / pages that refuse CDP attach.

## Permissions (per OS, design first-run around these)

- **macOS**: TCC-gated. Ship `NSScreenCaptureUsageDescription`; user grants in
  Settings → Privacy & Security → Screen Recording. Sequoia re-prompts monthly —
  detect the grant on launch and deep-link to the Settings pane when missing.
- **Windows**: `xcap` (GDI/DXGI) needs no special grant; window capture uses
  extended frame bounds. DRM video is black under every API — documented, not
  fought.
- **Linux**: X11 just works. Wayland goes through `xdg-desktop-portal`
  (Screenshot/ScreenCast, one-time consent dialog). Global hotkeys are not
  portable on Wayland — ship GNOME/KDE portal-shortcut guidance + native
  binding instructions; don't promise arbitrary hotkeys on Linux.

## Roadmap

- [x] v0.0 — repo, research, plan (this doc)
- [x] v0.1 — Tauri scaffold; region + fullscreen + window capture; annotation
  editor (shapes, text, blur/pixelate, crop, undo/redo); pin-to-screen;
  copy/save; tray; global hotkeys; companion extension (standalone
  full-page capture via CDP — verified seam-free on a 4800px test page)
- [x] v0.2 (in progress) — delayed capture (3/5/10 s countdown, region or
  fullscreen); JPEG save honoring the chosen extension; extension tiled
  fallback for pages over 16384 px (validated: seamless tiles, capped at
  32767 px, never silently truncated)
- [ ] v0.2 (remaining) — native-messaging link between app and extension;
  settings UI (hotkeys, save location, naming); scrolling capture of native
  windows
## Screen recording design (v0.3 — next milestone)

Prior art: quickshotter's `recording/` module (dual-thread pipeline, trait +
HW/CPU encoders, hand-written MP4 muxer). FullFrame adapts it:

```
src-tauri/src/recording/
  mod.rs          RecordingManager in AppState; start/stop commands
  pipeline.rs     capture thread (xcap paced loop, region crop) -> bounded
                  channel(4) -> encoder thread; drops frames under backpressure
  encoder.rs      VideoEncoder trait: new(w,h,fps) / encode_frame(rgba, pts)
                  / flush() -> H.264 Annex-B NALs + keyframe flags
  encoder_cpu.rs  openh264 backend — the v1 encoder on ALL platforms
                  (testable here; HW encoders later behind the same trait)
  mp4_muxer.rs    minimal ISOBMFF: ftyp, moov(mvhd/trak/avc1/avcC/stts/stss/
                  stsc/stsz/stco), mdat with AVCC length-prefixed samples
  gif_encoder.rs  stretch: gif crate, downscale + quantize, duration cap
```

- Region recording reuses the overlay: select rect -> record that rect.
- Fullscreen recording = all monitors stitched (same as screenshot).
- Frontend: main-window record section (region/fullscreen, MP4/GIF, 15/30
  fps), floating indicator window (elapsed + stop, draggable), tray stop item.
- v1 is video-only (no system-audio loopback); save via dialog on stop.
- Verify: ffprobe validity, full-decode frame count, visual frame inspection.

- [ ] v0.3 — screen recording (MP4 via openh264, region/fullscreen, indicator
  + tray stop; GIF stretch); OCR (Tesseract on-device)
- [ ] v1.0 — screen recording (Snipping Tool parity — required before
  "feature complete"), signed installers (Win/macOS/Linux), auto-update
  feed, first-run permission flows

## Non-goals for v1

Cloud upload (local-first by design), AI redaction (differentiator for v1.1+).

## Verification notes (2026-09-17)

- `cargo check` / `cargo build`: clean, zero warnings.
- App launched headless under Xvfb: no panics; main window screenshot-verified.
- Editor driven end-to-end in headless Chromium via CDP (rect, arrow, text
  typing + Enter commit, blur bake, step badge, undo/redo): zero console
  errors. Two real bugs found and fixed structurally: (1) the text tool's
  textarea was instantly blurred/hidden by the browser's default mousedown
  focus change — fixed with `preventDefault()`; (2) `commitText` double-fired
  via Enter keydown + async blur — fixed by making commit idempotent by
  state instead of a racy timeout guard. (3) `bake()` could burn the
  in-progress drag preview (marching ants) into the image — fixed by
  re-rendering clean before every bake.
- Region overlay driven via CDP: `finish_region_capture` invoked with exact
  logical coordinates; visuals (dim, crosshair, magnifier, selection)
  screenshot-verified.
- Linux window listing needs a running WM (`_NET_CLIENT_LIST_STACKING`); the
  UI degrades gracefully without one.
