//! Platform façade for containing, terminating, and observing the worker process tree.

#[cfg(unix)]
use std::os::fd::{AsRawFd as _, FromRawFd as _, OwnedFd};
#[cfg(unix)]
use std::os::unix::process::CommandExt as _;
use std::process::Child;
#[cfg(unix)]
use std::process::Command;

use super::kill::TerminationTarget;
use crate::error::Result;
#[cfg(windows)]
use crate::sys::windows::WindowsJob;
#[cfg(unix)]
use crate::worker::PARENT_LIFELINE_FD_ENV;
use crate::worker::worker_error;

#[derive(Debug)]
pub(super) struct ProcessTree {
    #[cfg(windows)]
    job: WindowsJob,
    #[cfg(unix)]
    process_group: i32,
    #[cfg(not(any(unix, windows)))]
    child_id: u32,
}

impl ProcessTree {
    #[cfg(windows)]
    pub(super) fn attach(child: &Child) -> Result<Self> {
        Ok(Self {
            job: WindowsJob::attach(child).map_err(worker_error)?,
        })
    }

    #[cfg(not(windows))]
    pub(super) fn attach(child: &Child) -> Result<Self> {
        #[cfg(unix)]
        {
            Ok(Self {
                process_group: i32::try_from(child.id())
                    .map_err(|_| worker_error("worker PID cannot be represented as a Unix process group"))?,
            })
        }
        #[cfg(not(any(unix, windows)))]
        {
            Ok(Self { child_id: child.id() })
        }
    }

    pub(super) fn terminate(&self, child: &Child) -> std::io::Result<()> {
        let child_id = child.id();
        #[cfg(unix)]
        debug_assert_eq!(
            i32::try_from(child_id).ok(),
            Some(self.process_group),
            "worker process group must match the direct child"
        );
        #[cfg(windows)]
        debug_assert_ne!(child_id, 0, "worker child must have a process ID");
        #[cfg(not(any(unix, windows)))]
        debug_assert_eq!(self.child_id, child_id, "worker child identity must remain stable");
        self.terminate_now()
    }

    pub(super) fn terminate_now(&self) -> std::io::Result<()> {
        #[cfg(unix)]
        loop {
            // SAFETY: the unreaped direct child pins this process-group identity.
            #[allow(unsafe_code)]
            let result = unsafe { libc::kill(-self.process_group, libc::SIGKILL) };
            if result == 0 {
                return Ok(());
            }
            match std::io::Error::last_os_error() {
                error if error.raw_os_error() == Some(libc::EINTR) => {}
                error if error.raw_os_error() == Some(libc::ESRCH) => return Ok(()),
                // macOS reports EPERM for an all-zombie group: nothing left to kill.
                #[cfg(target_os = "macos")]
                error if error.raw_os_error() == Some(libc::EPERM) => return Ok(()),
                error => return Err(error),
            }
        }
        #[cfg(windows)]
        {
            self.job.terminate()
        }
        #[cfg(not(any(unix, windows)))]
        Ok(())
    }

    /// Block until every process in the Windows Job has exited.
    #[cfg(windows)]
    pub(super) fn wait_quiescent(&self) -> std::io::Result<()> {
        self.job.wait_empty()
    }
}

impl TerminationTarget for ProcessTree {
    fn terminate_now(&self) -> std::io::Result<()> {
        Self::terminate_now(self)
    }
}

#[cfg(windows)]
pub(super) fn observe_child_exit_without_reaping(child: &mut Child) -> std::io::Result<bool> {
    // Windows try_wait is a nondestructive handle probe; reaping is Unix-only.
    Ok(child.try_wait()?.is_some())
}

#[cfg(unix)]
#[allow(unsafe_code)]
pub(super) fn observe_child_exit_without_reaping(child: &mut Child) -> std::io::Result<bool> {
    let pid = libc::id_t::try_from(child.id())
        .map_err(|_| std::io::Error::other("worker PID cannot be represented for waitid"))?;
    loop {
        let mut info = std::mem::MaybeUninit::<libc::siginfo_t>::zeroed();
        // SAFETY: info is writable siginfo_t storage and this supervisor owns the child PID.
        let result = unsafe {
            libc::waitid(
                libc::P_PID,
                pid,
                info.as_mut_ptr(),
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        };
        if result == 0 {
            // SAFETY: waitid initialized info; a zero PID denotes WNOHANG with no status.
            return Ok(unsafe { info.assume_init().si_pid() } != 0);
        }
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::EINTR) {
            return Err(error);
        }
    }
}

#[cfg(unix)]
#[allow(unsafe_code)]
pub(in crate::session) fn create_parent_lifeline() -> Result<(OwnedFd, OwnedFd)> {
    let (child_end, parent_end) = std::os::unix::net::UnixStream::pair().map_err(worker_error)?;
    let duplicate = |descriptor: OwnedFd| -> Result<OwnedFd> {
        // SAFETY: descriptor is valid and the returned descriptor has independent ownership.
        let duplicated = unsafe { libc::fcntl(descriptor.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 3) };
        if duplicated < 0 {
            return Err(worker_error(std::io::Error::last_os_error()));
        }
        // SAFETY: fcntl returned a new descriptor whose ownership is transferred here.
        Ok(unsafe { OwnedFd::from_raw_fd(duplicated) })
    };
    Ok((duplicate(child_end.into())?, duplicate(parent_end.into())?))
}

#[cfg(unix)]
#[allow(unsafe_code)]
pub(super) fn inherit_parent_lifeline(process: &mut Command, lifeline_fd: i32) {
    process
        .process_group(0)
        .env(PARENT_LIFELINE_FD_ENV, lifeline_fd.to_string());
    // SAFETY: fcntl is async-signal-safe and only clears CLOEXEC on the dedicated
    // descriptor after fork, so concurrent process spawns cannot inherit it.
    unsafe {
        process.pre_exec(move || {
            let flags = libc::fcntl(lifeline_fd, libc::F_GETFD);
            if flags < 0 || libc::fcntl(lifeline_fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
}
