//! Ownership of a spawned process tree. Attach before sending worker initialization.

pub struct ProcessGuard {
    #[cfg(windows)]
    job: windows_sys::Win32::Foundation::HANDLE,
    #[cfg(unix)]
    pgid: i32,
}

#[cfg(windows)]
unsafe impl Send for ProcessGuard {}
#[cfg(windows)]
unsafe impl Sync for ProcessGuard {}

impl ProcessGuard {
    /// The command must use a new process group on Unix. Assignment failure is fatal.
    pub fn attach(child: &tokio::process::Child) -> Result<Self, String> {
        #[cfg(windows)]
        { Self::attach_windows(child.raw_handle().ok_or("worker process handle unavailable")?) }
        #[cfg(unix)]
        { child.id().map(|pid| Self { pgid: pid as i32 }).ok_or_else(|| "worker process id unavailable".into()) }
    }

    #[cfg(windows)]
    pub(crate) fn attach_windows(process: std::os::windows::io::RawHandle) -> Result<Self, String> {
        unsafe {
            use windows_sys::Win32::{Foundation::CloseHandle, System::JobObjects::{
                AssignProcessToJobObject, CreateJobObjectW, SetInformationJobObject,
                JobObjectExtendedLimitInformation, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
                JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            }};
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() { return Err(format!("CreateJobObjectW: {}", std::io::Error::last_os_error())); }
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(job, JobObjectExtendedLimitInformation, &limits as *const _ as _, std::mem::size_of_val(&limits) as u32) == 0 {
                let error = std::io::Error::last_os_error();
                CloseHandle(job);
                return Err(format!("SetInformationJobObject: {error}"));
            }
            if AssignProcessToJobObject(job, process as _) == 0 {
                let error = std::io::Error::last_os_error();
                CloseHandle(job);
                return Err(format!("AssignProcessToJobObject: {error}"));
            }
            Ok(Self { job })
        }
    }
}

impl Drop for ProcessGuard {
    fn drop(&mut self) {
        #[cfg(windows)]
        unsafe { windows_sys::Win32::Foundation::CloseHandle(self.job); }
        #[cfg(unix)]
        unsafe { libc::kill(-self.pgid, libc::SIGKILL); }
    }
}

// This handle intentionally lives for the whole parent process, not a tool
// invocation: background commands retain their existing lifetime semantics.
#[cfg(windows)]
struct ParentToolJob(windows_sys::Win32::Foundation::HANDLE);

#[cfg(windows)]
unsafe impl Send for ParentToolJob {}
#[cfg(windows)]
unsafe impl Sync for ParentToolJob {}

/// Associate a parent-side tool process with the process-lifetime job before
/// attaching its per-invocation tree. Assignment failure must fail closed.
/// Shared MCP servers are intentionally not members of this job.
pub fn attach_parent_tool(child: &tokio::process::Child) -> Result<(), String> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::{Foundation::CloseHandle, System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, SetInformationJobObject,
            JobObjectExtendedLimitInformation, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        }};
        static JOB: std::sync::LazyLock<Result<ParentToolJob, String>> = std::sync::LazyLock::new(|| unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() { return Err(format!("CreateJobObjectW: {}", std::io::Error::last_os_error())); }
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(job, JobObjectExtendedLimitInformation, &limits as *const _ as _, std::mem::size_of_val(&limits) as u32) == 0 {
                let error = std::io::Error::last_os_error();
                CloseHandle(job);
                return Err(format!("SetInformationJobObject: {error}"));
            }
            Ok(ParentToolJob(job))
        });
        let job = JOB.as_ref().map_err(Clone::clone)?;
        let process = child.raw_handle().ok_or("tool process handle unavailable")?;
        if unsafe { AssignProcessToJobObject(job.0, process as _) } == 0 {
            return Err(format!("AssignProcessToJobObject: {}", std::io::Error::last_os_error()));
        }
        Ok(())
    }
    #[cfg(unix)]
    { let _ = child; Ok(()) }
}

/// A minimal tool bootstrap: it cannot execute a command until its parent has
/// assigned the process to its jobs and supplied a complete initialization.
pub fn tool_bootstrap_main() -> Result<i32, String> {
    use std::io::Read;
    use std::process::{Command, Stdio};

    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Init { shell: String, command: String }

    const MAX_INIT: u64 = 1024 * 1024;
    let mut bytes = Vec::new();
    std::io::stdin().lock().take(MAX_INIT + 1).read_to_end(&mut bytes)
        .map_err(|_| "tool bootstrap initialization read failed".to_owned())?;
    if bytes.len() as u64 > MAX_INIT { return Err("tool bootstrap initialization exceeds size limit".into()); }
    let init: Init = serde_json::from_slice(&bytes).map_err(|_| "invalid tool bootstrap initialization".to_owned())?;
    let status = Command::new(init.shell).arg("-c").arg(init.command)
        .stdin(Stdio::null()).stdout(Stdio::inherit()).stderr(Stdio::inherit())
        .status().map_err(|e| format!("tool shell failed to start or wait: {e}"))?;
    if let Some(code) = status.code() { return Ok(code); }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        Ok(128 + status.signal().unwrap_or(1))
    }
    #[cfg(windows)]
    { Ok(1) }
}
