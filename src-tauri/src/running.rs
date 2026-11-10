//! Game processes still running from an install folder.
//!
//! An update's last step replaces the game's files. A game whose window has
//! closed but whose process never exited keeps its exe locked, and the player
//! sees "in use" with nothing on screen to close. This finds those processes
//! and tells a game that is really open from one that was left behind.

use std::path::Path;

#[derive(Debug, Clone)]
pub struct GameProc {
    pub pid: u32,
    pub exe: String,
    /// A visible top-level window belongs to it: the player has the game open.
    pub has_window: bool,
    /// Seconds since it started.
    pub age_s: u64,
}

/// Processes whose exe lives under `root`.
#[cfg(windows)]
pub fn under(root: &Path) -> Vec<GameProc> {
    use windows::core::{BOOL, PWSTR};
    use windows::Win32::Foundation::{CloseHandle, FILETIME, HWND, LPARAM};
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
    };
    use windows::Win32::System::SystemInformation::GetSystemTimeAsFileTime;
    use windows::Win32::System::Threading::{
        GetProcessTimes, OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindowThreadProcessId, IsWindowVisible};

    fn norm(p: &str) -> String {
        p.replace('/', "\\").trim_start_matches("\\\\?\\").to_lowercase()
    }
    let mut root_s = norm(&root.to_string_lossy());
    if !root_s.ends_with('\\') {
        root_s.push('\\');
    }
    let me = std::process::id();

    // pids that own a visible top-level window
    unsafe extern "system" fn each(hwnd: HWND, l: LPARAM) -> BOOL {
        let v = &mut *(l.0 as *mut Vec<u32>);
        if IsWindowVisible(hwnd).as_bool() {
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            v.push(pid);
        }
        BOOL(1)
    }
    let mut windowed: Vec<u32> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(each), LPARAM(&mut windowed as *mut _ as isize));
    }

    let mut out = Vec::new();
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return out };
        let mut e = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut ok = Process32FirstW(snap, &mut e).is_ok();
        while ok {
            let pid = e.th32ProcessID;
            if pid != 0 && pid != me {
                if let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
                    let mut buf = [0u16; 1024];
                    let mut len = buf.len() as u32;
                    if QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok() {
                        let exe = String::from_utf16_lossy(&buf[..len as usize]);
                        if norm(&exe).starts_with(&root_s) {
                            let (mut c, mut x, mut k, mut u) =
                                (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
                            let mut age_s = u64::MAX;
                            if GetProcessTimes(h, &mut c, &mut x, &mut k, &mut u).is_ok() {
                                let now = GetSystemTimeAsFileTime();
                                let t = |f: FILETIME| ((f.dwHighDateTime as u64) << 32) | f.dwLowDateTime as u64;
                                age_s = t(now).saturating_sub(t(c)) / 10_000_000;
                            }
                            out.push(GameProc { pid, exe, has_window: windowed.contains(&pid), age_s });
                        }
                    }
                    let _ = CloseHandle(h);
                }
            }
            ok = Process32NextW(snap, &mut e).is_ok();
        }
        let _ = CloseHandle(snap);
    }
    out
}

#[cfg(not(windows))]
pub fn under(_root: &Path) -> Vec<GameProc> {
    Vec::new()
}

/// End a process and wait (up to 5 s) for it to go. True if it is gone.
#[cfg(windows)]
pub fn end(pid: u32) -> bool {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        OpenProcess, TerminateProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
    };
    unsafe {
        let Ok(h) = OpenProcess(PROCESS_TERMINATE | PROCESS_SYNCHRONIZE, false, pid) else { return false };
        let _ = TerminateProcess(h, 1);
        let gone = WaitForSingleObject(h, 5000).0 == 0;
        let _ = CloseHandle(h);
        gone
    }
}

#[cfg(not(windows))]
pub fn end(_pid: u32) -> bool {
    false
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::os::windows::process::CommandExt;

    #[test]
    fn finds_and_ends_a_windowless_process_under_a_folder() {
        let dir = std::env::temp_dir().join(format!("zl-running-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("leftover.exe");
        std::fs::copy(r"C:\Windows\System32\ping.exe", &exe).unwrap();
        let mut child = std::process::Command::new(&exe)
            .args(["-n", "30", "127.0.0.1"])
            .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(500));
        let found = under(&dir);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].pid, child.id());
        assert!(!found[0].has_window);
        assert!(found[0].age_s < 60);
        // the exe is locked while it runs
        assert!(std::fs::remove_file(&exe).is_err());
        assert!(end(found[0].pid));
        let _ = child.wait();
        assert!(under(&dir).is_empty());
        std::fs::remove_file(&exe).unwrap();
        let _ = std::fs::remove_dir(&dir);
        // and nothing of ours is reported under an unrelated folder
        assert!(under(std::path::Path::new(r"C:
o\sucholder")).is_empty());
    }
}
