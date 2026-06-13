//! NDJSON framing over stdio — one JSON-RPC message per line.

use tokio::io::{AsyncWriteExt as _, Stdin, Stdout};
use tokio::sync::mpsc::UnboundedReceiver;
use tokio_util::codec::{FramedRead, LinesCodec};

/// Reject oversized inbound frames before they exhaust memory.
const MAX_FRAME_BYTES: usize = 64 * 1024 * 1024;

/// A newline-delimited reader over stdin.
pub(crate) fn frame_reader(stdin: Stdin) -> FramedRead<Stdin, LinesCodec> {
    FramedRead::new(stdin, LinesCodec::new_with_max_length(MAX_FRAME_BYTES))
}

/// Write outbound frames to stdout in order (single writer, no interleaving).
pub(crate) async fn write_loop(mut stdout: Stdout, mut rx: UnboundedReceiver<String>) {
    while let Some(line) = rx.recv().await {
        if stdout.write_all(line.as_bytes()).await.is_err()
            || stdout.write_all(b"\n").await.is_err()
            || stdout.flush().await.is_err()
        {
            break;
        }
    }
}
