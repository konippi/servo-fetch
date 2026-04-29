//! Screenshot capture — viewport or full-page PNG rendering via Servo.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use dpi::PhysicalSize;
use euclid::{Box2D, Point2D};
use image::RgbaImage;
use servo::{DevicePixel, WebView, WebViewRect};

use crate::bridge::{SPIN_INTERVAL, eval_js};
use servo_fetch::layout;

/// Capture a PNG screenshot of the page, temporarily resizing the viewport
/// to the full content size when `full_page` is set.
pub(crate) fn capture(
    servo: &servo::Servo,
    webview: &WebView,
    full_page: bool,
    timeout_secs: u64,
) -> Option<RgbaImage> {
    /// 16,384 matches the GPU texture limit on most modern hardware and caps
    /// the RGBA framebuffer at ~1 GB.
    const MAX_PIXELS: u32 = 16_384;

    let viewport = PhysicalSize::new(layout::VIEWPORT_WIDTH, layout::VIEWPORT_HEIGHT);

    if !full_page {
        return take_screenshot(servo, webview, None, timeout_secs);
    }

    let Some(measured) = measure_full_page(servo, webview) else {
        eprintln!("warning: failed to measure full page size; falling back to viewport screenshot");
        return take_screenshot(servo, webview, None, timeout_secs);
    };

    let Some(resized) = resolve_full_page_size(measured, viewport, MAX_PIXELS) else {
        // Content already fits in the viewport; skip the resize round-trip.
        return take_screenshot(servo, webview, None, timeout_secs);
    };

    if resized != measured {
        eprintln!(
            "warning: full-page dimensions clamped to {}x{} (content was {}x{})",
            resized.width, resized.height, measured.width, measured.height
        );
    }

    // Resize the viewport for capture, restoring it via a guard so the engine
    // stays usable even if `take_screenshot` panics or times out.
    let _restore = ViewportRestore {
        webview,
        size: viewport,
    };
    webview.resize(resized);
    take_screenshot(servo, webview, Some(device_rect(resized)), timeout_secs)
}

/// RAII guard that restores the `WebView`'s viewport size on drop.
struct ViewportRestore<'a> {
    webview: &'a WebView,
    size: PhysicalSize<u32>,
}

impl Drop for ViewportRestore<'_> {
    fn drop(&mut self) {
        self.webview.resize(self.size);
    }
}

/// Invoke `WebView::take_screenshot` synchronously by spinning the event loop
/// until the callback fires or the deadline elapses.
fn take_screenshot(
    servo: &servo::Servo,
    webview: &WebView,
    rect: Option<WebViewRect>,
    timeout_secs: u64,
) -> Option<RgbaImage> {
    let result: Rc<RefCell<Option<Result<RgbaImage, servo::ScreenshotCaptureError>>>> = Rc::new(RefCell::new(None));
    let cb_result = result.clone();
    webview.take_screenshot(rect, move |r| {
        *cb_result.borrow_mut() = Some(r);
    });

    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    loop {
        servo.spin_event_loop();
        if let Some(outcome) = result.borrow_mut().take() {
            return outcome
                .inspect_err(|e| eprintln!("warning: screenshot capture failed: {e:?}"))
                .ok();
        }
        if Instant::now() > deadline {
            eprintln!("warning: screenshot capture timed out after {timeout_secs}s");
            return None;
        }
        std::thread::sleep(SPIN_INTERVAL);
    }
}

#[expect(clippy::cast_precision_loss, reason = "dimensions stay well below 2^23")]
fn device_rect(size: PhysicalSize<u32>) -> WebViewRect {
    let rect = Box2D::<f32, DevicePixel>::new(
        Point2D::new(0.0, 0.0),
        Point2D::new(size.width as f32, size.height as f32),
    );
    WebViewRect::Device(rect)
}

/// Return the clamped size to resize the viewport to for a full-page capture,
/// or `None` if the measured content already fits inside the viewport.
fn resolve_full_page_size(
    measured: PhysicalSize<u32>,
    viewport: PhysicalSize<u32>,
    max_pixels: u32,
) -> Option<PhysicalSize<u32>> {
    if measured.width <= viewport.width && measured.height <= viewport.height {
        return None;
    }
    Some(PhysicalSize::new(
        measured.width.clamp(viewport.width, max_pixels),
        measured.height.clamp(viewport.height, max_pixels),
    ))
}

/// Read the full scrollable content size via JS, saturating at [`u32::MAX`].
fn measure_full_page(servo: &servo::Servo, webview: &WebView) -> Option<PhysicalSize<u32>> {
    const SIZE_JS: &str = r"
        JSON.stringify({
            w: Math.max(document.body.scrollWidth, document.documentElement.scrollWidth),
            h: Math.max(document.body.scrollHeight, document.documentElement.scrollHeight)
        })
    ";
    #[derive(serde::Deserialize)]
    struct Size {
        w: f64,
        h: f64,
    }
    let raw = eval_js(servo, webview, SIZE_JS).ok()?;
    let size: Size = serde_json::from_str(&raw).ok()?;

    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "saturating cast is the intended behavior"
    )]
    let width = size.w as u32;
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "saturating cast is the intended behavior"
    )]
    let height = size.h as u32;
    Some(PhysicalSize::new(width, height))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn size(w: u32, h: u32) -> PhysicalSize<u32> {
        PhysicalSize::new(w, h)
    }

    #[test]
    fn resolve_full_page_skips_when_content_fits_viewport() {
        let vp = size(1280, 800);
        assert!(resolve_full_page_size(size(1000, 600), vp, 16_384).is_none());
        assert!(resolve_full_page_size(size(1280, 800), vp, 16_384).is_none());
    }

    #[test]
    fn resolve_full_page_expands_when_taller_than_viewport() {
        let vp = size(1280, 800);
        assert_eq!(
            resolve_full_page_size(size(1280, 4000), vp, 16_384),
            Some(size(1280, 4000)),
        );
    }

    #[test]
    fn resolve_full_page_clamps_to_max_pixels() {
        let vp = size(1280, 800);
        // Height exceeds the cap; width is left untouched.
        assert_eq!(
            resolve_full_page_size(size(1280, 50_000), vp, 16_384),
            Some(size(1280, 16_384)),
        );
        // Both axes exceed the cap.
        assert_eq!(
            resolve_full_page_size(size(32_000, 50_000), vp, 16_384),
            Some(size(16_384, 16_384)),
        );
    }

    #[test]
    fn resolve_full_page_never_shrinks_below_viewport() {
        let vp = size(1280, 800);
        // Narrow content must still fill the viewport width.
        assert_eq!(
            resolve_full_page_size(size(400, 4000), vp, 16_384),
            Some(size(1280, 4000)),
        );
    }
}
