//! Windows-specific: Job Object process-tree containment for isolated workers.

use std::os::windows::io::{
    AsRawHandle as _, FromRawHandle as _, HandleOrInvalid, HandleOrNull, OwnedHandle, RawHandle,
};
use std::os::windows::process::CommandExt as _;
use std::process::{Child, Command};
use std::time::Duration;

use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectBasicAccountingInformation,
    JobObjectExtendedLimitInformation, QueryInformationJobObject, SetInformationJobObject, TerminateJobObject,
};
use windows_sys::Win32::System::Threading::{CREATE_SUSPENDED, OpenThread, ResumeThread, THREAD_SUSPEND_RESUME};

/// Start the worker with every thread suspended so it cannot run before Job assignment.
pub(crate) fn suspend_new_process(command: &mut Command) {
    command.creation_flags(CREATE_SUSPENDED);
}

/// Own a raw handle from an API whose failure sentinel is null.
#[allow(unsafe_code)]
fn owned_from_nullable(raw: RawHandle) -> std::io::Result<OwnedHandle> {
    // SAFETY: the caller transfers sole ownership of the just-created handle.
    OwnedHandle::try_from(unsafe { HandleOrNull::from_raw_handle(raw) }).map_err(|_| std::io::Error::last_os_error())
}

/// Own a raw handle from an API whose failure sentinel is INVALID_HANDLE_VALUE.
#[allow(unsafe_code)]
fn owned_from_invalidable(raw: RawHandle) -> std::io::Result<OwnedHandle> {
    // SAFETY: the caller transfers sole ownership of the just-created handle.
    OwnedHandle::try_from(unsafe { HandleOrInvalid::from_raw_handle(raw) }).map_err(|_| std::io::Error::last_os_error())
}

/// A `KILL_ON_JOB_CLOSE` Job Object owning one worker process tree.
#[derive(Debug)]
pub(crate) struct WindowsJob(OwnedHandle);

impl WindowsJob {
    /// Create a Job and assign the still-suspended child to it.
    pub(crate) fn attach(child: &Child) -> std::io::Result<Self> {
        // SAFETY: null pointers request an unnamed Job with default security.
        #[allow(unsafe_code)]
        let job = owned_from_nullable(unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) })?;
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // SAFETY: limits is initialized storage of the exact information-class size.
        #[allow(unsafe_code)]
        let configured = unsafe {
            SetInformationJobObject(
                job.as_raw_handle().cast(),
                JobObjectExtendedLimitInformation,
                std::ptr::addr_of!(limits).cast(),
                u32::try_from(std::mem::size_of_val(&limits)).expect("job limits size fits u32"),
            )
        };
        if configured == 0 {
            return Err(std::io::Error::last_os_error());
        }
        // SAFETY: both handles are valid; the child is still suspended.
        #[allow(unsafe_code)]
        if unsafe { AssignProcessToJobObject(job.as_raw_handle().cast(), child.as_raw_handle().cast()) } == 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(Self(job))
    }

    /// Terminate every process currently in the Job.
    pub(crate) fn terminate(&self) -> std::io::Result<()> {
        // SAFETY: the owned Job handle is valid for the duration of the call.
        #[allow(unsafe_code)]
        if unsafe { TerminateJobObject(self.0.as_raw_handle().cast(), 1) } == 0 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    /// Block until the Job reports zero active processes.
    pub(crate) fn wait_empty(&self) -> std::io::Result<()> {
        loop {
            let mut accounting = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
            // SAFETY: accounting matches the requested information class.
            #[allow(unsafe_code)]
            let queried = unsafe {
                QueryInformationJobObject(
                    self.0.as_raw_handle().cast(),
                    JobObjectBasicAccountingInformation,
                    std::ptr::addr_of_mut!(accounting).cast(),
                    u32::try_from(std::mem::size_of_val(&accounting)).expect("job accounting size fits u32"),
                    std::ptr::null_mut(),
                )
            };
            if queried == 0 {
                return Err(std::io::Error::last_os_error());
            }
            if accounting.ActiveProcesses == 0 {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

/// Resume a `CREATE_SUSPENDED` child after Job assignment.
pub(crate) fn resume_suspended_process(child: &Child) -> std::io::Result<()> {
    // SAFETY: the snapshot handle is immediately transferred to RAII.
    #[allow(unsafe_code)]
    let snapshot = owned_from_invalidable(unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) })?;
    let mut entry = THREADENTRY32 {
        dwSize: u32::try_from(std::mem::size_of::<THREADENTRY32>()).expect("thread entry size fits u32"),
        ..THREADENTRY32::default()
    };
    let mut resumed = false;
    // SAFETY: entry is sized writable storage and snapshot is valid.
    #[allow(unsafe_code)]
    let mut present = unsafe { Thread32First(snapshot.as_raw_handle().cast(), &raw mut entry) } != 0;
    while present {
        if entry.th32OwnerProcessID == child.id() {
            // SAFETY: the thread belongs to the suspended child; handle owned by RAII.
            #[allow(unsafe_code)]
            let thread = owned_from_nullable(unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID) })?;
            // SAFETY: thread is a valid handle opened with suspend/resume access.
            #[allow(unsafe_code)]
            if unsafe { ResumeThread(thread.as_raw_handle().cast()) } == u32::MAX {
                return Err(std::io::Error::last_os_error());
            }
            resumed = true;
        }
        // SAFETY: entry and snapshot remain valid for the enumeration.
        #[allow(unsafe_code)]
        {
            present = unsafe { Thread32Next(snapshot.as_raw_handle().cast(), &raw mut entry) } != 0;
        }
    }
    if resumed {
        Ok(())
    } else {
        Err(std::io::Error::other("suspended worker primary thread was not found"))
    }
}
