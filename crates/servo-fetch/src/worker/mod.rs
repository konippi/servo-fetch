//! Process-local implementation of the isolated worker protocol.
mod protocol;
#[cfg(test)]
mod tests;
mod wire;
pub use protocol::run_worker_stdio;

use crate::error::Error;
pub(crate) const MAX_WORKER_FRAME_BYTES: usize = 64 * 1024 * 1024;
const MAX_WORKER_REQUEST_FRAME_BYTES: usize = 8 * 1024 * 1024;
const WORKER_PROTOCOL_MAGIC: [u8; 8] = *b"SFETCHW\0";
const MAX_WORKER_PROTOCOL_INFO_BYTES: usize = 4 * 1024;
const MAX_WORKER_BLOB_CHUNK_BYTES: usize = 1024 * 1024;
fn worker_error(source: impl Into<crate::error::BoxError>) -> Error {
    Error::WorkerUnavailable { source: source.into() }
}
