//! Logging initialization.

use std::io::IsTerminal as _;

use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::time::Uptime;

/// Verbosity resolved from `-v` count and `-q` flag.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Verbosity {
    Quiet,
    #[default]
    Default,
    Info,
    Debug,
    Trace,
}

impl Verbosity {
    pub(crate) fn from_flags(verbose: u8, quiet: bool) -> Self {
        if quiet {
            return Self::Quiet;
        }
        match verbose {
            0 => Self::Default,
            1 => Self::Info,
            2 => Self::Debug,
            _ => Self::Trace,
        }
    }

    fn default_directive(self) -> &'static str {
        match self {
            Self::Quiet => "servo_fetch=error",
            Self::Default => "servo_fetch=warn",
            Self::Info => "servo_fetch=info",
            Self::Debug => "servo_fetch=debug",
            Self::Trace => "servo_fetch=trace",
        }
    }

    fn detailed(self) -> bool {
        matches!(self, Self::Debug | Self::Trace)
    }
}

/// Install the global `tracing` subscriber.
pub(crate) fn init(verbosity: Verbosity) {
    let filter = EnvFilter::builder()
        .with_default_directive(
            verbosity
                .default_directive()
                .parse()
                .expect("hardcoded directive is valid"),
        )
        .from_env_lossy();

    let ansi = std::io::stderr().is_terminal() && std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty());
    let builder = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_ansi(ansi)
        .with_target(verbosity.detailed());

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
        assert_eq!(Verbosity::from_flags(u8::MAX, false), Verbosity::Trace);
    }

    #[test]
    fn default_directive_is_warn() {
        assert_eq!(Verbosity::Default.default_directive(), "servo_fetch=warn");
    }

    #[test]
    fn trace_stays_scoped_to_own_crate() {
        assert_eq!(Verbosity::Trace.default_directive(), "servo_fetch=trace");
    }

    #[test]
    fn detailed_only_for_debug_and_above() {
        assert!(!Verbosity::Quiet.detailed());
        assert!(!Verbosity::Default.detailed());
        assert!(!Verbosity::Info.detailed());
        assert!(Verbosity::Debug.detailed());
        assert!(Verbosity::Trace.detailed());
    }

    #[test]
    fn all_directives_parse() {
        for v in [
            Verbosity::Quiet,
            Verbosity::Default,
            Verbosity::Info,
            Verbosity::Debug,
            Verbosity::Trace,
        ] {
            let _: tracing_subscriber::filter::Directive = v.default_directive().parse().unwrap();
        }
    }
}
