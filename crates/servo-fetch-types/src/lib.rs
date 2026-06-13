//! Wire DTOs shared by the servo-fetch CLI, servers, daemon, and language
//! bindings — the single source of truth for the wire format.

mod request;
mod wire;

pub use request::*;
pub use wire::*;
