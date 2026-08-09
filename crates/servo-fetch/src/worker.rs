//! Process-local implementation of the isolated worker protocol.
pub(crate) mod protocol;
#[cfg(test)]
mod tests;
pub(crate) mod wire;
pub use protocol::run_worker_stdio;

use crate::error::Error;
pub(crate) const MAX_WORKER_FRAME_BYTES: usize = 64 * 1024 * 1024;
pub(crate) const MAX_WORKER_REQUEST_FRAME_BYTES: usize = 8 * 1024 * 1024;
pub(crate) const WORKER_PROTOCOL_MAGIC: [u8; 8] = *b"SFETCHW\0";
pub(crate) const MAX_WORKER_PROTOCOL_INFO_BYTES: usize = 4 * 1024;
pub(crate) const MAX_WORKER_BLOB_CHUNK_BYTES: usize = 1024 * 1024;
pub(crate) const PARENT_LIFELINE_FD_ENV: &str = "SERVO_FETCH_INTERNAL_PARENT_LIFELINE_FD";
pub(crate) fn worker_error(source: impl Into<crate::error::BoxError>) -> Error {
    Error::WorkerUnavailable { source: source.into() }
}
