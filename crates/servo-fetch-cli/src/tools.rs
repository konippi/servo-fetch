//! Shared tool implementations used by both the MCP server and the HTTP API.

mod common;
mod crawl;
mod error;
mod fetch;
mod map;
mod screenshot;

pub(crate) use common::{extract, paginate, validated_url};
pub(crate) use crawl::{CrawlOptions, crawl_pages};
pub(crate) use error::{ToolError, ToolResult};
pub(crate) use fetch::{batch_fetch_pages, fetch_js, fetch_page};
pub(crate) use map::discover_urls;
pub(crate) use screenshot::take_screenshot;
