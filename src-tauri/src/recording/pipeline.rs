//! Screen-recording pipeline: capture thread -> bounded channel -> encode thread.
//!
//! The capture thread paces itself at the target FPS with `xcap` and drops
//! frames when the encoder can't keep up (bounded channel of 4). The encode
//! thread turns frames into H.264 packets and muxes a finished `.mp4` when
//! the capture thread exits.

use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crossbeam_channel::bounded;
use image::RgbaImage;

use super::capture::create_capturer;
use super::encoder::{create_encoder, EncodedPacket, VideoEncoder};
use super::mp4_muxer::{self, MuxInput, MuxSample, TIMESCALE};

/// What to record, in desktop physical pixels (same space as screenshots).
#[derive(Debug, Clone)]
pub enum RecordSource {
    /// All monitors stitched, like a fullscreen screenshot.
    Fullscreen,
    /// Fixed rectangle; may span monitors.
    Region { x: i32, y: i32, w: u32, h: u32 },
}

pub struct PipelineConfig {
    pub source: RecordSource,
    pub fps: u32,
    pub output: PathBuf,
}

struct TimestampedFrame {
    image: RgbaImage,
    pts_ms: f64,
}

pub struct PipelineHandle {
    stop: Arc<AtomicBool>,
    started: Instant,
    capture: Option<JoinHandle<()>>,
    encode: Option<JoinHandle<Result<PathBuf, String>>>,
}

impl PipelineHandle {
    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }

    /// Signal stop, wait for both threads, return the written file.
    pub fn stop_and_wait(mut self) -> Result<PathBuf, String> {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.capture.take() {
            let _ = h.join();
        }
        match self.encode.take() {
            Some(h) => h
                .join()
                .map_err(|_| "encoder thread panicked".to_string())?,
            None => Err("no encoder thread".to_string()),
        }
    }
}

/// Crop a desktop image to the recording region (clamped, 2px-aligned).
fn crop_region(
    desktop: &RgbaImage,
    origin: (i32, i32),
    r: (i32, i32, u32, u32),
) -> Option<RgbaImage> {
    let (ox, oy) = origin;
    let (rx, ry, rw, rh) = r;
    let x = (rx - ox).max(0) as u32;
    let y = (ry - oy).max(0) as u32;
    let w = rw.min(desktop.width().saturating_sub(x)) & !1;
    let h = rh.min(desktop.height().saturating_sub(y)) & !1;
    if w < 16 || h < 16 {
        return None;
    }
    Some(image::imageops::crop_imm(desktop, x, y, w, h).to_image())
}

fn capture_loop(
    source: RecordSource,
    fps: u32,
    stop: Arc<AtomicBool>,
    sender: crossbeam_channel::Sender<TimestampedFrame>,
) {
    let mut capturer = match create_capturer() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("recording: capturer init failed: {e}");
            return;
        }
    };
    let origin = capturer.origin();
    let interval = Duration::from_secs_f64(1.0 / fps as f64);
    let start = Instant::now();
    let mut n: u64 = 0;
    let mut sent = 0u32;
    let mut dropped = 0u32;
    loop {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        // Pace: next frame deadline. If we fall more than one interval behind,
        // resync instead of accumulating lag.
        let deadline = start + Duration::from_secs_f64((n + 1) as f64 / fps as f64);
        let now = Instant::now();
        if now < deadline {
            std::thread::sleep(deadline - now);
        } else if now > deadline + interval {
            // Too far behind; resync the clock to now.
            n = (start.elapsed().as_secs_f64() * fps as f64) as u64;
        }
        n += 1;

        let frame = match capturer.capture() {
            Ok(desktop) => match &source {
                RecordSource::Fullscreen => desktop,
                RecordSource::Region { x, y, w, h } => {
                    match crop_region(&desktop, origin, (*x, *y, *w, *h)) {
                        Some(c) => c,
                        None => continue,
                    }
                }
            },
            Err(e) => {
                eprintln!("recording: capture failed: {e}");
                continue;
            }
        };
        // Encoder needs even dimensions.
        let (fw, fh) = (frame.width() & !1, frame.height() & !1);
        let frame = if fw != frame.width() || fh != frame.height() {
            image::imageops::crop_imm(&frame, 0, 0, fw, fh).to_image()
        } else {
            frame
        };
        let pts_ms = start.elapsed().as_secs_f64() * 1000.0;
        match sender.try_send(TimestampedFrame {
            image: frame,
            pts_ms,
        }) {
            Ok(()) => sent += 1,
            Err(_) => dropped += 1,
        }
    }
    eprintln!("recording: capture loop ended (sent={sent} dropped={dropped})");
}

/// Strip SPS/PPS/SEI-only concerns: keep slice NALs for mdat, drop SPS/PPS
/// (they live in avcC). Returns None if nothing slice-like remains.
fn sample_nals(packet: &EncodedPacket) -> Vec<Vec<u8>> {
    packet
        .nal_units
        .iter()
        .filter(|n| !n.is_empty() && !matches!(n[0] & 0x1F, 7 | 8))
        .cloned()
        .collect()
}

fn encode_loop(
    receiver: crossbeam_channel::Receiver<TimestampedFrame>,
    fps: u32,
    output: PathBuf,
) -> Result<PathBuf, String> {
    let mut encoder: Option<Box<dyn VideoEncoder>> = None;
    let mut samples: Vec<MuxSample> = Vec::new();
    let mut pending: Option<(EncodedPacket, f64)> = None; // packet awaiting duration
    let nominal_90k = (TIMESCALE as f64 / fps as f64).round() as u32;

    // Force the first frame to be a keyframe so SPS/PPS exist immediately.
    let mut first = true;

    for frame in receiver {
        if encoder.is_none() {
            let mut enc = create_encoder(frame.image.width(), frame.image.height(), fps)?;
            enc.request_keyframe();
            encoder = Some(enc);
        }
        let enc = encoder.as_mut().unwrap();
        if first {
            enc.request_keyframe();
            first = false;
        }
        let packet = enc.encode_frame(&frame.image.into_raw(), frame.pts_ms)?;

        // The previous packet's duration is now known.
        if let Some((prev_packet, prev_pts_ms)) = pending.take() {
            let dt_ms = (frame.pts_ms - prev_pts_ms).clamp(1.0, 1000.0);
            let duration_90k = (dt_ms * 90.0).round() as u32;
            let nals = sample_nals(&prev_packet);
            if !nals.is_empty() {
                samples.push(MuxSample {
                    avcc: mp4_muxer::nals_to_avcc(&nals),
                    duration_90k,
                    is_keyframe: prev_packet.is_keyframe,
                });
            }
        }
        pending = Some((packet, frame.pts_ms));
    }
    // Last packet gets the nominal duration.
    if let Some((packet, _)) = pending.take() {
        let nals = sample_nals(&packet);
        if !nals.is_empty() {
            samples.push(MuxSample {
                avcc: mp4_muxer::nals_to_avcc(&nals),
                duration_90k: nominal_90k,
                is_keyframe: packet.is_keyframe,
            });
        }
    }
    // Stopping before the first frame arrived: no encoder exists yet.
    let Some(enc) = encoder.as_mut() else {
        return Err("no frames were captured".to_string());
    };
    for p in enc.flush()? {
        let nals = sample_nals(&p);
        if !nals.is_empty() {
            samples.push(MuxSample {
                avcc: mp4_muxer::nals_to_avcc(&nals),
                duration_90k: nominal_90k,
                is_keyframe: p.is_keyframe,
            });
        }
    }

    if samples.is_empty() {
        return Err("no frames were captured".to_string());
    }
    let enc = encoder
        .as_ref()
        .ok_or_else(|| "no frames were captured".to_string())?;
    let (sps, pps) = enc
        .codec_config()
        .ok_or_else(|| "encoder produced no SPS/PPS".to_string())?;
    let mp4 = mp4_muxer::write_mp4(&MuxInput {
        width: enc.width(),
        height: enc.height(),
        sps,
        pps,
        samples,
    })?;
    std::fs::write(&output, &mp4).map_err(|e| format!("write failed: {e}"))?;
    Ok(output)
}

pub fn start_pipeline(config: PipelineConfig) -> Result<PipelineHandle, String> {
    let fps = config.fps.clamp(5, 60);
    let stop = Arc::new(AtomicBool::new(false));
    let (sender, receiver) = bounded::<TimestampedFrame>(4);

    let cap_stop = stop.clone();
    let source = config.source.clone();
    let capture = std::thread::Builder::new()
        .name("ff-record-capture".into())
        .spawn(move || capture_loop(source, fps, cap_stop, sender))
        .map_err(|e| format!("spawn capture thread: {e}"))?;

    let output = config.output.clone();
    let encode = std::thread::Builder::new()
        .name("ff-record-encode".into())
        .spawn(move || encode_loop(receiver, fps, output))
        .map_err(|e| format!("spawn encoder thread: {e}"))?;

    Ok(PipelineHandle {
        stop,
        started: Instant::now(),
        capture: Some(capture),
        encode: Some(encode),
    })
}
