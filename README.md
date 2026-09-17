# FullFrame

A cross-platform screenshot app that does everything Snipping Tool does — and the things it should have done years ago.

**Capture**
- **Region** — drag to select, with crosshair, live coordinates, and a pixel magnifier. `PrtSc`
- **Fullscreen** — all monitors merged into one image. `Ctrl+PrtSc`
- **Window** — pick any window from the list
- **Delayed** — 3/5/10 s countdown for region or fullscreen (menus & tooltips stay open)
- **Long screenshot** — full web pages via the companion browser extension. Rendered by the browser engine itself (CDP `Page.captureScreenshot`) — no stitch seams, sticky headers appear once. Pages taller than 16384 px are captured in tiles and stitched, never silently truncated (up to 32767 px).

**After capture**
- Annotation editor: select/move, rectangle, ellipse, arrow, line, pen, highlighter, text, step-number badges, blur, pixelate, crop — with full undo/redo and zoom
- **Pin to screen** (frameless always-on-top reference)
- Copy to clipboard, save as PNG or JPEG (format follows the file extension you pick)
- Tray icon with quick actions; single-instance; global hotkeys

**Platforms:** Windows, macOS, Linux. Local-first — no account, no cloud, nothing leaves your machine.

**Roadmap:** screen recording (Snipping Tool parity — required before v1.0), app↔extension link, OCR, settings UI, signed installers. See [docs/PLAN.md](docs/PLAN.md).

## Status

v0.2-dev — region / fullscreen / window / delayed capture, annotation editor, pin-to-screen, companion extension. See [docs/PLAN.md](docs/PLAN.md) for the architecture and roadmap.

## Building

Prerequisites: Rust (stable), plus Tauri's system libraries for your OS.

```bash
# Linux (Debian/Ubuntu)
sudo apt install libwebkit2gtk-4.1-dev build-essential libxdo-dev libssl-dev \
  libayatana-appindicator3-dev librsvg2-dev libpipewire-0.3-dev libclang-dev \
  libgbm-dev

cd src-tauri
cargo run        # dev
cargo build --release   # release binary
```

macOS: first launch will ask for Screen Recording permission — grant it in System Settings → Privacy & Security.

Linux notes: X11 works out of the box. On Wayland, capture goes through `xdg-desktop-portal` (one-time consent dialog), and arbitrary global hotkeys are not portable — use the tray menu there. Window listing needs a running window manager.

## Browser extension (long screenshots)

`extension/` is a Manifest V3 extension. Load it unpacked in `chrome://extensions` (Developer mode on), then click its toolbar button on any page. It attaches via the DevTools protocol, wakes lazy-loaded content with a pre-scroll, and renders the entire page as one PNG. Extremely long pages (>16384 px) are captured in tiles and stitched — never silently cut off.

## License

MIT — see [LICENSE](LICENSE).
