//! Shared tool implementations used by the MCP server, the HTTP API, and the RPC server.

mod crawl;
mod error;
mod fetch;
pub(crate) mod limits;
mod map;
mod options;
mod render;

pub(crate) use crawl::{CrawlSpec, build_crawl_options, crawl_pages};
pub(crate) use error::ToolError;
pub(crate) use fetch::{BatchSpec, batch_fetch_pages, fetch_with};
pub(crate) use map::{MapSpec, build_map_options, map_with};
pub(crate) use options::{apply_options, validate_selector, validated_url, visibility_policy};
pub(crate) use render::{clamp_js_output, paginate, render_page};
