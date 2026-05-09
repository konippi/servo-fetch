//! Fetch, render, and extract web content with an embedded Servo browser engine.
//! No Chrome, no containers, no external processes.
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
pub(crate) mod engine;
pub(crate) mod error;
pub(crate) mod layout;
pub(crate) mod map;
pub(crate) mod net;
pub(crate) mod pdf;
pub(crate) mod robots;
pub(crate) mod runtime;
pub(crate) mod scope;
pub(crate) mod screenshot;
pub(crate) mod sys;

pub use engine::{
    ConsoleLevel, ConsoleMessage, CrawlError, CrawlOptions, CrawlPage, CrawlResult, FetchOptions, MapOptions,
    MappedUrl, Page, crawl, crawl_each, extract_json, fetch, init, map, markdown, text, validate_url,
};
pub use error::{Error, Result};
pub use net::NetworkPolicy;
