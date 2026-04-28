//! Web content extraction library powered by Servo and Readability.
//!
//! This crate provides utilities for extracting readable content from HTML:
//!
//! - [`extract`] — Convert HTML into Markdown or structured JSON using
//!   Mozilla's Readability algorithm.
//! - [`layout`] — CSS layout heuristics to detect and strip navbars,
//!   sidebars, and footers before extraction.
//! - [`sanitize`] — Strip ANSI escape sequences and control characters
//!   from output strings.

#![forbid(unsafe_code)]

pub mod extract;
pub mod layout;
pub mod sanitize;
