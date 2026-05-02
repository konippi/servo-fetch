//! Screenshot tool helper.

use base64::Engine as _;
use rmcp::model::{CallToolResult, Content};

use super::fetch::fetch_screenshot;

pub(crate) async fn take_screenshot(
    url: &str,
    timeout: u64,
    settle_ms: u64,
    full_page: bool,
) -> Result<CallToolResult, String> {
    let page = fetch_screenshot(url, full_page, timeout, settle_ms).await?;
    let png = page
        .screenshot_png()
        .ok_or_else(|| "screenshot capture failed".to_string())?;

    Ok(CallToolResult::success(vec![Content::image(
        base64::engine::general_purpose::STANDARD.encode(png),
        "image/png",
    )]))
}
