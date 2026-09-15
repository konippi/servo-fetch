//! Blocking API mirror of the top-level async API.
//!
//! These functions block the calling thread and panic when called from within an async runtime;
//! call them from a plain thread or [`tokio::task::spawn_blocking`], or use the async API instead.

mod client;

pub use client::{Client, ClientBuilder};

pub use crate::crawl::{crawl_blocking as crawl, crawl_each_blocking as crawl_each};
pub use crate::fetch::{
    extract_json_blocking as extract_json, fetch_blocking as fetch, markdown_blocking as markdown,
    text_blocking as text,
};
pub use crate::map::map_blocking as map;
