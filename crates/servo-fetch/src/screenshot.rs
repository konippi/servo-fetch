//! Screenshot capture — viewport or full-page PNG rendering via Servo.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

use dpi::PhysicalSize;
use euclid::{Box2D, Point2D};
use image::RgbaImage;
use servo::{DevicePixel, WebView, WebViewRect};

use crate::bridge::{eval_js, wait_for_wake};
use crate::layout;

/// Matches the GPU texture limit on most modern hardware and caps the RGBA framebuffer at ~1 GB.
const MAX_SCREENSHOT_DIMENSION: u32 = 16_384;
const MAX_FULL_PAGE_RESIZE_PASSES: usize = 3;
const SCENE_PROBE_SIZE: PhysicalSize<u32> = PhysicalSize::new(1, 1);

/// Capture a PNG screenshot of the page, temporarily resizing the viewport
/// to the full content size when `full_page` is set.
pub(crate) fn capture(
    servo: &servo::Servo,
    webview: &WebView,
    full_page: bool,
    deadline: Instant,
) -> Option<RgbaImage> {
    if !full_page {
        return take_screenshot(servo, webview, None, deadline);
    }

    capture_full_page(servo, webview, deadline)
}

fn capture_full_page(servo: &servo::Servo, webview: &WebView, deadline: Instant) -> Option<RgbaImage> {
    let viewport = PhysicalSize::new(layout::VIEWPORT_WIDTH, layout::VIEWPORT_HEIGHT);
    let Some(measured) = measure_full_page(servo, webview, deadline) else {
        tracing::warn!("failed to measure full page size; falling back to viewport screenshot");
        return take_screenshot(servo, webview, None, deadline);
    };
    let Some(mut capture_size) = resolve_full_page_size(measured, viewport, MAX_SCREENSHOT_DIMENSION) else {
        return take_screenshot(servo, webview, None, deadline);
    };
    warn_if_clamped(measured, capture_size);

    let _restore = ViewportRestore {
        webview,
        size: viewport,
    };

    for pass in 0..MAX_FULL_PAGE_RESIZE_PASSES {
        webview.resize(capture_size);
        wait_for_scene_update(servo, webview, deadline)?;

        let Some(measured) = measure_full_page(servo, webview, deadline) else {
            tracing::warn!("failed to remeasure full page after resize; capturing current geometry");
            break;
        };
        let resolved = resolve_full_page_size(measured, viewport, MAX_SCREENSHOT_DIMENSION).unwrap_or(viewport);
        warn_if_clamped(measured, resolved);
        let grown = grow_capture_size(capture_size, resolved);
        if grown == capture_size {
            break;
        }
        if pass + 1 == MAX_FULL_PAGE_RESIZE_PASSES {
            tracing::warn!(
                current_w = capture_size.width,
                current_h = capture_size.height,
                measured_w = measured.width,
                measured_h = measured.height,
                "full-page geometry did not stabilize within the resize limit",
            );
            break;
        }
        capture_size = grown;
    }

    take_screenshot(servo, webview, Some(device_rect(capture_size)), deadline)
}

/// Wait for Servo's rendered scene to catch up with the most recent resize.
fn wait_for_scene_update(servo: &servo::Servo, webview: &WebView, deadline: Instant) -> Option<()> {
    take_screenshot(servo, webview, Some(device_rect(SCENE_PROBE_SIZE)), deadline)?;
    Some(())
}

fn grow_capture_size(current: PhysicalSize<u32>, measured: PhysicalSize<u32>) -> PhysicalSize<u32> {
    PhysicalSize::new(current.width.max(measured.width), current.height.max(measured.height))
}

fn warn_if_clamped(measured: PhysicalSize<u32>, resolved: PhysicalSize<u32>) {
    if measured.width > MAX_SCREENSHOT_DIMENSION || measured.height > MAX_SCREENSHOT_DIMENSION {
        tracing::warn!(
            clamped_w = resolved.width,
            clamped_h = resolved.height,
            measured_w = measured.width,
            measured_h = measured.height,
            "full-page dimensions clamped",
        );
    }
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
    deadline: Instant,
) -> Option<RgbaImage> {
    let result: Rc<RefCell<Option<Result<RgbaImage, servo::ScreenshotCaptureError>>>> = Rc::new(RefCell::new(None));
    let cb_result = result.clone();
    webview.take_screenshot(rect, move |r| {
        *cb_result.borrow_mut() = Some(r);
    });

    loop {
        servo.spin_event_loop();
        if let Some(outcome) = result.borrow_mut().take() {
            return outcome
                .inspect_err(|e| tracing::warn!(error = ?e, "screenshot capture failed"))
                .ok();
        }
        let now = Instant::now();
        if now >= deadline {
            tracing::warn!("screenshot capture timed out");
            return None;
        }
        wait_for_wake(deadline.saturating_duration_since(now));
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
fn measure_full_page(servo: &servo::Servo, webview: &WebView, deadline: Instant) -> Option<PhysicalSize<u32>> {
    const SIZE_JS: &str = r"
        (() => {
            const root = document.documentElement;
            const body = document.body;
            const widths = [root.scrollWidth, root.offsetWidth, root.clientWidth];
            const heights = [root.scrollHeight, root.offsetHeight, root.clientHeight];
            if (body) {
                widths.push(body.scrollWidth, body.offsetWidth, body.clientWidth);
                heights.push(body.scrollHeight, body.offsetHeight, body.clientHeight);
            }
            return JSON.stringify({ w: Math.max(...widths), h: Math.max(...heights) });
        })()
    ";
    #[derive(serde::Deserialize)]
    struct Size {
        w: f64,
        h: f64,
    }
    let raw = eval_js(servo, webview, SIZE_JS, deadline).ok()?;
    let size: Size = serde_json::from_str(&raw).ok()?;
    Some(PhysicalSize::new(
        normalize_dimension(size.w),
        normalize_dimension(size.h),
    ))
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "finite non-negative values intentionally saturate to u32"
)]
fn normalize_dimension(value: f64) -> u32 {
    if !value.is_finite() || value <= 0.0 {
        return 0;
    }
    value.min(f64::from(u32::MAX)) as u32
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
    fn grow_capture_size_is_monotonic() {
        assert_eq!(grow_capture_size(size(1280, 2000), size(1400, 1800)), size(1400, 2000));
        assert_eq!(grow_capture_size(size(1280, 2000), size(1000, 1000)), size(1280, 2000));
    }

    #[test]
    fn normalize_dimension_rejects_invalid_values_and_saturates() {
        assert_eq!(normalize_dimension(f64::NAN), 0);
        assert_eq!(normalize_dimension(-1.0), 0);
        assert_eq!(normalize_dimension(42.9), 42);
        assert_eq!(normalize_dimension(f64::MAX), u32::MAX);
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
