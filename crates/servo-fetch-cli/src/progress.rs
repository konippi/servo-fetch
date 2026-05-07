//! Terminal progress UI written directly to stderr.

use std::io::{IsTerminal as _, Write as _};

/// TTY-aware reporter for long-running commands.
pub(crate) struct Progress {
    is_tty: bool,
}

impl Progress {
    pub(crate) fn new() -> Self {
        Self {
            is_tty: std::io::stderr().is_terminal(),
        }
    }

    pub(crate) fn header(&self, msg: &str) {
        if self.is_tty {
            let _ = writeln!(std::io::stderr(), "{msg}");
        }
    }

    pub(crate) fn ticker(&self, msg: &str) {
        if self.is_tty {
            let mut out = std::io::stderr();
            let _ = write!(out, "{msg}");
            let _ = out.flush();
        }
    }

    pub(crate) fn clear(&self) {
        if self.is_tty {
            let mut err = std::io::stderr();
            let _ = write!(err, "\r\x1b[2K");
            let _ = err.flush();
        }
    }

    pub(crate) fn item_done(&self, index: usize, total: Option<usize>, url: &str, ok: bool) {
        if !self.is_tty {
            return;
        }
        let mark = if ok { "✓" } else { "✗" };
        let _ = match total {
            Some(total) => writeln!(std::io::stderr(), "[{index}/{total}] {url} {mark}"),
            None => writeln!(std::io::stderr(), "[{index}] {url} {mark}"),
        };
    }
}
