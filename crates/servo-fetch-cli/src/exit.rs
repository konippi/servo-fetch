//! Process exit lifecycle.

use std::fmt;
use std::io::{self, Write as _};

use anyhow::{Error, Result};
use servo_fetch::Error as FetchError;

pub(crate) fn exit_code(result: Result<()>) -> i32 {
    match result {
        Ok(()) => 0,
        Err(err) if is_closed_output_pipe(&err) => 0,
        Err(err) => {
            eprintln!("error: {err:#}");
            error_exit_code(&err)
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
struct OutputError(#[from] io::Error);

pub(crate) fn output_error(error: io::Error) -> Error {
    Error::new(OutputError(error))
}

/// Combines command output and per-item failures.
pub(crate) fn finalize_multi_item_result(output_error: Option<Error>, failure_error: Option<Error>) -> Result<()> {
    match output_error {
        Some(error) if is_closed_output_pipe(&error) => failure_error.map_or(Ok(()), Err),
        Some(error) => Err(error),
        None => failure_error.map_or(Ok(()), Err),
    }
}

/// Internal failure categories are ordered by product precedence,
/// independently of their public process exit-code numbers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FailureCategory {
    Usage,
    Data,
    NoInput,
    Unavailable,
    Software,
    Io,
    TempFail,
    Unknown,
}

impl FailureCategory {
    const COUNT: usize = 8;
    const PRECEDENCE: [Self; Self::COUNT] = [
        Self::Usage,
        Self::Data,
        Self::NoInput,
        Self::Unavailable,
        Self::Software,
        Self::Io,
        Self::TempFail,
        Self::Unknown,
    ];

    const fn index(self) -> usize {
        match self {
            Self::Usage => 0,
            Self::Data => 1,
            Self::NoInput => 2,
            Self::Unavailable => 3,
            Self::Software => 4,
            Self::Io => 5,
            Self::TempFail => 6,
            Self::Unknown => 7,
        }
    }

    const fn exit_code(self) -> i32 {
        match self {
            Self::Usage => sysexits::USAGE,
            Self::Data => sysexits::DATAERR,
            Self::NoInput => sysexits::NOINPUT,
            Self::Unavailable => sysexits::UNAVAILABLE,
            Self::Software => sysexits::SOFTWARE,
            Self::Io => sysexits::IOERR,
            Self::TempFail => sysexits::TEMPFAIL,
            Self::Unknown => 1,
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Usage => "invalid input",
            Self::Data => "invalid data",
            Self::NoInput => "missing input",
            Self::Unavailable => "unavailable",
            Self::Software => "software",
            Self::Io => "I/O",
            Self::TempFail => "temporary failure",
            Self::Unknown => "other",
        }
    }
}

/// Classify every currently reachable Servo failure.
fn classify_fetch_error(err: &FetchError) -> FailureCategory {
    match err {
        FetchError::InvalidUrl { .. }
        | FetchError::InvalidGlob(_)
        | FetchError::InvalidHeader(_)
        | FetchError::InvalidSessionConfig { .. }
        | FetchError::UnsupportedSessionOperation { .. } => FailureCategory::Usage,
        FetchError::Schema(servo_fetch::schema::SchemaError::Io(_)) | FetchError::Io(_) => FailureCategory::Io,
        FetchError::Schema(_) => FailureCategory::Data,
        FetchError::Cookies { .. } => FailureCategory::NoInput,
        FetchError::AddressNotAllowed { .. } => FailureCategory::Unavailable,
        FetchError::Engine { .. }
        | FetchError::JavaScript { .. }
        | FetchError::Screenshot { .. }
        | FetchError::Extract(_)
        | FetchError::OutputTooLarge { .. }
        | FetchError::WorkerUnavailable { .. } => FailureCategory::Software,
        FetchError::Timeout { .. }
        | FetchError::SessionCancelled
        | FetchError::WorkerProtocolTimeout { .. }
        | FetchError::SessionAcquireTimeout { .. }
        | FetchError::SessionBrokerFull => FailureCategory::TempFail,
        _ => FailureCategory::Unknown,
    }
}

/// Failure counts for a multi-item run.
#[derive(Debug, Default)]
pub(crate) struct Failures {
    counts: [usize; FailureCategory::COUNT],
}

impl Failures {
    pub(crate) fn record(&mut self, err: &FetchError) {
        self.record_category(classify_fetch_error(err));
    }

    fn record_category(&mut self, category: FailureCategory) {
        self.counts[category.index()] += 1;
    }

    pub(crate) fn failed(&self) -> usize {
        self.counts.iter().sum()
    }

    pub(crate) fn into_error(self, unit: &'static str, total: usize) -> Option<Error> {
        let category = FailureCategory::PRECEDENCE
            .into_iter()
            .find(|category| self.counts[category.index()] > 0)?;
        debug_assert!(self.failed() <= total, "failed count must not exceed total");
        Some(Error::new(PartialFailure {
            counts: self.counts,
            total,
            unit,
            category,
        }))
    }
}

/// The deterministic summary that ends a multi-item run.
#[derive(Debug)]
struct PartialFailure {
    counts: [usize; FailureCategory::COUNT],
    total: usize,
    unit: &'static str,
    category: FailureCategory,
}

impl fmt::Display for PartialFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let failed: usize = self.counts.iter().sum();
        write!(f, "{failed} of {} {} failed (", self.total, self.unit)?;
        let mut separator = "";
        for category in FailureCategory::PRECEDENCE {
            let count = self.counts[category.index()];
            if count > 0 {
                write!(f, "{separator}{}: {count}", category.label())?;
                separator = ", ";
            }
        }
        f.write_str(")")
    }
}

impl std::error::Error for PartialFailure {}

fn error_exit_code(err: &Error) -> i32 {
    if let Some(partial) = err.chain().find_map(|cause| cause.downcast_ref::<PartialFailure>()) {
        return partial.category.exit_code();
    }

    let mut present = [false; FailureCategory::COUNT];
    for cause in err.chain() {
        if let Some(fetch) = cause.downcast_ref::<FetchError>() {
            present[classify_fetch_error(fetch).index()] = true;
        }
        if cause.downcast_ref::<io::Error>().is_some() || cause.downcast_ref::<OutputError>().is_some() {
            present[FailureCategory::Io.index()] = true;
        }
    }
    FailureCategory::PRECEDENCE
        .into_iter()
        .find(|category| present[category.index()])
        .unwrap_or(FailureCategory::Unknown)
        .exit_code()
}

/// Exit codes from [`sysexits.h`](https://man.freebsd.org/cgi/man.cgi?sysexits).
mod sysexits {
    pub(super) const USAGE: i32 = 64;
    pub(super) const DATAERR: i32 = 65;
    pub(super) const NOINPUT: i32 = 66;
    pub(super) const UNAVAILABLE: i32 = 69;
    pub(super) const SOFTWARE: i32 = 70;
    pub(super) const IOERR: i32 = 74;
    pub(super) const TEMPFAIL: i32 = 75;
}

fn is_closed_output_pipe(err: &Error) -> bool {
    err.chain().any(|cause| {
        cause
            .downcast_ref::<OutputError>()
            .is_some_and(|error| error.0.kind() == io::ErrorKind::BrokenPipe)
    })
}

/// Flush stdio and terminate via `libc::_exit`, skipping SpiderMonkey's
/// static destructors that race on `pthread_mutex_destroy`.
#[expect(
    unsafe_code,
    reason = "libc::_exit bypasses the static destructors that crash on exit"
)]
pub(crate) fn flush_and_exit(code: i32, stderr_filter: Option<crate::stderr_filter::StderrFilter>) -> ! {
    let _ = io::stdout().flush();
    let _ = io::stderr().flush();
    drop(stderr_filter);
    // SAFETY: `_exit` only terminates the process; no Rust state is touched afterwards.
    unsafe { libc::_exit(code) }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn ok_is_zero() {
        assert_eq!(exit_code(Ok(())), 0);
    }

    #[test]
    fn only_marked_output_broken_pipe_is_success() {
        let cases = [
            (output_error(io::Error::new(io::ErrorKind::BrokenPipe, "stdout")), 0),
            (
                Error::new(FetchError::Io(io::Error::new(io::ErrorKind::BrokenPipe, "worker pipe"))),
                sysexits::IOERR,
            ),
            (
                Error::new(io::Error::new(io::ErrorKind::BrokenPipe, "unmarked pipe")),
                sysexits::IOERR,
            ),
        ];

        for (error, expected) in cases {
            assert_eq!(exit_code(Err(error)), expected);
        }
    }

    #[test]
    fn representative_failures_map_to_public_exit_categories() {
        let cases = [
            (
                Error::new(servo_fetch::validate_url("ftp://x").unwrap_err()),
                sysexits::USAGE,
            ),
            (
                Error::new(FetchError::from(
                    servo_fetch::schema::ExtractSchema::from_json("{").unwrap_err(),
                )),
                sysexits::DATAERR,
            ),
            (
                Error::new(servo_fetch::load_cookies("/no/such/cookies.txt").unwrap_err()),
                sysexits::NOINPUT,
            ),
            (
                Error::new(FetchError::AddressNotAllowed {
                    host: "127.0.0.1".into(),
                }),
                sysexits::UNAVAILABLE,
            ),
            (
                Error::new(FetchError::Extract(servo_fetch::extract::ExtractError::InvalidSelector)),
                sysexits::SOFTWARE,
            ),
            (
                Error::new(FetchError::Io(io::Error::other("disk failed"))),
                sysexits::IOERR,
            ),
            (
                Error::new(FetchError::Timeout {
                    url: "https://example.com".into(),
                    timeout: Duration::from_secs(1),
                }),
                sysexits::TEMPFAIL,
            ),
            (anyhow::anyhow!("unknown"), 1),
        ];

        for (error, expected) in cases {
            assert_eq!(error_exit_code(&error), expected, "{error:?}");
        }
    }

    #[test]
    fn aggregate_failure_policy_is_order_independent_and_prefers_known() {
        let orders = [
            [
                FailureCategory::TempFail,
                FailureCategory::Unknown,
                FailureCategory::Usage,
                FailureCategory::TempFail,
            ],
            [
                FailureCategory::Usage,
                FailureCategory::TempFail,
                FailureCategory::TempFail,
                FailureCategory::Unknown,
            ],
        ];
        let errors = orders.map(|order| {
            let mut failures = Failures::default();
            for category in order {
                failures.record_category(category);
            }
            failures.into_error("URLs", 5).unwrap()
        });

        assert_eq!(errors[0].to_string(), errors[1].to_string());
        assert_eq!(
            errors[0].to_string(),
            "4 of 5 URLs failed (invalid input: 1, temporary failure: 2, other: 1)"
        );
        assert_eq!(error_exit_code(&errors[0]), sysexits::USAGE);
    }

    #[test]
    fn no_failures_produce_no_error() {
        assert!(Failures::default().into_error("URLs", 4).is_none());
    }

    #[test]
    fn multi_item_error_selection_policy() {
        let cases = [
            (Some(io::ErrorKind::BrokenPipe), true, Some(sysexits::USAGE)),
            (Some(io::ErrorKind::BrokenPipe), false, None),
            (Some(io::ErrorKind::Other), true, Some(sysexits::IOERR)),
            (None, true, Some(sysexits::USAGE)),
            (None, false, None),
        ];

        for (output_kind, has_failure, expected) in cases {
            let output = output_kind.map(|kind| output_error(io::Error::new(kind, "output failed")));
            let failure = has_failure.then(|| {
                let mut failures = Failures::default();
                failures.record(&servo_fetch::validate_url("ftp://x").unwrap_err());
                failures.into_error("URLs", 1).unwrap()
            });
            let result = finalize_multi_item_result(output, failure);
            let actual = result.err().map(|error| error_exit_code(&error));

            assert_eq!(actual, expected, "output={output_kind:?}, failure={has_failure}");
        }
    }
}
