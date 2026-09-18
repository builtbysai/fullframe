//! Screen capture tuned for recording.
//!
//! Screenshots can afford xcap's convenience; recording cannot: xcap opens a
//! new X connection per frame and converts pixels one at a time. On Linux we
//! keep one persistent x11rb connection and bulk-convert BGRA->RGBA. Other
//! platforms use xcap (single native call per frame there) with monitor
//! geometries cached once instead of re-queried every frame.

use image::RgbaImage;

pub trait FrameCapturer: Send {
    /// Capture the full virtual desktop stitched into one image.
    fn capture(&mut self) -> Result<RgbaImage, String>;
    /// Stitched desktop size in physical pixels.
    fn desktop_size(&self) -> (u32, u32);
    /// Desktop physical coordinates of the canvas top-left (may be negative
    /// with multi-monitor layouts).
    fn origin(&self) -> (i32, i32);
}

pub fn create_capturer() -> Result<Box<dyn FrameCapturer>, String> {
    #[cfg(target_os = "linux")]
    {
        linux::X11Capturer::new_boxed()
    }
    #[cfg(not(target_os = "linux"))]
    {
        generic::XcapCapturer::new_boxed()
    }
}

/// Bulk BGRA(X)->RGBA blit of one monitor into the stitched canvas.
fn blit_bgra(
    src: &[u8],
    canvas: &mut RgbaImage,
    dst_x: u32,
    dst_y: u32,
    w: u32,
    h: u32,
) -> Result<(), String> {
    let (cw, ch) = canvas.dimensions();
    if dst_x + w > cw || dst_y + h > ch {
        return Err("monitor outside canvas".to_string());
    }
    if src.len() < (w * h * 4) as usize {
        return Err(format!("short frame: {} < {}", src.len(), w * h * 4));
    }
    let canvas_raw = canvas.as_mut();
    let dst_stride = cw as usize * 4;
    for row in 0..h as usize {
        let s = &src[row * w as usize * 4..(row + 1) * w as usize * 4];
        let d_off = (dst_y as usize + row) * dst_stride + dst_x as usize * 4;
        let d = &mut canvas_raw[d_off..d_off + w as usize * 4];
        for (sp, dp) in s.chunks_exact(4).zip(d.chunks_exact_mut(4)) {
            dp[0] = sp[2];
            dp[1] = sp[1];
            dp[2] = sp[0];
            dp[3] = 255;
        }
    }
    Ok(())
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{ConnectionExt, Drawable, ImageFormat};
    use x11rb::rust_connection::RustConnection;

    pub struct X11Capturer {
        conn: RustConnection,
        root: Drawable,
        monitors: Vec<(i32, i32, u32, u32)>,
        min_x: i32,
        min_y: i32,
        width: u32,
        height: u32,
    }

    impl X11Capturer {
        pub fn new_boxed() -> Result<Box<dyn FrameCapturer>, String> {
            Ok(Box::new(Self::new()?))
        }

        fn new() -> Result<Self, String> {
            // Geometries come from xcap once, so the recording uses the same
            // coordinate space as the screenshot path.
            let xc_monitors = xcap::Monitor::all().map_err(|e| e.to_string())?;
            if xc_monitors.is_empty() {
                return Err("no monitors found".to_string());
            }
            let mut monitors = Vec::new();
            let (mut min_x, mut min_y) = (i32::MAX, i32::MAX);
            let (mut max_x, mut max_y) = (i32::MIN, i32::MIN);
            for m in &xc_monitors {
                let (x, y) = (
                    m.x().map_err(|e| e.to_string())?,
                    m.y().map_err(|e| e.to_string())?,
                );
                let (w, h) = (
                    m.width().map_err(|e| e.to_string())?,
                    m.height().map_err(|e| e.to_string())?,
                );
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x + w as i32);
                max_y = max_y.max(y + h as i32);
                monitors.push((x, y, w, h));
            }
            let (conn, screen_num) = x11rb::connect(None).map_err(|e| e.to_string())?;
            let screen = conn.setup().roots[screen_num].clone();
            Ok(Self {
                conn,
                root: screen.root,
                monitors,
                min_x,
                min_y,
                width: (max_x - min_x) as u32,
                height: (max_y - min_y) as u32,
            })
        }
    }

    impl FrameCapturer for X11Capturer {
        fn capture(&mut self) -> Result<RgbaImage, String> {
            let mut canvas = RgbaImage::new(self.width, self.height);
            for (x, y, w, h) in self.monitors.clone() {
                let reply = self
                    .conn
                    .get_image(
                        ImageFormat::Z_PIXMAP,
                        self.root,
                        x as i16,
                        y as i16,
                        w as u16,
                        h as u16,
                        u32::MAX,
                    )
                    .map_err(|e| format!("get_image: {e}"))?
                    .reply()
                    .map_err(|e| format!("get_image reply: {e}"))?;
                // Depth 24/32 on little-endian X is BGRX/BGRA 32bpp.
                if !cfg!(target_endian = "little") {
                    return Err("big-endian X11 capture not supported".to_string());
                }
                if !matches!(reply.depth, 24 | 32) {
                    return Err(format!("unsupported X depth {}", reply.depth));
                }
                blit_bgra(
                    reply.data.as_slice(),
                    &mut canvas,
                    (x - self.min_x) as u32,
                    (y - self.min_y) as u32,
                    w,
                    h,
                )?;
            }
            Ok(canvas)
        }

        fn desktop_size(&self) -> (u32, u32) {
            (self.width, self.height)
        }

        fn origin(&self) -> (i32, i32) {
            (self.min_x, self.min_y)
        }
    }
}

#[cfg(not(target_os = "linux"))]
mod generic {
    use super::*;

    pub struct XcapCapturer {
        monitors: Vec<xcap::Monitor>,
        width: u32,
        height: u32,
        min_x: i32,
        min_y: i32,
    }

    impl XcapCapturer {
        pub fn new_boxed() -> Result<Box<dyn FrameCapturer>, String> {
            let monitors = xcap::Monitor::all().map_err(|e| e.to_string())?;
            if monitors.is_empty() {
                return Err("no monitors found".to_string());
            }
            let (mut min_x, mut min_y) = (i32::MAX, i32::MAX);
            let (mut max_x, mut max_y) = (i32::MIN, i32::MIN);
            for m in &monitors {
                let (x, y) = (
                    m.x().map_err(|e| e.to_string())?,
                    m.y().map_err(|e| e.to_string())?,
                );
                let img_w = m.width().map_err(|e| e.to_string())?;
                let img_h = m.height().map_err(|e| e.to_string())?;
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x + img_w as i32);
                max_y = max_y.max(y + img_h as i32);
            }
            Ok(Box::new(Self {
                monitors,
                width: (max_x - min_x) as u32,
                height: (max_y - min_y) as u32,
                min_x,
                min_y,
            }))
        }
    }

    impl FrameCapturer for XcapCapturer {
        fn capture(&mut self) -> Result<RgbaImage, String> {
            let mut canvas = RgbaImage::new(self.width, self.height);
            for m in &self.monitors {
                let img = m.capture_image().map_err(|e| e.to_string())?;
                let (x, y) = (
                    m.x().map_err(|e| e.to_string())?,
                    m.y().map_err(|e| e.to_string())?,
                );
                image::imageops::overlay(
                    &mut canvas,
                    &img,
                    (x - self.min_x) as i64,
                    (y - self.min_y) as i64,
                );
            }
            Ok(canvas)
        }

        fn desktop_size(&self) -> (u32, u32) {
            (self.width, self.height)
        }

        fn origin(&self) -> (i32, i32) {
            (self.min_x, self.min_y)
        }
    }
}
