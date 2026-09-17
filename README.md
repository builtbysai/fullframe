# FullFrame

A cross-platform screenshot app that does everything Snipping Tool does — and the things it should have done years ago.

**Capture**
- **Region** — drag to select, with crosshair, live coordinates, and a pixel magnifier. `PrtSc`
- **Fullscreen** — all monitors merged into one image. `Ctrl+PrtSc`
- **Window** — pick any window from the list
- **Long screenshot** — full web pages rendered in one shot via the companion browser extension (CDP `Page.captureScreenshot`, no stitching, no seams)

**After capture**
- Annotation editor: select/move, rectangle, ellipse, arrow, line, pen, highlighter, text, step-number badges, blur, pixelate, crop — with full undo/redo and zoom
- **Pin to screen** (frameless always-on-top reference)
- Copy to clipboard, save as PNG/JPEG
- Tray icon with quick actions; single-instance; global hotkeys

**Platforms:** Windows, macOS, Linux. Local-first — no account, no cloud, nothing leaves your machine.

## Status

v0.1 — region / fullscreen / window capture, annotation editor, pin-to-screen, companion extension. See [docs/PLAN.md](docs/PLAN.md) for the architecture and roadmap.

## Building

Prerequisites: Rust (stable), plus Tauri's system libraries for your OS.

```bash
# Linux (Debian/Ubuntu)
sudo apt install libwebkit2gtk-4.1-dev build-essential libxdo-dev libssl-dev \
  libayatana-appindicator3-dev librsvg2-dev libpipewire-0.3-dev libclang-dev

cd src-tauri
cargo run        # dev
cargo build --release   # release binary
```

macOS: first launch will ask for Screen Recording permission — grant it in System Settings → Privacy & Security.

## Browser extension (long screenshots)

`extension/` is a Manifest V3 extension. Load it unpacked in `chrome://extensions` (Developer mode on), then click its toolbar button on any page. It attaches via the DevTools protocol, wakes lazy-loaded content with a pre-scroll, and renders the entire page as one PNG.

## License

MIT — see [LICENSE](LICENSE).
