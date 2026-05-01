//! Screenshot tool helper.

use base64::Engine as _;
use rmcp::model::{CallToolResult, Content};

use crate::bridge;

use super::fetch::fetch_page;

pub(crate) async fn take_screenshot(
    url: &str,
    timeout: u64,
    settle_ms: u64,
    full_page: bool,
) -> Result<CallToolResult, String> {
    let page = fetch_page(url, timeout, settle_ms, bridge::FetchMode::Screenshot { full_page }).await?;
    let img = page.screenshot.ok_or_else(|| "screenshot capture failed".to_string())?;

    let mut buf = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;

    Ok(CallToolResult::success(vec![Content::image(
        base64::engine::general_purpose::STANDARD.encode(&buf),
        "image/png",
    )]))
}
