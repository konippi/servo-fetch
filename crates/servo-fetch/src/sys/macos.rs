//! macOS-specific: pipe-based stderr filter for Apple OpenGL driver noise.

use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};

use os_pipe::{PipeWriter, pipe};

const MAX_LINE_BYTES: usize = 8 * 1024;
const DISABLE_ENV: &str = "SERVO_FETCH_NO_STDERR_FILTER";
static INSTALLED: AtomicBool = AtomicBool::new(false);

pub(crate) struct StderrFilter {
    saved: Option<OwnedFd>,
    writer: Option<PipeWriter>,
    thread: Option<JoinHandle<()>>,
}

impl StderrFilter {
    #[expect(unsafe_code, reason = "fd redirection via libc::dup2")]
    pub(crate) fn install<F>(predicate: F) -> io::Result<Self>
    where
        F: Fn(&str) -> bool + Send + 'static,
    {
        if std::env::var_os(DISABLE_ENV).is_some() {
            return Ok(Self::disabled());
        }
        if INSTALLED.swap(true, Ordering::AcqRel) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "stderr filter already installed",
            ));
        }
        let undo_installed = Undo::new(|| INSTALLED.store(false, Ordering::Release));

        let (reader, writer) = pipe()?;
        let saved = dup_stderr()?;
        let thread_out = saved.try_clone()?;

        if unsafe { dup2_retry(writer.as_raw_fd(), libc::STDERR_FILENO) } < 0 {
            return Err(io::Error::last_os_error());
        }
        let undo_redirect = Undo::new({
            let saved_fd = saved.as_raw_fd();
            move || unsafe {
                dup2_retry(saved_fd, libc::STDERR_FILENO);
            }
        });

        let thread = thread::Builder::new()
            .name("stderr-filter".into())
            .spawn(move || run_filter(reader, File::from(thread_out), predicate))?;

        undo_redirect.defuse();
        undo_installed.defuse();
        Ok(Self {
            saved: Some(saved),
            writer: Some(writer),
            thread: Some(thread),
        })
    }

    fn disabled() -> Self {
        Self {
            saved: None,
            writer: None,
            thread: None,
        }
    }
}

impl Drop for StderrFilter {
    #[expect(unsafe_code, reason = "fd restoration via libc::dup2")]
    fn drop(&mut self) {
        if let Some(saved) = self.saved.as_ref() {
            unsafe { dup2_retry(saved.as_raw_fd(), libc::STDERR_FILENO) };
        }
        self.writer.take();
        if let Some(h) = self.thread.take() {
            let _ = h.join();
        }
        if self.saved.is_some() {
            INSTALLED.store(false, Ordering::Release);
        }
    }
}

fn run_filter<R, W, F>(reader: R, mut out: W, predicate: F)
where
    R: Read,
    W: Write,
    F: Fn(&str) -> bool,
{
    let mut reader = BufReader::new(reader);
    let mut buf = Vec::with_capacity(256);
    loop {
        buf.clear();
        match reader.by_ref().take(MAX_LINE_BYTES as u64).read_until(b'\n', &mut buf) {
            Ok(0) => break,
            Ok(_) => {}
            Err(_) => continue,
        }
        let Ok(line) = std::str::from_utf8(&buf) else {
            let _ = out.write_all(&buf);
            continue;
        };
        let trimmed = line.strip_suffix('\n').unwrap_or(line);
        let suppress = std::panic::catch_unwind(AssertUnwindSafe(|| predicate(trimmed))).unwrap_or(false);
        if !suppress {
            let _ = out.write_all(line.as_bytes());
        }
    }
}

#[expect(
    unsafe_code,
    reason = "libc::fcntl for fd cloning, OwnedFd::from_raw_fd for exclusive ownership"
)]
fn dup_stderr() -> io::Result<OwnedFd> {
    let fd = unsafe { libc::fcntl(libc::STDERR_FILENO, libc::F_DUPFD_CLOEXEC, 0) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

#[expect(unsafe_code, reason = "libc::dup2 with EINTR retry")]
unsafe fn dup2_retry(src: libc::c_int, dst: libc::c_int) -> libc::c_int {
    loop {
        let r = unsafe { libc::dup2(src, dst) };
        if r < 0 && io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
            continue;
        }
        return r;
    }
}

struct Undo<F: FnOnce()> {
    action: Option<F>,
}
impl<F: FnOnce()> Undo<F> {
    fn new(action: F) -> Self {
        Self { action: Some(action) }
    }
    fn defuse(mut self) {
        self.action = None;
    }
}
impl<F: FnOnce()> Drop for Undo<F> {
    fn drop(&mut self) {
        if let Some(action) = self.action.take() {
            action();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(input: &[u8], predicate: impl Fn(&str) -> bool) -> Vec<u8> {
        let (in_r, mut in_w) = pipe().unwrap();
        in_w.write_all(input).unwrap();
        drop(in_w);
        let (mut out_r, out_w) = pipe().unwrap();
        run_filter(in_r, out_w, predicate);
        let mut captured = Vec::new();
        out_r.read_to_end(&mut captured).unwrap();
        captured
    }

    #[test]
    fn drops_matching_lines() {
        let out = run(b"keep one\nDROP me\nkeep two\n", |line| line.contains("DROP"));
        assert_eq!(std::str::from_utf8(&out).unwrap(), "keep one\nkeep two\n");
    }

    #[test]
    fn tolerates_predicate_panic() {
        let out = run(b"a\nb\nc\n", |_| panic!("boom"));
        assert_eq!(std::str::from_utf8(&out).unwrap(), "a\nb\nc\n");
    }

    #[test]
    fn caps_line_length() {
        let mut input = vec![b'x'; MAX_LINE_BYTES * 3];
        input.extend_from_slice(b"\nshort\n");
        let out = run(&input, |_| false);
        assert!(out.ends_with(b"short\n"));
        assert!(out.len() >= MAX_LINE_BYTES);
    }
}
