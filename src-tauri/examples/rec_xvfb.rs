//! Xvfb end-to-end test: animate the X root window with x11rb while the real
//! recording pipeline (xcap capture -> openh264 -> mp4 muxer) records it.
//! Run under Xvfb:  xvfb-run -s "-screen 0 1280x800x24" cargo run --example rec_xvfb

use fullframe::recording::pipeline::{start_pipeline, PipelineConfig, RecordSource};
use std::time::Duration;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::*;

fn main() {
    let (conn, screen_num) = x11rb::connect(None).expect("x11 connect");
    let screen = conn.setup().roots[screen_num].clone();
    let root = screen.root;
    let cmap = screen.default_colormap;

    // Allocate a few colors.
    let alloc = |r: u16, g: u16, b: u16| -> u32 {
        conn.alloc_color(cmap, r, g, b)
            .expect("alloc_color")
            .reply()
            .expect("reply")
            .pixel
    };
    let red = alloc(0xFFFF, 0, 0);
    let green = alloc(0, 0xFFFF, 0);
    let blue = alloc(0, 0, 0xFFFF);
    let white = screen.white_pixel;
    let black = screen.black_pixel;

    let gc = conn.generate_id().unwrap();
    conn.create_gc(
        gc,
        root,
        &CreateGCAux::new().foreground(black).background(white),
    )
    .unwrap();

    // Paint the background once.
    conn.poly_fill_rectangle(
        root,
        gc,
        &[Rectangle {
            x: 0,
            y: 0,
            width: 1280,
            height: 800,
        }],
    )
    .unwrap();
    conn.flush().unwrap();

    // Animate in a background thread: a bouncing box + color cycling.
    let anim = std::thread::spawn(move || {
        let colors = [red, green, blue, white];
        for i in 0..40 {
            let x = ((i * 37) % 1000) as i16;
            let c = colors[i % colors.len()];
            conn.change_gc(gc, &ChangeGCAux::new().foreground(c))
                .unwrap();
            conn.poly_fill_rectangle(
                root,
                gc,
                &[Rectangle {
                    x,
                    y: 300,
                    width: 160,
                    height: 160,
                }],
            )
            .unwrap();
            // Clear the previous box position with black every few frames so
            // motion is visible rather than a smear.
            if i % 4 == 3 {
                conn.change_gc(gc, &ChangeGCAux::new().foreground(black))
                    .unwrap();
                conn.poly_fill_rectangle(
                    root,
                    gc,
                    &[Rectangle {
                        x: 0,
                        y: 300,
                        width: 1280,
                        height: 160,
                    }],
                )
                .unwrap();
            }
            conn.flush().unwrap();
            std::thread::sleep(Duration::from_millis(150));
        }
    });

    let out = std::path::PathBuf::from("/tmp/fullframe-rec-xvfb.mp4");
    let _ = std::fs::remove_file(&out);
    let handle = start_pipeline(PipelineConfig {
        source: RecordSource::Fullscreen,
        fps: 10,
        output: out.clone(),
    })
    .expect("start_pipeline");
    std::thread::sleep(Duration::from_secs(6));
    let path = handle.stop_and_wait().expect("stop_and_wait");
    println!("recorded {}", path.display());
    anim.join().unwrap();

    // Verify with ffprobe + full decode.
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
            path.to_str().unwrap(),
        ])
        .output()
        .expect("ffprobe");
    let info = String::from_utf8_lossy(&probe.stdout);
    println!("--- ffprobe ---\n{info}");
    assert!(info.contains("codec_name=h264"));
    assert!(info.contains("width=1280") && info.contains("height=800"));

    let dec = std::process::Command::new("ffmpeg")
        .args([
            "-v",
            "error",
            "-i",
            path.to_str().unwrap(),
            "-f",
            "null",
            "-",
        ])
        .output()
        .expect("ffmpeg");
    assert!(dec.status.success(), "decode failed");
    println!("XVFB E2E PASS");
}
