//! Screen recording: Tauri commands, recording state, indicator window.
//!
//! Flow: `start_recording` (fullscreen) or `begin_region_record` (overlay
//! selection, then `finish_region_capture` in main.rs hands the region back
//! here) -> pipeline records to a temp `.mp4` -> `stop_recording` finalizes
//! and returns the temp path -> frontend shows a save dialog -> `save_recording`
//! moves it to the chosen location, or `delete_recording` discards it.

pub mod capture;
pub mod encoder;
pub mod encoder_cpu;
pub mod mp4_muxer;
pub mod pipeline;

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

struct ActiveRecording {
    handle: pipeline::PipelineHandle,
    #[allow(dead_code)]
    temp_path: PathBuf,
}

#[derive(Default)]
pub struct RecordingManager {
    inner: Mutex<Option<ActiveRecording>>,
}

impl RecordingManager {
    pub fn is_recording(&self) -> bool {
        self.inner.lock().map(|g| g.is_some()).unwrap_or(false)
    }
    pub fn elapsed_ms(&self) -> u64 {
        self.inner
            .lock()
            .ok()
            .and_then(|g| g.as_ref().map(|r| r.handle.elapsed().as_millis() as u64))
            .unwrap_or(0)
    }
}

fn temp_output() -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("fullframe-recording-{stamp}.mp4"))
}

/// Start the pipeline, open the floating indicator. Called from commands and
/// from `finish_region_capture` (region path).
pub fn begin(app: &AppHandle, source: pipeline::RecordSource, fps: u32) -> Result<(), String> {
    {
        let mgr = app.state::<RecordingManager>();
        if mgr.is_recording() {
            return Err("already recording".to_string());
        }
    }
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.hide();
    }
    let temp = temp_output();
    let handle = match pipeline::start_pipeline(pipeline::PipelineConfig {
        source,
        fps: fps.clamp(5, 60),
        output: temp.clone(),
    }) {
        Ok(h) => h,
        Err(e) => {
            // Don't leave the user with no visible UI if startup failed.
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
            }
            return Err(e);
        }
    };
    {
        let mgr = app.state::<RecordingManager>();
        *mgr.inner.lock().map_err(|e| e.to_string())? = Some(ActiveRecording {
            handle,
            temp_path: temp,
        });
    }
    open_indicator(app)?;
    let _ = app.emit("recording_started", serde_json::json!({}));
    Ok(())
}

fn open_indicator(app: &AppHandle) -> Result<(), String> {
    if app.get_webview_window("recorder").is_some() {
        return Ok(());
    }
    let win = WebviewWindowBuilder::new(app, "recorder", WebviewUrl::App("recording.html".into()))
        .title("FullFrame recording")
        .inner_size(248.0, 84.0)
        .resizable(false)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .build()
        .map_err(|e| e.to_string())?;
    // Bottom-right of the primary monitor, 24px margin.
    // Note: the actual rendered height may exceed the requested 84px
    // (WM minimums, content), so leave room for up to 220px.
    if let Ok(Some(mon)) = app.primary_monitor() {
        let scale = mon.scale_factor().max(0.5);
        let pos = mon.position();
        let size = mon.size();
        let mw = size.width as f64 / scale;
        let mh = size.height as f64 / scale;
        let mx = pos.x as f64 / scale;
        let my = pos.y as f64 / scale;
        let _ = win.set_position(tauri::Position::Logical(tauri::LogicalPosition {
            x: mx + mw - 248.0 - 24.0,
            y: my + mh - 220.0 - 24.0,
        }));
    }
    Ok(())
}

/// Start a fullscreen recording.
#[tauri::command]
pub fn start_recording(app: AppHandle, fps: Option<u32>) -> Result<(), String> {
    begin(&app, pipeline::RecordSource::Fullscreen, fps.unwrap_or(15))
}

/// Stop the pipeline and return the temp `.mp4` path.
///
/// The floating indicator is NOT closed here: its JavaScript shows the save
/// dialog after this returns and closes itself when the user is done. Closing
/// it here would destroy the webview before the dialog opens.
#[tauri::command]
pub fn stop_recording(app: AppHandle) -> Result<String, String> {
    let rec = {
        let mgr = app.state::<RecordingManager>();
        let taken = mgr.inner.lock().map_err(|e| e.to_string())?.take();
        taken
    };
    let rec = rec.ok_or_else(|| "not recording".to_string())?;
    let result = rec.handle.stop_and_wait();
    // Always restore the main window, even if finalization failed: the user
    // must never be left with no visible UI.
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
    }
    let path = result?;
    let _ = app.emit(
        "recording_stopped",
        serde_json::json!({ "path": path.to_string_lossy() }),
    );
    Ok(path.to_string_lossy().into_owned())
}

/// Move the temp recording to its final location (after the save dialog).
/// Refuses to move anything outside the temp dir.
#[tauri::command]
pub fn save_recording(src: String, dest: String) -> Result<(), String> {
    let src = PathBuf::from(src);
    if !src.starts_with(std::env::temp_dir()) {
        return Err("refusing to move a file from outside the temp dir".to_string());
    }
    if std::fs::rename(&src, &dest).is_err() {
        std::fs::copy(&src, &dest).map_err(|e| format!("copy failed: {e}"))?;
        let _ = std::fs::remove_file(&src);
    }
    Ok(())
}

/// Discard a temp recording (user cancelled the save dialog).
#[tauri::command]
pub fn delete_recording(path: String) -> Result<(), String> {
    let path = PathBuf::from(path);
    if !path.starts_with(std::env::temp_dir()) {
        return Err("refusing to delete a file outside the temp dir".to_string());
    }
    let _ = std::fs::remove_file(&path);
    Ok(())
}

#[tauri::command]
pub fn recording_status(app: AppHandle) -> serde_json::Value {
    let mgr = app.state::<RecordingManager>();
    serde_json::json!({
        "recording": mgr.is_recording(),
        "elapsed_ms": mgr.elapsed_ms(),
    })
}
