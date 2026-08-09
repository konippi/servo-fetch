//! Best-effort cleanup of session storage left behind by killed processes.

use std::path::Path;
use std::sync::Once;

const OWNER_MARKER: &str = "owner.pid";

/// Record the owning process so future processes can tell stale storage from a live broker's.
pub(super) fn write_owner_marker(config_dir: &Path) {
    let _ = std::fs::write(config_dir.join(OWNER_MARKER), std::process::id().to_string());
}

/// Delete session directories whose recorded owner is dead; unreadable markers and live owners are always kept.
pub(super) fn scavenge_stale_sessions_once() {
    static SCAVENGE: Once = Once::new();
    SCAVENGE.call_once(|| {
        let _ = std::thread::Builder::new()
            .name("servo-fetch-scavenger".into())
            .spawn(|| scavenge_stale_sessions(&std::env::temp_dir()));
    });
}

fn scavenge_stale_sessions(temp_root: &Path) {
    let Ok(entries) = std::fs::read_dir(temp_root) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if !entry.file_name().to_string_lossy().starts_with("servo-fetch-session-") {
            continue;
        }
        let Ok(owner) = std::fs::read_to_string(path.join(OWNER_MARKER)) else {
            continue;
        };
        let Ok(owner) = owner.trim().parse::<u32>() else {
            continue;
        };
        if process_is_alive(owner) {
            continue;
        }
        match std::fs::remove_dir_all(&path) {
            Ok(()) => tracing::debug!(path = %path.display(), owner, "removed stale session storage"),
            Err(error) => tracing::debug!(path = %path.display(), %error, "failed to remove stale session storage"),
        }
    }
}

#[cfg(unix)]
#[allow(unsafe_code)]
fn process_is_alive(pid: u32) -> bool {
    let Ok(pid) = i32::try_from(pid) else {
        return true;
    };
    // SAFETY: kill with signal 0 performs a liveness/permission probe only.
    let alive = unsafe { libc::kill(pid, 0) } == 0;
    alive || std::io::Error::last_os_error().kind() == std::io::ErrorKind::PermissionDenied
}

#[cfg(windows)]
#[allow(unsafe_code)]
fn process_is_alive(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
    // SAFETY: OpenProcess/CloseHandle probe a foreign process without touching it.
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            // Access denied means the process exists; treat other failures as dead.
            return std::io::Error::last_os_error().raw_os_error() == Some(5);
        }
        CloseHandle(handle);
        true
    }
}

#[cfg(not(any(unix, windows)))]
fn process_is_alive(_pid: u32) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stale_dir(root: &Path, name: &str, marker: Option<&str>) -> std::path::PathBuf {
        let dir = root.join(name);
        std::fs::create_dir(&dir).unwrap();
        if let Some(marker) = marker {
            std::fs::write(dir.join(OWNER_MARKER), marker).unwrap();
        }
        dir
    }

    #[test]
    fn removes_only_dead_owner_directories() {
        let root = tempfile::tempdir().unwrap();
        let live = stale_dir(
            root.path(),
            "servo-fetch-session-live",
            Some(&std::process::id().to_string()),
        );
        let dead = stale_dir(root.path(), "servo-fetch-session-dead", Some("999999999"));
        let unmarked = stale_dir(root.path(), "servo-fetch-session-unmarked", None);
        let malformed = stale_dir(root.path(), "servo-fetch-session-bad", Some("not-a-pid"));
        let unrelated = stale_dir(root.path(), "other-app-data", Some("999999999"));

        scavenge_stale_sessions(root.path());

        assert!(live.exists(), "live owner must be preserved");
        assert!(!dead.exists(), "dead owner must be removed");
        assert!(unmarked.exists(), "unmarked directories must be preserved");
        assert!(malformed.exists(), "malformed markers must be preserved");
        assert!(unrelated.exists(), "non-session directories must be ignored");
    }
}
