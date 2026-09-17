// Dev-only helper: capture the primary monitor to /tmp/xvfb-shot.png.
// Used for headless visual verification under Xvfb.
fn main() {
    let mons = xcap::Monitor::all().expect("no monitors");
    let img = mons[0].capture_image().expect("capture failed");
    img.save("/tmp/xvfb-shot.png").expect("save failed");
    println!("saved {}x{}", img.width(), img.height());
}
