//! Logging initialization for servo-fetch.
//!
//! Logs are always written to **stderr** so the MCP stdio transport can own
//! stdout for JSON-RPC messages (the MCP specification mandates this). The
//! filter is assembled from, in order of precedence: `RUST_LOG` if set, the
//! CLI `-v`/`-q` flags, or a conservative default.

use std::io::IsTerminal as _;

use tracing::level_filters::LevelFilter;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::time::Uptime;

/// Verbosity resolved from CLI flags.
///
/// Unlike a raw count this is an exhaustive enum so match arms can be
/// verified at compile time and the `RUST_LOG` override story stays
/// explicit.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Verbosity {
    /// `-q`/`--quiet`: only errors are shown.
    Quiet,
    /// Default: warnings and errors from `servo_fetch`.
    #[default]
    Default,
    /// `-v`: info and above from `servo_fetch`.
    Info,
    /// `-vv`: debug and above from `servo_fetch`.
    Debug,
    /// `-vvv`: trace and above from `servo_fetch`.
    Trace,
    /// `-vvvv`: trace and above from every crate (Servo internals included).
    TraceAll,
}

impl Verbosity {
    /// Resolve from `clap`'s `-v` count and `-q` flag.
    pub(crate) fn from_flags(verbose: u8, quiet: bool) -> Self {
        if quiet {
            return Self::Quiet;
        }
        match verbose {
            0 => Self::Default,
            1 => Self::Info,
            2 => Self::Debug,
            3 => Self::Trace,
            _ => Self::TraceAll,
        }
    }

    /// Default filter applied when `RUST_LOG` is unset.
    fn default_filter(self) -> &'static str {
        match self {
            Self::Quiet => "servo_fetch=error",
            Self::Default => "servo_fetch=warn",
            Self::Info => "servo_fetch=info",
            Self::Debug => "servo_fetch=debug",
            Self::Trace => "servo_fetch=trace",
            Self::TraceAll => "trace",
        }
    }

    /// Whether to include a relative timestamp and target with each log line.
    fn detailed(self) -> bool {
        matches!(self, Self::Debug | Self::Trace | Self::TraceAll)
    }
}

/// Install a global `tracing` subscriber.
///
/// Must be called exactly once, early in `main`. Further calls are no-ops
/// because we rely on `tracing`'s first-installer-wins semantics.
///
/// # Panics
/// Never. An invalid `RUST_LOG` directive is reported to stderr and the
/// resolved default filter is used instead, matching how `cargo` and `uv`
/// behave.
pub(crate) fn init(verbosity: Verbosity) {
    let filter = EnvFilter::builder()
        .with_default_directive(LevelFilter::WARN.into())
        .parse(verbosity.default_filter())
        .expect("default_filter is a valid EnvFilter directive");

    // `RUST_LOG` takes priority over the CLI-derived default so operators can
    // always override without recompiling or remembering our flag taxonomy.
    let filter = match std::env::var("RUST_LOG") {
        Ok(raw) if !raw.is_empty() => match EnvFilter::builder().parse(&raw) {
            Ok(from_env) => from_env,
            Err(err) => {
                eprintln!("warning: invalid RUST_LOG={raw:?}: {err}; falling back to CLI defaults");
                filter
            }
        },
        _ => filter,
    };

    let ansi = std::io::stderr().is_terminal();
    let builder = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_ansi(ansi)
        .with_level(true)
        .with_target(verbosity.detailed());

    // `try_init` converts "already installed" into a silent no-op, which is
    // the right default for library/test contexts that may set up their own
    // subscriber first.
    if verbosity.detailed() {
        let _ = builder.with_timer(Uptime::default()).try_init();
    } else {
        let _ = builder.without_time().try_init();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quiet_wins_over_verbose() {
        assert_eq!(Verbosity::from_flags(3, true), Verbosity::Quiet);
    }

    #[test]
    fn verbose_escalates() {
        assert_eq!(Verbosity::from_flags(0, false), Verbosity::Default);
        assert_eq!(Verbosity::from_flags(1, false), Verbosity::Info);
        assert_eq!(Verbosity::from_flags(2, false), Verbosity::Debug);
        assert_eq!(Verbosity::from_flags(3, false), Verbosity::Trace);
        assert_eq!(Verbosity::from_flags(4, false), Verbosity::TraceAll);
        assert_eq!(Verbosity::from_flags(42, false), Verbosity::TraceAll);
    }

    #[test]
    fn default_filter_is_warn() {
        assert!(Verbosity::Default.default_filter().contains("warn"));
    }

    #[test]
    fn trace_all_opts_into_global_trace() {
        assert_eq!(Verbosity::TraceAll.default_filter(), "trace");
    }

    #[test]
    fn detailed_only_for_debug_and_above() {
        assert!(!Verbosity::Default.detailed());
        assert!(!Verbosity::Info.detailed());
        assert!(Verbosity::Debug.detailed());
        assert!(Verbosity::Trace.detailed());
        assert!(Verbosity::TraceAll.detailed());
    }
}
