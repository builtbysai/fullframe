//! Video encoding for screen recording.
//!
//! The [`VideoEncoder`] trait is the seam where platform hardware encoders
//! (Media Foundation on Windows, VideoToolbox on macOS) can slot in later.
//! v1 ships the software backend (`encoder_cpu`, OpenH264) on every platform:
//! it is fully testable here and good enough for screen content.

/// One encoded video access unit: H.264 NAL units plus metadata.
pub struct EncodedPacket {
    /// Raw NAL units (RBSP with NAL header byte, no start codes).
    pub nal_units: Vec<Vec<u8>>,
    /// True when the packet holds an IDR slice: independently decodable.
    pub is_keyframe: bool,
}

pub trait VideoEncoder: Send {
    /// Encode one RGBA frame (`width*height*4` bytes). Dimensions must be
    /// even and identical for every call.
    fn encode_frame(&mut self, rgba: &[u8], pts_ms: f64) -> Result<EncodedPacket, String>;
    /// Drain delayed frames after the last input frame.
    fn flush(&mut self) -> Result<Vec<EncodedPacket>, String>;
    /// Ask the encoder to make the next frame a keyframe.
    fn request_keyframe(&mut self);
    /// Raw SPS/PPS NAL payloads (no start codes), for the MP4 `avcC` box.
    /// `None` until the first keyframe has been encoded.
    fn codec_config(&self) -> Option<(Vec<u8>, Vec<u8>)>;
    fn width(&self) -> u32;
    fn height(&self) -> u32;
}

/// Build the v1 encoder: software OpenH264 on all platforms.
pub fn create_encoder(width: u32, height: u32, fps: u32) -> Result<Box<dyn VideoEncoder>, String> {
    Ok(Box::new(super::encoder_cpu::CpuEncoder::new(
        width, height, fps,
    )?))
}
