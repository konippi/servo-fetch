//! Progress UI for long-running commands.

use std::io::{self, IsTerminal as _};
use std::time::Duration;

use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};

/// Mirror indicatif's built-in 20 Hz default; the `/dev/tty` (`term_like`)
/// target has no default of its own.
const REFRESH_HZ: u8 = 20;
/// Spinner frame interval — indicatif's documented spinner cadence.
const TICK: Duration = Duration::from_millis(100);

/// Spinner for a single step (e.g. fetching one URL).
#[must_use]
pub(crate) fn spinner(message: String) -> ProgressBar {
    let bar = make(None, "{spinner} {msg}");
    bar.set_message(message);
    bar
}

/// Bounded bar for a known item count (e.g. a batch of URLs).
#[must_use]
pub(crate) fn bar(total: u64) -> ProgressBar {
    make(Some(total), "{spinner} [{pos}/{len}] {msg}")
}

/// Unbounded counter for an open-ended stream (e.g. a crawl).
#[must_use]
pub(crate) fn counter() -> ProgressBar {
    make(None, "{spinner} {pos} crawled {msg}")
}

/// Build a steady-ticking progress bar on the controlling-terminal draw target.
fn make(len: Option<u64>, template: &str) -> ProgressBar {
    let bar = ProgressBar::with_draw_target(len, draw_target());
    bar.set_style(ProgressStyle::with_template(template).expect("static progress template is valid"));
    bar.enable_steady_tick(TICK);
    bar
}

/// A draw target on the controlling terminal, or a hidden one when none is attached.
fn draw_target() -> ProgressDrawTarget {
    if !io::stderr().is_terminal() {
        return ProgressDrawTarget::hidden();
    }
    #[cfg(unix)]
    if let Some(target) = controlling_terminal() {
        return target;
    }
    ProgressDrawTarget::stderr_with_hz(REFRESH_HZ)
}

/// Draw to `/dev/tty` — independent of fd 1 (stdout) and fd 2 (stderr).
#[cfg(unix)]
fn controlling_terminal() -> Option<ProgressDrawTarget> {
    let tty = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .ok()?;
    let term = console::Term::read_write_pair(tty.try_clone().ok()?, tty);
    Some(ProgressDrawTarget::term_like_with_hz(Box::new(term), REFRESH_HZ))
}
