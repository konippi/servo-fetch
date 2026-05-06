//! Platform-specific implementations.

#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "macos")]
pub(crate) use macos::StderrFilter;

#[cfg(not(target_os = "macos"))]
pub(crate) struct StderrFilter;

#[cfg(not(target_os = "macos"))]
impl StderrFilter {
    #[expect(clippy::unnecessary_wraps, reason = "signature must match macOS impl")]
    pub(crate) fn install<F>(_: F) -> std::io::Result<Self>
    where
        F: Fn(&str) -> bool + Send + 'static,
    {
        Ok(Self)
    }
}
