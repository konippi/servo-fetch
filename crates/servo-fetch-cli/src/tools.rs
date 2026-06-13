//! Shared tool implementations used by the MCP server, the HTTP API, and the RPC server.

mod common;
mod crawl;
mod error;
mod fetch;
pub(crate) mod limits;
mod map;
mod screenshot;

pub(crate) use common::{extract, paginate, validated_url};
pub(crate) use crawl::{CrawlOptions, crawl_pages};
pub(crate) use error::{ToolError, ToolResult};
pub(crate) use fetch::{batch_fetch_pages, fetch_js, fetch_page, fetch_with};
pub(crate) use map::{discover_urls, map_with};
pub(crate) use screenshot::take_screenshot;
