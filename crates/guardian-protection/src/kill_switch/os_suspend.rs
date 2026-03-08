// crates/guardian-protection/src/kill_switch/os_suspend.rs
// OS-native process suspension — NOT kill -9. The process is FROZEN, not terminated.
// It can be resumed. This is the key difference from a hard kill.

/// Suspend a process by PID using OS-native APIs.
/// Returns Ok(true) if suspension succeeded, Ok(false) if process not found.
pub fn suspend_process(pid: u32) -> Result<bool, String> {
    #[cfg(target_os = "windows")]
    return suspend_windows(pid);

    #[cfg(target_os = "linux")]
    return suspend_linux(pid);

    #[cfg(target_os = "macos")]
    return suspend_macos(pid);

    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    Err(format!("Process suspension not implemented for this platform"))
}

/// Resume a previously suspended process.
pub fn resume_process(pid: u32) -> Result<bool, String> {
    #[cfg(target_os = "windows")]
    return resume_windows(pid);

    #[cfg(target_os = "linux")]
    return resume_linux(pid);

    #[cfg(target_os = "macos")]
    return resume_macos(pid);

    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    Err(format!("Process resume not implemented for this platform"))
}

// ── WINDOWS ──────────────────────────────────────────────────────────────────
// Enumerates all threads of the target PID and calls SuspendThread on each.
// Uses toolhelp snapshot (documented Win32 API — no undocumented functions).

#[cfg(target_os = "windows")]
fn suspend_windows(pid: u32) -> Result<bool, String> {
    use std::mem;
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Thread32First, Thread32Next,
        THREADENTRY32, TH32CS_SNAPTHREAD,
    };
    use windows_sys::Win32::System::Threading::{OpenThread, SuspendThread, THREAD_SUSPEND_RESUME};

    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return Err(format!("CreateToolhelp32Snapshot failed for PID {}", pid));
        }

        let mut entry: THREADENTRY32 = mem::zeroed();
        entry.dwSize = mem::size_of::<THREADENTRY32>() as u32;

        let mut suspended = 0u32;
        if Thread32First(snapshot, &mut entry) != 0 {
            loop {
                if entry.th32OwnerProcessID == pid {
                    let thread_handle = OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID);
                    if thread_handle != std::ptr::null_mut() {
                        SuspendThread(thread_handle);
                        CloseHandle(thread_handle);
                        suspended += 1;
                    }
                }
                entry.dwSize = mem::size_of::<THREADENTRY32>() as u32;
                if Thread32Next(snapshot, &mut entry) == 0 { break; }
            }
        }
        CloseHandle(snapshot);
        Ok(suspended > 0)
    }
}

#[cfg(target_os = "windows")]
fn resume_windows(pid: u32) -> Result<bool, String> {
    use std::mem;
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Thread32First, Thread32Next,
        THREADENTRY32, TH32CS_SNAPTHREAD,
    };
    use windows_sys::Win32::System::Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME};

    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return Err(format!("CreateToolhelp32Snapshot failed for PID {}", pid));
        }
        let mut entry: THREADENTRY32 = mem::zeroed();
        entry.dwSize = mem::size_of::<THREADENTRY32>() as u32;
        let mut resumed = 0u32;
        if Thread32First(snapshot, &mut entry) != 0 {
            loop {
                if entry.th32OwnerProcessID == pid {
                    let th = OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID);
                    if th != std::ptr::null_mut() { ResumeThread(th); CloseHandle(th); resumed += 1; }
                }
                entry.dwSize = mem::size_of::<THREADENTRY32>() as u32;
                if Thread32Next(snapshot, &mut entry) == 0 { break; }
            }
        }
        CloseHandle(snapshot);
        Ok(resumed > 0)
    }
}

// ── LINUX ─────────────────────────────────────────────────────────────────────
// SIGSTOP freezes the entire process group. Fully reversible with SIGCONT.

#[cfg(target_os = "linux")]
fn suspend_linux(pid: u32) -> Result<bool, String> {
    use nix::sys::signal::{kill, Signal};
    use nix::unistd::Pid;
    kill(Pid::from_raw(pid as i32), Signal::SIGSTOP)
        .map(|_| true)
        .map_err(|e| format!("SIGSTOP failed for PID {}: {}", pid, e))
}

#[cfg(target_os = "linux")]
fn resume_linux(pid: u32) -> Result<bool, String> {
    use nix::sys::signal::{kill, Signal};
    use nix::unistd::Pid;
    kill(Pid::from_raw(pid as i32), Signal::SIGCONT)
        .map(|_| true)
        .map_err(|e| format!("SIGCONT failed for PID {}: {}", pid, e))
}

// ── macOS ─────────────────────────────────────────────────────────────────────
// Uses SIGSTOP same as Linux — Mach task_suspend is more granular but overkill for Phase 2.
#[cfg(target_os = "macos")]
fn suspend_macos(pid: u32) -> Result<bool, String> { suspend_linux(pid) }
#[cfg(target_os = "macos")]
fn resume_macos(pid: u32) -> Result<bool, String> { resume_linux(pid) }
