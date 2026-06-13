//! Stdio JSON-RPC server that keeps the Servo engine warm for language bindings.

mod dispatch;
mod protocol;
mod transport;

use std::any::Any;
use std::collections::HashMap;
use std::panic::AssertUnwindSafe;
use std::sync::{Arc, Mutex};

use futures_util::{FutureExt as _, StreamExt as _};
use protocol::{CancelParams, ErrorData, Incoming, RequestId, Response, ResponseError, code};
use serde_json::Value;
use servo_fetch_types::ErrorKind;
use tokio::sync::mpsc::{self, UnboundedSender};
use tokio_util::codec::LinesCodecError;
use tokio_util::sync::CancellationToken;

type Cancellations = Arc<Mutex<HashMap<RequestId, CancellationToken>>>;

/// Serve JSON-RPC over stdio until stdin closes or an `exit` notification arrives.
pub(crate) async fn run() -> anyhow::Result<()> {
    let (tx, rx) = mpsc::unbounded_channel::<String>();
    let writer = tokio::spawn(transport::write_loop(tokio::io::stdout(), rx));
    let cancels: Cancellations = Arc::new(Mutex::new(HashMap::new()));
    let mut reader = transport::frame_reader(tokio::io::stdin());

    while let Some(frame) = reader.next().await {
        let line = match frame {
            Ok(line) => line,
            Err(LinesCodecError::MaxLineLengthExceeded) => {
                // FramedRead ends the stream after a decode error, so report and stop.
                let _ = tx.send(error_line(
                    code::INVALID_REQUEST,
                    ErrorKind::ParseError,
                    "frame exceeds maximum length",
                ));
                break;
            }
            Err(LinesCodecError::Io(e)) => {
                tracing::warn!("rpc transport error: {e}");
                break;
            }
        };
        if line.trim().is_empty() {
            continue;
        }
        let incoming: Incoming = match serde_json::from_str(&line) {
            Ok(msg) => msg,
            Err(e) => {
                let _ = tx.send(error_line(code::PARSE_ERROR, ErrorKind::ParseError, &e.to_string()));
                continue;
            }
        };
        match incoming.id {
            Some(id) => spawn_request(id, incoming.method, incoming.params, &tx, &cancels),
            None => match incoming.method.as_str() {
                "$/cancelRequest" => cancel(incoming.params, &cancels),
                "exit" => break,
                _ => {}
            },
        }
    }

    // EOF/exit: let in-flight requests finish, then drain.
    drop(tx);
    if let Err(e) = writer.await {
        tracing::error!("rpc writer task panicked: {e}");
    }
    Ok(())
}

fn spawn_request(id: RequestId, method: String, params: Value, tx: &UnboundedSender<String>, cancels: &Cancellations) {
    let token = CancellationToken::new();
    {
        let mut guard = cancels.lock().expect("cancellations poisoned");
        if guard.contains_key(&id) {
            if let Ok(line) = serde_json::to_string(&Response::failure(id, ResponseError::duplicate_id())) {
                let _ = tx.send(line);
            }
            return;
        }
        guard.insert(id.clone(), token.clone());
    }

    let tx = tx.clone();
    let cancels = cancels.clone();
    tokio::spawn(async move {
        let dispatched = AssertUnwindSafe(dispatch::dispatch(&method, params, &id, &tx)).catch_unwind();
        let outcome = tokio::select! {
            biased;
            () = token.cancelled() => Err(ResponseError::cancelled()),
            result = dispatched => result.unwrap_or_else(|payload| {
                tracing::error!("rpc handler `{method}` panicked: {}", panic_message(payload.as_ref()));
                Err(ResponseError::new(code::INTERNAL_ERROR, ErrorKind::Internal, "handler panicked"))
            }),
        };
        // Always deregister and answer exactly once, even on panic/cancel.
        cancels.lock().expect("cancellations poisoned").remove(&id);
        let response = match outcome {
            Ok(result) => Response::success(id, result),
            Err(error) => Response::failure(id, error),
        };
        if let Ok(line) = serde_json::to_string(&response) {
            let _ = tx.send(line);
        }
    });
}

fn cancel(params: Value, cancels: &Cancellations) {
    if let Ok(params) = serde_json::from_value::<CancelParams>(params) {
        if let Some(token) = cancels.lock().expect("cancellations poisoned").get(&params.id) {
            token.cancel();
        }
    }
}

fn panic_message(payload: &(dyn Any + Send)) -> &str {
    payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("unknown panic payload")
}

fn error_line(code: i32, kind: ErrorKind, message: &str) -> String {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": Value::Null,
        "error": { "code": code, "message": message, "data": ErrorData { kind } },
    })
    .to_string()
}
