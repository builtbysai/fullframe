//! End-to-end recording test: synthetic frames -> CpuEncoder -> MP4 muxer ->
//! ffprobe metadata + full ffmpeg decode. Run: cargo run --example rec_e2e

use fullframe::recording::encoder::VideoEncoder;
use fullframe::recording::encoder_cpu::CpuEncoder;
use fullframe::recording::mp4_muxer::{self, MuxInput, MuxSample};

fn main() {
    let (w, h, fps, nframes) = (320u32, 240u32, 10u32, 30u32);
    let mut enc = CpuEncoder::new(w, h, fps).expect("encoder");
    enc.request_keyframe();

    let mut samples: Vec<MuxSample> = Vec::new();
    let mut pending: Option<(Vec<Vec<u8>>, bool)> = None;
    let nominal_90k = (mp4_muxer::TIMESCALE / fps) as u32;

    for f in 0..nframes {
        // Moving vertical bars on a gradient: exercises real encoding.
        let mut rgba = vec![0u8; (w * h * 4) as usize];
        for y in 0..h {
            for x in 0..w {
                let i = ((y * w + x) * 4) as usize;
                let bar = ((x + f * 8) / 32) % 2;
                rgba[i] = if bar == 0 { (x * 255 / w) as u8 } else { 40 };
                rgba[i + 1] = (y * 255 / h) as u8;
                rgba[i + 2] = ((f * 8) % 256) as u8;
                rgba[i + 3] = 255;
            }
        }
        let pts = f as f64 * 100.0;
        let pkt = enc.encode_frame(&rgba, pts).expect("encode_frame");
        if let Some((prev_nals, prev_key)) = pending.take() {
            let nals: Vec<Vec<u8>> = prev_nals
                .into_iter()
                .filter(|n| !matches!(n[0] & 0x1F, 7 | 8))
                .collect();
            if !nals.is_empty() {
                samples.push(MuxSample {
                    avcc: mp4_muxer::nals_to_avcc(&nals),
                    duration_90k: nominal_90k,
                    is_keyframe: prev_key,
                });
            }
        }
        pending = Some((pkt.nal_units, pkt.is_keyframe));
    }
    if let Some((nals, key)) = pending.take() {
        let nals: Vec<Vec<u8>> = nals
            .into_iter()
            .filter(|n| !matches!(n[0] & 0x1F, 7 | 8))
            .collect();
        samples.push(MuxSample {
            avcc: mp4_muxer::nals_to_avcc(&nals),
            duration_90k: nominal_90k,
            is_keyframe: key,
        });
    }
    let (sps, pps) = enc.codec_config().expect("sps/pps");
    println!(
        "SPS {} bytes, PPS {} bytes, {} samples",
        sps.len(),
        pps.len(),
        samples.len()
    );

    let mp4 = mp4_muxer::write_mp4(&MuxInput {
        width: w,
        height: h,
        sps,
        pps,
        samples,
    })
    .expect("write_mp4");
    let path = "/tmp/fullframe-rec-e2e.mp4";
    std::fs::write(path, &mp4).expect("write file");
    println!("wrote {path} ({} bytes)", mp4.len());

    // ffprobe metadata.
    let probe = std::process::Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=codec_name,width,height,avg_frame_rate,duration,nb_frames",
            "-of",
            "default=noprint_wrappers=1",
            path,
        ])
        .output()
        .expect("ffprobe");
    let out = String::from_utf8_lossy(&probe.stdout);
    println!("--- ffprobe ---\n{out}");
    assert!(out.contains("codec_name=h264"), "not h264!");
    assert!(
        out.contains("width=320") && out.contains("height=240"),
        "wrong dims!"
    );

    // Full decode; any error fails the run.
    let dec = std::process::Command::new("ffmpeg")
        .args(["-v", "error", "-i", path, "-f", "null", "-"])
        .output()
        .expect("ffmpeg");
    assert!(
        dec.status.success(),
        "decode failed: {}",
        String::from_utf8_lossy(&dec.stderr)
    );
    println!("decode OK, stderr was clean");

    // Frame count via ffprobe read_intervals.
    let cnt = std::process::Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-count_frames",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=nb_read_frames",
            "-of",
            "csv=p=0",
            path,
        ])
        .output()
        .expect("ffprobe count");
    println!(
        "decoded frames: {}",
        String::from_utf8_lossy(&cnt.stdout).trim()
    );
    println!("E2E PASS");
}
