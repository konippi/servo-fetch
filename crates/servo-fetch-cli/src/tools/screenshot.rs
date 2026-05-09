//! Screenshot tool helper.

use super::error::{ToolError, ToolResult};
use super::fetch::fetch_screenshot;

pub(crate) async fn take_screenshot(url: &str, timeout: u64, settle_ms: u64, full_page: bool) -> ToolResult<Vec<u8>> {
    let page = fetch_screenshot(url, full_page, timeout, settle_ms).await?;
    page.screenshot_png()
        .map(<[u8]>::to_vec)
        .ok_or_else(|| ToolError::internal("screenshot capture failed"))
}
