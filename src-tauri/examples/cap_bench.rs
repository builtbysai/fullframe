//! Benchmark xcap capture speed. Run under Xvfb (debug AND release).

fn main() {
    let monitors = xcap::Monitor::all().expect("monitors");
    println!("monitors: {}", monitors.len());
    let m = &monitors[0];
    // Warm up.
    let _ = m.capture_image().expect("warmup");
    let t = std::time::Instant::now();
    let n = 10;
    for _ in 0..n {
        let img = m.capture_image().expect("capture");
        std::hint::black_box(img);
    }
    let ms = t.elapsed().as_secs_f64() * 1000.0 / n as f64;
    println!("capture_image: {ms:.1} ms/frame");
}
