// FullFrame backend — Tauri v2.
// Capture pipeline: xcap (physical pixels) -> crop/scale math -> PNG bytes.
// Overlay windows show the screenshot dimmed; selection is cropped from the
// already-captured image, so there is no second capture and no drift.

use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use image::{GenericImageView, ImageFormat, RgbaImage};
use serde::Serialize;
use std::collections::HashMap;
use std::io::Cursor;
use std::sync::Mutex;
use tauri::{
    image::Image as TauriImage,
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder,
};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

// ---------------------------------------------------------------- state ---

#[derive(Clone, Copy)]
struct MonitorGeo {
    id: u32,
    x: i32, // logical px, virtual-desktop coords (may be negative)
    y: i32,
    w: u32, // logical px
    h: u32,
    scale: f32,
}

#[derive(Default)]
struct AppState {
    previews: Mutex<HashMap<u32, Vec<u8>>>, // monitor id -> full PNG bytes
    geos: Mutex<HashMap<u32, MonitorGeo>>,
    capture: Mutex<Option<Vec<u8>>>, // latest finished capture (PNG bytes)
    overlay_open: Mutex<bool>,
}

// ------------------------------------------------------------- helpers ---

fn png_bytes(img: &RgbaImage) -> Result<Vec<u8>, String> {
    let mut buf = Vec::new();
    image::DynamicImage::ImageRgba8(img.clone())
        .write_to(&mut Cursor::new(&mut buf), ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    Ok(buf)
}

fn data_url(png: &[u8]) -> String {
    format!("data:image/png;base64,{}", B64.encode(png))
}

fn png_from_data_url(url: &str) -> Result<Vec<u8>, String> {
    let b64 = url
        .split_once(",")
        .map(|(_, b)| b)
        .ok_or_else(|| "bad data url".to_string())?;
    B64.decode(b64).map_err(|e| e.to_string())
}

/// Clamp a physical-pixel rect inside an image and crop.
fn crop_png(png: &[u8], x: u32, y: u32, w: u32, h: u32) -> Result<Vec<u8>, String> {
    let img = image::load_from_memory(png)
        .map_err(|e| e.to_string())?
        .to_rgba8();
    let (iw, ih) = img.dimensions();
    if iw == 0 || ih == 0 {
        return Err("empty image".to_string());
    }
    let x = x.min(iw - 1);
    let y = y.min(ih - 1);
    let w = w.min(iw - x).max(1);
    let h = h.min(ih - y).max(1);
    let sub = image::imageops::crop_imm(&img, x, y, w, h).to_image();
    png_bytes(&sub)
}

fn close_overlays(app: &AppHandle) {
    let labels: Vec<String> = app
        .webview_windows()
        .keys()
        .filter(|l| l.starts_with("overlay-"))
        .cloned()
        .collect();
    for l in labels {
        if let Some(w) = app.get_webview_window(&l) {
            let _ = w.close();
        }
    }
    let st = app.state::<AppState>();
    if let Ok(mut o) = st.overlay_open.lock() {
        *o = false;
    };
}

fn open_editor(app: &AppHandle) -> Result<(), String> {
    if let Some(w) = app.get_webview_window("editor") {
        let _ = w.close();
    }
    let win = WebviewWindowBuilder::new(app, "editor", WebviewUrl::App("editor.html".into()))
        .title("FullFrame Editor")
        .inner_size(1280.0, 800.0)
        .min_inner_size(800.0, 600.0)
        .center()
        .resizable(true)
        .build()
        .map_err(|e| e.to_string())?;
    let _ = win.set_focus();
    Ok(())
}

// ------------------------------------------------------------ commands ---

#[derive(Serialize, Clone)]
struct PreviewInfo {
    id: u32,
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    scale: f32,
}

/// Capture every monitor, open a transparent overlay on each, and hand the
/// screenshots to the overlays for dimmed display + magnifier.
#[tauri::command]
fn begin_region_capture(app: AppHandle) -> Result<(), String> {
    let st = app.state::<AppState>();
    if *st.overlay_open.lock().map_err(|e| e.to_string())? {
        return Ok(()); // already capturing
    }
    let monitors = xcap::Monitor::all().map_err(|e| e.to_string())?;
    if monitors.is_empty() {
        return Err("no monitors found".to_string());
    }
    {
        let mut previews = st.previews.lock().map_err(|e| e.to_string())?;
        let mut geos = st.geos.lock().map_err(|e| e.to_string())?;
        previews.clear();
        geos.clear();
        for m in &monitors {
            let id = m.id().map_err(|e| e.to_string())?;
            let img = m.capture_image().map_err(|e| e.to_string())?;
            let png = png_bytes(&img)?;
            let scale = m.scale_factor().map_err(|e| e.to_string())?.max(0.5);
            let (pw, ph) = (
                m.width().map_err(|e| e.to_string())?,
                m.height().map_err(|e| e.to_string())?,
            );
            let geo = MonitorGeo {
                id,
                x: m.x().map_err(|e| e.to_string())?,
                y: m.y().map_err(|e| e.to_string())?,
                w: ((pw as f32) / scale).round() as u32,
                h: ((ph as f32) / scale).round() as u32,
                scale,
            };
            previews.insert(id, png);
            geos.insert(id, geo);
        }
    }
    *st.overlay_open.lock().map_err(|e| e.to_string())? = true;

    let geos: Vec<MonitorGeo> = st
        .geos
        .lock()
        .map_err(|e| e.to_string())?
        .values()
        .cloned()
        .collect();

    // Fallible from here on: keep the flag false until every overlay is up,
    // so a failure can't wedge the hotkey into a dead "capturing" state.
    let previews = st.previews.lock().map_err(|e| e.to_string())?;
    for g in &geos {
        let label = format!("overlay-{}", g.id);
        if let Some(w) = app.get_webview_window(&label) {
            let _ = w.close();
        }
        let win =
            match WebviewWindowBuilder::new(&app, &label, WebviewUrl::App("overlay.html".into()))
                .title("FullFrame Capture")
                .transparent(true)
                .decorations(false)
                .shadow(false)
                .always_on_top(true)
                .skip_taskbar(true)
                .resizable(false)
                .minimizable(false)
                .maximizable(false)
                .closable(true)
                .position(g.x as f64, g.y as f64)
                .inner_size(g.w as f64, g.h as f64)
                .visible(true)
                .build()
            {
                Ok(w) => w,
                Err(e) => {
                    drop(previews);
                    close_overlays(&app);
                    return Err(format!("could not open capture overlay: {e}"));
                }
            };
        let png = previews.get(&g.id).cloned().unwrap_or_default();
        let info = PreviewInfo {
            id: g.id,
            x: g.x,
            y: g.y,
            w: g.w,
            h: g.h,
            scale: g.scale,
        };
        let _ = win.emit(
            "overlay-preview",
            serde_json::json!({
                "monitor": info,
                "data_url": data_url(&png),
            }),
        );
        let _ = win.set_focus();
    }
    drop(previews);
    // Only now mark capturing: a failure above leaves the flag false, so the
    // hotkey can't wedge into a dead "capturing" state.
    *st.overlay_open.lock().map_err(|e| e.to_string())? = true;
    Ok(())
}

/// Crop the selected logical rect out of the stored full-monitor capture.
#[tauri::command]
fn finish_region_capture(
    app: AppHandle,
    monitor_id: u32,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
) -> Result<(), String> {
    let st = app.state::<AppState>();
    let geo = st
        .geos
        .lock()
        .map_err(|e| e.to_string())?
        .get(&monitor_id)
        .cloned()
        .ok_or_else(|| "unknown monitor".to_string())?;
    let png = st
        .previews
        .lock()
        .map_err(|e| e.to_string())?
        .get(&monitor_id)
        .cloned()
        .ok_or_else(|| "no preview stored".to_string())?;
    // Logical -> physical. Selection is relative to this monitor's overlay.
    let px = (x.max(0.0) * geo.scale as f64).round() as u32;
    let py = (y.max(0.0) * geo.scale as f64).round() as u32;
    let pw = (w * geo.scale as f64).round() as u32;
    let ph = (h * geo.scale as f64).round() as u32;
    let cropped = crop_png(&png, px, py, pw, ph)?;
    *st.capture.lock().map_err(|e| e.to_string())? = Some(cropped);
    close_overlays(&app);
    open_editor(&app)?;
    Ok(())
}

#[tauri::command]
fn cancel_capture(app: AppHandle) {
    close_overlays(&app);
}

/// Capture all monitors merged into one image (handles negative offsets).
#[tauri::command]
fn capture_fullscreen(app: AppHandle) -> Result<(), String> {
    let monitors = xcap::Monitor::all().map_err(|e| e.to_string())?;
    if monitors.is_empty() {
        return Err("no monitors found".to_string());
    }
    let mut min_x = i32::MAX;
    let mut min_y = i32::MAX;
    let mut max_x = i32::MIN;
    let mut max_y = i32::MIN;
    let mut shots: Vec<(i32, i32, RgbaImage)> = Vec::new();
    for m in &monitors {
        let img = m.capture_image().map_err(|e| e.to_string())?;
        let (x, y) = (
            m.x().map_err(|e| e.to_string())?,
            m.y().map_err(|e| e.to_string())?,
        );
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x + img.width() as i32);
        max_y = max_y.max(y + img.height() as i32);
        shots.push((x, y, img));
    }
    let mut canvas = RgbaImage::new((max_x - min_x) as u32, (max_y - min_y) as u32);
    for (x, y, img) in &shots {
        image::imageops::overlay(&mut canvas, img, (x - min_x) as i64, (y - min_y) as i64);
    }
    let png = png_bytes(&canvas)?;
    *app.state::<AppState>()
        .capture
        .lock()
        .map_err(|e| e.to_string())? = Some(png);
    open_editor(&app)?;
    Ok(())
}

#[derive(Serialize)]
struct WindowInfo {
    id: u32,
    title: String,
    app_name: String,
    width: u32,
    height: u32,
    is_minimized: bool,
}

#[tauri::command]
fn list_windows() -> Result<Vec<WindowInfo>, String> {
    let mut out = Vec::new();
    for w in xcap::Window::all().map_err(|e| e.to_string())? {
        let title = w.title().unwrap_or_default();
        if title.trim().is_empty() {
            continue;
        }
        out.push(WindowInfo {
            id: w.id().map_err(|e| e.to_string())?,
            title,
            app_name: w.app_name().unwrap_or_default(),
            width: w.width().unwrap_or(0),
            height: w.height().unwrap_or(0),
            is_minimized: w.is_minimized().unwrap_or(false),
        });
    }
    Ok(out)
}

#[tauri::command]
fn capture_window(app: AppHandle, id: u32) -> Result<(), String> {
    let win = xcap::Window::all()
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|w| w.id().map(|i| i == id).unwrap_or(false))
        .ok_or_else(|| "window not found".to_string())?;
    let img = win.capture_image().map_err(|e| e.to_string())?;
    let png = png_bytes(&img)?;
    *app.state::<AppState>()
        .capture
        .lock()
        .map_err(|e| e.to_string())? = Some(png);
    open_editor(&app)?;
    Ok(())
}

/// The editor calls this on load to fetch the latest capture.
#[tauri::command]
fn take_capture(state: tauri::State<'_, AppState>) -> Result<Option<String>, String> {
    let lock = state.capture.lock().map_err(|e| e.to_string())?;
    Ok(lock.as_ref().map(|png| data_url(png)))
}

/// Copy an edited PNG (data URL from the editor canvas) to the clipboard.
#[tauri::command]
fn copy_data_url(data_url: String) -> Result<(), String> {
    let png = png_from_data_url(&data_url)?;
    let img = image::load_from_memory(&png)
        .map_err(|e| e.to_string())?
        .to_rgba8();
    let (w, h) = img.dimensions();
    let mut cb = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    cb.set_image(arboard::ImageData {
        width: w as usize,
        height: h as usize,
        bytes: std::borrow::Cow::Owned(img.into_raw()),
    })
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Save an edited PNG (data URL from the editor canvas) to disk.
#[tauri::command]
fn save_data_url(data_url: String, path: String) -> Result<(), String> {
    let png = png_from_data_url(&data_url)?;
    // Trust but verify: only write real PNG bytes.
    let _ = image::load_from_memory(&png).map_err(|e| e.to_string())?;
    std::fs::write(&path, &png).map_err(|e| e.to_string())?;
    Ok(())
}

/// Open a frameless always-on-top window showing the image.
#[tauri::command]
fn pin_data_url(app: AppHandle, data_url: String) -> Result<(), String> {
    let png = png_from_data_url(&data_url)?;
    let img = image::load_from_memory(&png).map_err(|e| e.to_string())?;
    let (w, h) = img.dimensions();
    // Cap the pin window at a sane size, keep aspect.
    let max = 640.0;
    let scale = (max / w as f64).min(max / h as f64).min(1.0);
    let (ww, hh) = ((w as f64 * scale).round(), (h as f64 * scale).round());
    if let Some(prev) = app.get_webview_window("pin") {
        let _ = prev.close();
    }
    let win = WebviewWindowBuilder::new(&app, "pin", WebviewUrl::App("pin.html".into()))
        .title("FullFrame Pin")
        .transparent(false)
        .decorations(false)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(true)
        .inner_size(ww, hh)
        .position(80.0, 80.0)
        .visible(true)
        .build()
        .map_err(|e| e.to_string())?;
    let _ = win.emit("pin-image", serde_json::json!({ "data_url": data_url }));
    Ok(())
}

#[tauri::command]
fn close_pin(app: AppHandle) {
    if let Some(w) = app.get_webview_window("pin") {
        let _ = w.close();
    }
}

// ----------------------------------------------------------------- app ---

fn build_tray(app: &AppHandle) -> Result<(), String> {
    let region = MenuItem::with_id(app, "tray-region", "Capture region", true, None::<&str>)
        .map_err(|e| e.to_string())?;
    let full = MenuItem::with_id(app, "tray-full", "Capture fullscreen", true, None::<&str>)
        .map_err(|e| e.to_string())?;
    let windows = MenuItem::with_id(app, "tray-windows", "Capture window…", true, None::<&str>)
        .map_err(|e| e.to_string())?;
    let open = MenuItem::with_id(app, "tray-open", "Open FullFrame", true, None::<&str>)
        .map_err(|e| e.to_string())?;
    let quit = MenuItem::with_id(app, "tray-quit", "Quit", true, None::<&str>)
        .map_err(|e| e.to_string())?;
    let menu = Menu::with_items(app, &[&region, &full, &windows, &open, &quit])
        .map_err(|e| e.to_string())?;

    // Icon: decode the bundled PNG to raw RGBA for the tray.
    let icon_bytes: &[u8] = include_bytes!("../icons/icon.png");
    let icon_rgba = image::load_from_memory(icon_bytes)
        .map_err(|e| e.to_string())?
        .to_rgba8();
    let (iw, ih) = icon_rgba.dimensions();
    let icon = TauriImage::new_owned(icon_rgba.into_raw(), iw, ih);

    TrayIconBuilder::new()
        .menu(&menu)
        .icon(icon)
        .tooltip("FullFrame")
        .on_menu_event(|app, event| match event.id.as_ref() {
            "tray-region" => {
                let _ = begin_region_capture(app.clone());
            }
            "tray-full" => {
                let _ = capture_fullscreen(app.clone());
            }
            "tray-windows" | "tray-open" => {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.show();
                    let _ = w.set_focus();
                }
            }
            "tray-quit" => app.exit(0),
            _ => {}
        })
        .build(app)
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn main() {
    tauri::Builder::default()
        .manage(AppState::default())
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            begin_region_capture,
            finish_region_capture,
            cancel_capture,
            capture_fullscreen,
            list_windows,
            capture_window,
            take_capture,
            copy_data_url,
            save_data_url,
            pin_data_url,
            close_pin,
        ])
        .setup(|app| {
            build_tray(app.handle())?;

            // Global hotkeys. PrintScreen = region overlay,
            // Ctrl+PrintScreen = fullscreen. (Wayland: not portable; the tray
            // menu is the fallback — see docs/PLAN.md.)
            let region_sc = Shortcut::new(Some(Modifiers::empty()), Code::PrintScreen);
            app.global_shortcut()
                .on_shortcut(region_sc, |app, _shortcut, event| {
                    if event.state == ShortcutState::Pressed {
                        let _ = begin_region_capture(app.clone());
                    }
                })?;
            let full_sc = Shortcut::new(Some(Modifiers::CONTROL), Code::PrintScreen);
            app.global_shortcut()
                .on_shortcut(full_sc, |app, _shortcut, event| {
                    if event.state == ShortcutState::Pressed {
                        let _ = capture_fullscreen(app.clone());
                    }
                })?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running FullFrame");
}
