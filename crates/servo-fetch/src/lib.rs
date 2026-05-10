//! Fetch, render, and extract web content as Markdown, JSON, or screenshots with an embedded Servo browser engine.
//! No Chromium, no containers, no external processes.
//!
//! ```no_run
//! let md = servo_fetch::markdown("https://example.com")?;
//! # Ok::<(), servo_fetch::Error>(())
//! ```

#![deny(unsafe_code)]

pub mod extract;
pub mod sanitize;

pub(crate) mod bridge;
pub(crate) mod crawl;
pub(crate) mod error;
pub(crate) mod fetch;
pub(crate) mod layout;
pub(crate) mod map;
pub(crate) mod net;
pub(crate) mod pdf;
pub(crate) mod robots;
pub(crate) mod runtime;
pub(crate) mod scope;
pub(crate) mod screenshot;
pub(crate) mod sys;

pub use crawl::{CrawlError, CrawlOptions, CrawlPage, CrawlResult, crawl, crawl_each};
pub use error::{Error, Result};
pub use fetch::{ConsoleLevel, ConsoleMessage, FetchOptions, Page, extract_json, fetch, markdown, text};
pub use map::{MapOptions, MappedUrl, map};
pub use net::{NetworkPolicy, validate_url};

/// Set the network policy. Must be called at most once, before any engine use.
pub fn init(policy: NetworkPolicy) {
    bridge::set_engine_policy(policy);
}
