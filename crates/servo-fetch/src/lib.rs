//! Fetch, render, and extract web content with an embedded Servo browser engine.
//! No Chrome, no containers, no external processes.
//!
//! ```no_run
//! let md = servo_fetch::markdown("https://example.com")?;
//! # Ok::<(), servo_fetch::Error>(())
//! ```

#![deny(unsafe_code)]

pub mod engine;
pub mod error;
pub mod extract;
pub mod layout;
pub mod sanitize;

pub(crate) mod bridge;
pub(crate) mod crawl;
pub(crate) mod net;
pub(crate) mod pdf;
pub(crate) mod runtime;
pub(crate) mod screenshot;
pub(crate) mod sys;

pub use engine::{
    ConsoleLevel, ConsoleMessage, CrawlError, CrawlOptions, CrawlPage, CrawlResult, CrawlStatus, FetchOptions, Page,
    crawl, crawl_each, extract_json, fetch, markdown, text, validate_url,
};
pub use error::{Error, Result};
