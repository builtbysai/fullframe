//! Software H.264 encoder via OpenH264 (Cisco, BSD-2).
//!
//! Used on every platform in v1. The bundled OpenH264 source compiles in via
//! the `source` feature — no network fetch, no system libraries.

use openh264::encoder::{
    BitRate, Encoder, EncoderConfig, FrameRate, FrameType, IntraFramePeriod, UsageType,
};
use openh264::formats::{RgbaSliceU8, YUVBuffer};
use openh264::{OpenH264API, Timestamp};

use super::encoder::{EncodedPacket, VideoEncoder};

pub struct CpuEncoder {
    enc: Encoder,
    width: u32,
    height: u32,
    sps: Option<Vec<u8>>,
    pps: Option<Vec<u8>>,
}

impl CpuEncoder {
    pub fn new(width: u32, height: u32, fps: u32) -> Result<Self, String> {
        // H.264 needs even dimensions; shave a pixel rather than fail.
        let (w, h) = (width & !1, height & !1);
        if w < 16 || h < 16 {
            return Err(format!("recording region too small: {width}x{height}"));
        }
        let fps = fps.clamp(5, 60);
        // ~0.12 bits per pixel per frame, clamped to a sane range.
        let bps = ((w as u64 * h as u64 * fps as u64 * 12) / 100).clamp(400_000, 16_000_000);
        let config = EncoderConfig::new()
            .bitrate(BitRate::from_bps(bps as u32))
            .max_frame_rate(FrameRate::from_hz(fps as f32))
            .usage_type(UsageType::ScreenContentRealTime)
            .intra_frame_period(IntraFramePeriod::from_num_frames((fps * 2).max(30)))
            .skip_frames(false);
        let enc = Encoder::with_api_config(OpenH264API::from_source(), config)
            .map_err(|e| format!("openh264 init failed: {e}"))?;
        Ok(Self {
            enc,
            width: w,
            height: h,
            sps: None,
            pps: None,
        })
    }
}

impl VideoEncoder for CpuEncoder {
    fn encode_frame(&mut self, rgba: &[u8], pts_ms: f64) -> Result<EncodedPacket, String> {
        let expect = self.width as usize * self.height as usize * 4;
        if rgba.len() != expect {
            return Err(format!(
                "frame size {} != {}x{} RGBA",
                rgba.len(),
                self.width,
                self.height
            ));
        }
        let src = RgbaSliceU8::new(rgba, (self.width as usize, self.height as usize));
        let yuv = YUVBuffer::from_rgba8_source(src);
        let ts = Timestamp::from_millis(pts_ms.max(0.0) as u64);
        let stream = self
            .enc
            .encode_at(&yuv, ts)
            .map_err(|e| format!("encode failed: {e}"))?;
        let is_key = matches!(stream.frame_type(), FrameType::IDR | FrameType::I);
        let mut nals = Vec::new();
        for li in 0..stream.num_layers() {
            let layer = stream.layer(li).ok_or_else(|| format!("no layer {li}"))?;
            for ni in 0..layer.nal_count() {
                let nal = layer.nal_unit(ni).ok_or_else(|| format!("no nal {ni}"))?;
                // openh264 prefixes a 4-byte start code; strip it for raw NALs.
                let raw = nal.strip_prefix(&[0, 0, 0, 1]).unwrap_or(nal);
                if raw.is_empty() {
                    continue;
                }
                match raw[0] & 0x1F {
                    7 if self.sps.is_none() => self.sps = Some(raw.to_vec()),
                    8 if self.pps.is_none() => self.pps = Some(raw.to_vec()),
                    _ => {}
                }
                nals.push(raw.to_vec());
            }
        }
        Ok(EncodedPacket {
            nal_units: nals,
            is_keyframe: is_key,
        })
    }

    fn flush(&mut self) -> Result<Vec<EncodedPacket>, String> {
        // OpenH264 emits every frame synchronously in encode_at; nothing delayed.
        Ok(Vec::new())
    }

    fn request_keyframe(&mut self) {
        self.enc.force_intra_frame();
    }

    fn codec_config(&self) -> Option<(Vec<u8>, Vec<u8>)> {
        match (&self.sps, &self.pps) {
            (Some(s), Some(p)) => Some((s.clone(), p.clone())),
            _ => None,
        }
    }

    fn width(&self) -> u32 {
        self.width
    }
    fn height(&self) -> u32 {
        self.height
    }
}
