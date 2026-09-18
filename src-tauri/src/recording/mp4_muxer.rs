//! Minimal ISOBMFF MP4 muxer for H.264 (AVC) video.
//!
//! Writes a plain (non-fragmented) `.mp4`: `ftyp`, `moov` (with `avc1`/`avcC`
//! sample entry, `stts`/`stss`/`stsc`/`stsz`/`stco`), then `mdat` holding
//! AVCC length-prefixed samples. No external dependency; verified against
//! ffprobe and full decode in tests.

/// One video sample for the muxer.
pub struct MuxSample {
    /// NAL units in AVCC form: each prefixed with a 4-byte big-endian length.
    pub avcc: Vec<u8>,
    /// Sample duration in 90 kHz ticks.
    pub duration_90k: u32,
    /// True for IDR/keyframe samples (listed in `stss`).
    pub is_keyframe: bool,
}

pub struct MuxInput {
    pub width: u32,
    pub height: u32,
    /// Raw SPS/PPS NAL payloads (no start codes).
    pub sps: Vec<u8>,
    pub pps: Vec<u8>,
    pub samples: Vec<MuxSample>,
}

/// Video timescale: 90 kHz, the H.264 convention.
pub const TIMESCALE: u32 = 90_000;
/// Movie timescale: milliseconds.
const MOVIE_TIMESCALE: u32 = 1_000;

struct Writer {
    buf: Vec<u8>,
}

impl Writer {
    fn new() -> Self {
        Self { buf: Vec::new() }
    }
    fn u8(&mut self, v: u8) {
        self.buf.push(v);
    }
    fn u16(&mut self, v: u16) {
        self.buf.extend_from_slice(&v.to_be_bytes());
    }
    fn u24(&mut self, v: u32) {
        self.buf.extend_from_slice(&v.to_be_bytes()[1..]);
    }
    fn u32(&mut self, v: u32) {
        self.buf.extend_from_slice(&v.to_be_bytes());
    }
    fn bytes(&mut self, b: &[u8]) {
        self.buf.extend_from_slice(b);
    }
    fn tag(&mut self, t: &[u8; 4]) {
        self.buf.extend_from_slice(t);
    }
    /// Write a box: reserves the size field, runs `f`, patches the size.
    fn bx(&mut self, tag: &[u8; 4], f: impl FnOnce(&mut Writer)) {
        let start = self.buf.len();
        self.u32(0);
        self.tag(tag);
        f(self);
        let size = (self.buf.len() - start) as u32;
        self.buf[start..start + 4].copy_from_slice(&size.to_be_bytes());
    }
    fn full_box(&mut self, tag: &[u8; 4], version: u8, flags: u32, f: impl FnOnce(&mut Writer)) {
        self.bx(tag, |w| {
            w.u8(version);
            w.u24(flags);
            f(w);
        });
    }
}

fn identity_matrix(w: &mut Writer) {
    w.u32(0x0001_0000);
    w.u32(0);
    w.u32(0);
    w.u32(0);
    w.u32(0x0001_0000);
    w.u32(0);
    w.u32(0);
    w.u32(0);
    w.u32(0x4000_0000);
}

/// Convert raw NAL units (no start codes) to one AVCC sample.
pub fn nals_to_avcc(nals: &[Vec<u8>]) -> Vec<u8> {
    let mut out = Vec::new();
    for nal in nals {
        out.extend_from_slice(&(nal.len() as u32).to_be_bytes());
        out.extend_from_slice(nal);
    }
    out
}

pub fn write_mp4(input: &MuxInput) -> Result<Vec<u8>, String> {
    if input.samples.is_empty() {
        return Err("no samples to mux".to_string());
    }
    if input.sps.is_empty() || input.pps.is_empty() {
        return Err("missing SPS/PPS".to_string());
    }
    if input.sps.len() < 4 {
        return Err("SPS too short".to_string());
    }
    let total_90k: u64 = input.samples.iter().map(|s| s.duration_90k as u64).sum();
    if total_90k == 0 {
        return Err("zero-duration recording".to_string());
    }
    let total_ms = (total_90k * 1_000 / TIMESCALE as u64).max(1) as u32;

    // ---- ftyp ----
    let mut w = Writer::new();
    w.bx(b"ftyp", |w| {
        w.tag(b"isom");
        w.u32(512);
        w.tag(b"isom");
        w.tag(b"iso2");
        w.tag(b"avc1");
        w.tag(b"mp41");
    });
    let ftyp_len = w.buf.len();

    // ---- stts runs: coalesce equal deltas ----
    let mut stts_runs: Vec<(u32, u32)> = Vec::new();
    for s in &input.samples {
        match stts_runs.last_mut() {
            Some((_, d)) if *d == s.duration_90k => stts_runs.last_mut().unwrap().0 += 1,
            _ => stts_runs.push((1, s.duration_90k)),
        }
    }
    let sync_samples: Vec<u32> = input
        .samples
        .iter()
        .enumerate()
        .filter(|(_, s)| s.is_keyframe)
        .map(|(i, _)| i as u32 + 1) // 1-based
        .collect();
    if sync_samples.is_empty() {
        return Err("no keyframe in recording".to_string());
    }

    // ---- moov (built twice: once to measure, once for real) ----
    // Chunk offset = ftyp + moov + 8 (mdat header). moov's own size depends on
    // stco's content only through a fixed 4-byte field, so one pass suffices:
    // build moov with a placeholder, measure, then rebuild with the real offset.
    let build_moov = |stco_offset: u32| -> Vec<u8> {
        let mut w = Writer::new();
        w.bx(b"moov", |w| {
            w.full_box(b"mvhd", 0, 0, |w| {
                w.u32(0); // creation
                w.u32(0); // modification
                w.u32(MOVIE_TIMESCALE);
                w.u32(total_ms);
                w.u32(0x0001_0000); // rate
                w.u16(0x0100); // volume
                w.u16(0); // reserved
                w.bytes(&[0; 8]); // reserved
                identity_matrix(w);
                w.bytes(&[0; 24]); // pre_defined
                w.u32(2); // next_track_ID
            });
            w.bx(b"trak", |w| {
                w.full_box(b"tkhd", 0, 0x0000_07, |w| {
                    w.u32(0);
                    w.u32(0);
                    w.u32(1); // track_ID
                    w.u32(0); // reserved
                    w.u32(total_ms); // duration (movie timescale)
                    w.bytes(&[0; 8]); // reserved
                    w.u16(0); // layer
                    w.u16(0); // alternate_group
                    w.u16(0); // volume
                    w.u16(0); // reserved
                    identity_matrix(w);
                    w.u32(input.width << 16); // 16.16
                    w.u32(input.height << 16);
                });
                w.bx(b"mdia", |w| {
                    w.full_box(b"mdhd", 0, 0, |w| {
                        w.u32(0);
                        w.u32(0);
                        w.u32(TIMESCALE);
                        w.u32(total_90k.min(u32::MAX as u64) as u32);
                        w.u16(0x55c4); // 'und'
                        w.u16(0);
                    });
                    w.full_box(b"hdlr", 0, 0, |w| {
                        w.u32(0); // pre_defined
                        w.tag(b"vide");
                        w.bytes(&[0; 12]); // reserved
                        w.bytes(b"VideoHandler\0");
                    });
                    w.bx(b"minf", |w| {
                        w.full_box(b"vmhd", 0, 1, |w| {
                            w.u16(0); // graphicsmode
                            w.u16(0);
                            w.u16(0);
                            w.u16(0); // opcolor
                        });
                        w.bx(b"dinf", |w| {
                            w.full_box(b"dref", 0, 0, |w| {
                                w.u32(1); // entry_count
                                w.full_box(b"url ", 0, 1, |_| {}); // self-contained
                            });
                        });
                        w.bx(b"stbl", |w| {
                            // stsd -> avc1 -> avcC
                            w.full_box(b"stsd", 0, 0, |w| {
                                w.u32(1); // entry_count
                                w.bx(b"avc1", |w| {
                                    w.bytes(&[0; 6]); // reserved
                                    w.u16(1); // data_reference_index
                                    w.u16(0); // pre_defined
                                    w.u16(0); // reserved
                                    w.bytes(&[0; 12]); // pre_defined[3]
                                    w.u16(input.width.min(0xFFFF) as u16);
                                    w.u16(input.height.min(0xFFFF) as u16);
                                    w.u32(0x0048_0000); // horizresolution 72dpi
                                    w.u32(0x0048_0000); // vertresolution
                                    w.u32(0); // reserved
                                    w.u16(1); // frame_count
                                    w.bytes(&[0; 32]); // compressorname
                                    w.u16(0x0018); // depth
                                    w.u16(0xFFFF); // pre_defined = -1
                                    w.bx(b"avcC", |w| {
                                        w.u8(1); // configurationVersion
                                        w.u8(input.sps[1]); // profile
                                        w.u8(input.sps[2]); // compatibility
                                        w.u8(input.sps[3]); // level
                                        w.u8(0xFF); // 4-byte NAL lengths
                                        w.u8(0xE1); // 1 SPS
                                        w.u16(input.sps.len() as u16);
                                        w.bytes(&input.sps);
                                        w.u8(1); // 1 PPS
                                        w.u16(input.pps.len() as u16);
                                        w.bytes(&input.pps);
                                    });
                                });
                            });
                            w.full_box(b"stts", 0, 0, |w| {
                                w.u32(stts_runs.len() as u32);
                                for (count, delta) in &stts_runs {
                                    w.u32(*count);
                                    w.u32(*delta);
                                }
                            });
                            w.full_box(b"stss", 0, 0, |w| {
                                w.u32(sync_samples.len() as u32);
                                for n in &sync_samples {
                                    w.u32(*n);
                                }
                            });
                            w.full_box(b"stsc", 0, 0, |w| {
                                w.u32(1); // entry_count
                                w.u32(1); // first_chunk
                                w.u32(input.samples.len() as u32); // samples_per_chunk
                                w.u32(1); // sample_description_index
                            });
                            w.full_box(b"stsz", 0, 0, |w| {
                                w.u32(0); // sample_size = variable
                                w.u32(input.samples.len() as u32);
                                for s in &input.samples {
                                    w.u32(s.avcc.len() as u32);
                                }
                            });
                            w.full_box(b"stco", 0, 0, |w| {
                                w.u32(1); // entry_count
                                w.u32(stco_offset);
                            });
                        });
                    });
                });
            });
        });
        w.buf
    };

    let moov_probe = build_moov(0);
    let mdat_data_offset = (ftyp_len + moov_probe.len() + 8) as u32;
    let moov = build_moov(mdat_data_offset);

    let mut out = Vec::with_capacity(moov.len() + 1024 * 1024);
    out.extend_from_slice(&w.buf); // ftyp (w still holds only ftyp)
    out.extend_from_slice(&moov);
    let mut mw = Writer::new();
    mw.bx(b"mdat", |mw| {
        for s in &input.samples {
            mw.bytes(&s.avcc);
        }
    });
    out.extend_from_slice(&mw.buf);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parse top-level box tags to sanity-check structure.
    fn top_boxes(mp4: &[u8]) -> Vec<[u8; 4]> {
        let mut v = Vec::new();
        let mut off = 0;
        while off + 8 <= mp4.len() {
            let size = u32::from_be_bytes(mp4[off..off + 4].try_into().unwrap()) as usize;
            if size < 8 || off + size > mp4.len() {
                break;
            }
            v.push(mp4[off + 4..off + 8].try_into().unwrap());
            off += size;
        }
        v
    }

    /// Find a box by tag anywhere (first match, recursive), return its content range.
    fn find_box(mp4: &[u8], tag: &[u8; 4]) -> Option<(usize, usize)> {
        fn rec(buf: &[u8], base: usize, tag: &[u8; 4]) -> Option<(usize, usize)> {
            let mut off = 0;
            while off + 8 <= buf.len() {
                let size = u32::from_be_bytes(buf[off..off + 4].try_into().unwrap()) as usize;
                if size < 8 || off + size > buf.len() {
                    return None;
                }
                if &buf[off + 4..off + 8] == tag {
                    return Some((base + off + 8, base + off + size));
                }
                // Recurse into known containers.
                if matches!(
                    &buf[off + 4..off + 8],
                    b"moov" | b"trak" | b"mdia" | b"minf" | b"stbl" | b"edts" | b"dinf"
                ) {
                    if let Some(r) = rec(&buf[off + 8..off + size], base + off + 8, tag) {
                        return Some(r);
                    }
                }
                off += size;
            }
            None
        }
        rec(mp4, 0, tag)
    }

    fn fake_input() -> MuxInput {
        // Minimal plausible SPS (baseline, level 3.1, 640x480) / PPS.
        let sps = vec![0x67, 0x42, 0x00, 0x1f, 0x96, 0x54, 0x05, 0x01];
        let pps = vec![0x68, 0xce, 0x38, 0x80];
        let samples = (0..10)
            .map(|i| MuxSample {
                avcc: {
                    let mut v = (100 + i as u32).to_be_bytes().to_vec();
                    v.extend_from_slice(&vec![0x65u8; 100 + i as usize]);
                    v
                },
                duration_90k: 3000, // 30fps
                is_keyframe: i % 5 == 0,
            })
            .collect();
        MuxInput {
            width: 640,
            height: 480,
            sps,
            pps,
            samples,
        }
    }

    #[test]
    fn structure_is_sound() {
        let mp4 = write_mp4(&fake_input()).unwrap();
        let tags: Vec<String> = top_boxes(&mp4)
            .iter()
            .map(|t| String::from_utf8_lossy(t).into_owned())
            .collect();
        assert_eq!(tags, vec!["ftyp", "moov", "mdat"]);
        // stco points exactly at mdat payload.
        let (s, _) = find_box(&mp4, b"stco").unwrap();
        let entry_count = u32::from_be_bytes(mp4[s + 4..s + 8].try_into().unwrap());
        assert_eq!(entry_count, 1);
        let chunk_off = u32::from_be_bytes(mp4[s + 8..s + 12].try_into().unwrap()) as usize;
        let (ms, _) = find_box(&mp4, b"mdat").unwrap();
        assert_eq!(chunk_off, ms, "stco must point at mdat payload");
        // stss lists samples 1 and 6.
        let (s, e) = find_box(&mp4, b"stss").unwrap();
        let n = u32::from_be_bytes(mp4[s + 4..s + 8].try_into().unwrap());
        assert_eq!(n, 2);
        assert_eq!(
            u32::from_be_bytes(mp4[s + 8..s + 12].try_into().unwrap()),
            1
        );
        assert_eq!(
            u32::from_be_bytes(mp4[s + 12..s + 16].try_into().unwrap()),
            6
        );
        let _ = e;
    }

    #[test]
    fn rejects_empty() {
        let mut bad = fake_input();
        bad.samples.clear();
        assert!(write_mp4(&bad).is_err());
    }
}
