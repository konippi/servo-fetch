//! Apple GL stderr filtering owned by the CLI process.

#[cfg(target_os = "macos")]
mod imp {
    use std::fs::File;
    use std::io::{self, BufRead as _, BufReader, Read, Write};
    use std::os::fd::{AsFd as _, AsRawFd as _, OwnedFd};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
    use std::thread::{self, JoinHandle};
    use std::time::Duration;

    use os_pipe::{PipeWriter, pipe};

    const MAX_FILTER_LINE_BYTES: usize = 8 * 1024;
    const DISABLE_ENV: &str = "SERVO_FETCH_NO_STDERR_FILTER";
    const NOISE_PREFIX: &[u8] = b"UNSUPPORTED (log once): POSSIBLE ISSUE: unit ";
    const NOISE_SUFFIX: &[u8] = b" GLD_TEXTURE_INDEX_2D is unloadable and bound to sampler type (Float) - using zero texture because texture unloadable";
    static INSTALLED: AtomicBool = AtomicBool::new(false);

    /// Suppresses Apple GL driver noise for the guard's lifetime.
    #[must_use]
    pub(crate) struct StderrFilter(Option<Active>);

    struct Active {
        saved: OwnedFd,
        writer: PipeWriter,
        done: Receiver<()>,
        thread: Option<JoinHandle<()>>,
    }

    impl StderrFilter {
        /// Installs the process-wide stderr filter.
        pub(crate) fn install() -> io::Result<Self> {
            if std::env::var_os(DISABLE_ENV).is_some_and(|value| !value.is_empty()) {
                return Ok(Self(None));
            }

            let saved = io::stderr().as_fd().try_clone_to_owned()?;
            let (reader, writer) = pipe()?;
            let thread_out = saved.try_clone()?;
            let (done_tx, done) = mpsc::channel();

            if INSTALLED.swap(true, Ordering::AcqRel) {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "stderr filter already installed",
                ));
            }

            let thread = match thread::Builder::new().name("stderr-filter".into()).spawn(move || {
                run_filter(reader, File::from(thread_out));
                let _ = done_tx.send(());
            }) {
                Ok(thread) => thread,
                Err(error) => {
                    INSTALLED.store(false, Ordering::Release);
                    return Err(error);
                }
            };

            if dup2_retry(writer.as_raw_fd(), libc::STDERR_FILENO) < 0 {
                let error = io::Error::last_os_error();
                INSTALLED.store(false, Ordering::Release);
                drop(writer);
                let _ = thread.join();
                return Err(error);
            }

            Ok(Self(Some(Active {
                saved,
                writer,
                done,
                thread: Some(thread),
            })))
        }
    }

    impl Drop for StderrFilter {
        fn drop(&mut self) {
            if let Some(mut active) = self.0.take() {
                dup2_retry(active.saved.as_raw_fd(), libc::STDERR_FILENO);
                drop(active.writer);
                match active.done.recv_timeout(Duration::from_secs(1)) {
                    Ok(()) | Err(RecvTimeoutError::Disconnected) => {
                        if let Some(thread) = active.thread.take() {
                            let _ = thread.join();
                        }
                        INSTALLED.store(false, Ordering::Release);
                    }
                    Err(RecvTimeoutError::Timeout) => drop(active.thread.take()),
                }
            }
        }
    }

    fn is_apple_gl_driver_noise(line: &[u8]) -> bool {
        let (line, terminated) = line.strip_suffix(b"\n").map_or((line, false), |line| (line, true));
        let line = if terminated {
            line.strip_suffix(b"\r").unwrap_or(line)
        } else {
            line
        };
        let Some(unit_and_suffix) = line.strip_prefix(NOISE_PREFIX) else {
            return false;
        };
        let Some(unit) = unit_and_suffix.strip_suffix(NOISE_SUFFIX) else {
            return false;
        };
        !unit.is_empty() && unit.iter().all(u8::is_ascii_digit)
    }

    fn run_filter<R: Read, W: Write>(reader: R, mut out: W) {
        let mut reader = BufReader::new(reader);
        let mut pending = Vec::with_capacity(256);
        let mut pass_through = false;

        loop {
            let available = match reader.fill_buf() {
                Ok([]) => {
                    if !pass_through {
                        let _ = emit_line(&pending, &mut out);
                    }
                    break;
                }
                Ok(available) => available,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(_) => {
                    if !pass_through {
                        let _ = out.write_all(&pending);
                    }
                    break;
                }
            };
            let newline = available.iter().position(|byte| *byte == b'\n');
            let consumed = newline.map_or(available.len(), |index| index + 1);
            let segment = &available[..consumed];

            if pass_through {
                if out.write_all(segment).is_err() {
                    return;
                }
                if newline.is_some() {
                    pass_through = false;
                }
            } else {
                let bytes_before_newline = newline.unwrap_or(consumed);
                if pending.len() + bytes_before_newline > MAX_FILTER_LINE_BYTES {
                    if out.write_all(&pending).and_then(|()| out.write_all(segment)).is_err() {
                        return;
                    }
                    pending.clear();
                    pass_through = newline.is_none();
                } else {
                    pending.extend_from_slice(segment);
                    if newline.is_some() {
                        if emit_line(&pending, &mut out).is_err() {
                            return;
                        }
                        pending.clear();
                    }
                }
            }
            reader.consume(consumed);
        }
        let _ = out.flush();
    }

    fn emit_line<W: Write>(line: &[u8], out: &mut W) -> io::Result<()> {
        if !is_apple_gl_driver_noise(line) {
            out.write_all(line)?;
        }
        Ok(())
    }

    #[expect(unsafe_code, reason = "libc::dup2 redirects fd 2")]
    fn dup2_retry(src: libc::c_int, dst: libc::c_int) -> libc::c_int {
        loop {
            // SAFETY: dup2 accepts integer descriptors.
            let result = unsafe { libc::dup2(src, dst) };
            if result < 0 && io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return result;
        }
    }

    #[cfg(test)]
    mod tests {
        use std::io::{self, Read, Write as _};
        use std::mem::replace;
        use std::sync::mpsc;
        use std::thread;
        use std::time::Duration;

        use os_pipe::pipe;

        use super::*;

        const DRIVER_LINE: &[u8] = b"UNSUPPORTED (log once): POSSIBLE ISSUE: unit 1 GLD_TEXTURE_INDEX_2D is unloadable and bound to sampler type (Float) - using zero texture because texture unloadable";

        fn run(input: &[u8]) -> Vec<u8> {
            let mut out = Vec::new();
            run_filter(io::Cursor::new(input), &mut out);
            out
        }

        #[test]
        fn filters_only_exact_driver_line() {
            let mut exact_lf = DRIVER_LINE.to_vec();
            exact_lf.push(b'\n');
            assert_eq!(run(&exact_lf), b"");

            let mut exact_unit_42_crlf = NOISE_PREFIX.to_vec();
            exact_unit_42_crlf.extend_from_slice(b"42");
            exact_unit_42_crlf.extend_from_slice(NOISE_SUFFIX);
            exact_unit_42_crlf.extend_from_slice(b"\r\n");
            assert_eq!(run(&exact_unit_42_crlf), b"");
            assert_eq!(run(DRIVER_LINE), b"");

            let mut contextual = b"context: ".to_vec();
            contextual.extend_from_slice(&exact_lf);
            let mut trailing = DRIVER_LINE.to_vec();
            trailing.extend_from_slice(b" trailing\n");
            let mut missing_unit = NOISE_PREFIX.to_vec();
            missing_unit.extend_from_slice(NOISE_SUFFIX);
            missing_unit.push(b'\n');
            let mut wrong_grammar = DRIVER_LINE.to_vec();
            let float = wrong_grammar.windows(7).position(|part| part == b"(Float)").unwrap();
            wrong_grammar.splice(float..float + 7, b"(Int)".iter().copied());
            wrong_grammar.push(b'\n');
            let mut final_cr = DRIVER_LINE.to_vec();
            final_cr.push(b'\r');

            for preserved in [contextual, trailing, missing_unit, wrong_grammar, final_cr] {
                assert_eq!(run(&preserved), preserved);
            }
        }

        #[test]
        fn preserves_long_invalid_and_partial_bytes() {
            let mut input = DRIVER_LINE.to_vec();
            input.extend(std::iter::repeat_n(b'x', MAX_FILTER_LINE_BYTES));
            input.extend_from_slice(b"\ninvalid: \xff\xfe\nfinal partial: \x80");
            assert_eq!(run(&input), input);
        }

        #[test]
        fn terminal_read_error_preserves_buffered_bytes() {
            struct TerminalError {
                emitted: bool,
                failed: bool,
            }

            impl Read for TerminalError {
                fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
                    if !replace(&mut self.emitted, true) {
                        buffer[..7].copy_from_slice(b"pending");
                        return Ok(7);
                    }
                    assert!(!replace(&mut self.failed, true), "reader polled after terminal error");
                    Err(io::Error::other("read failed"))
                }
            }

            let mut out = Vec::new();
            run_filter(
                TerminalError {
                    emitted: false,
                    failed: false,
                },
                &mut out,
            );
            assert_eq!(out, b"pending");
        }

        #[test]
        fn forwarder_drains_on_eof() {
            let (reader, mut writer) = pipe().unwrap();
            let (mut out_reader, out_writer) = pipe().unwrap();
            let (done_tx, done) = mpsc::channel();
            let thread = thread::spawn(move || {
                run_filter(reader, out_writer);
                let _ = done_tx.send(());
            });

            writer.write_all(b"first\nsecond\n").unwrap();
            drop(writer);
            assert_eq!(done.recv_timeout(Duration::from_secs(1)), Ok(()));
            thread.join().unwrap();
            let mut out = Vec::new();
            out_reader.read_to_end(&mut out).unwrap();
            assert_eq!(out, b"first\nsecond\n");
        }
    }
}

#[cfg(target_os = "macos")]
pub(crate) use imp::StderrFilter;

#[cfg(not(target_os = "macos"))]
#[must_use]
pub(crate) struct StderrFilter(());

#[cfg(not(target_os = "macos"))]
impl StderrFilter {
    #[expect(clippy::unnecessary_wraps, reason = "signature matches the macOS implementation")]
    pub(crate) fn install() -> std::io::Result<Self> {
        Ok(Self(()))
    }
}
