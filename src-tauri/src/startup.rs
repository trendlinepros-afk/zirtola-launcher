//! Startup diagnostics: a plain log of every start and a message box when the launcher can't open, so a launcher that
//! "flashes and goes away" says why.
//!
//! The log is `%LOCALAPPDATA%\Zirtola Launcher\logs\startup.log` (kept under 200 KB). It is written with plain file
//! appends before anything else in the launcher exists, so it works when Tauri or the WebView doesn't.

use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

static UI_ALIVE: AtomicBool = AtomicBool::new(false);
const MAX_LOG: u64 = 200 * 1024;

pub fn log_path() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join("Zirtola Launcher").join("logs").join("startup.log")
}

/// One line in the startup log. Never fails loudly: a launcher that can't log still starts.
pub fn note(msg: impl AsRef<str>) {
    let path = log_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if std::fs::metadata(&path).map(|m| m.len() > MAX_LOG).unwrap_or(false) {
        let _ = std::fs::rename(&path, path.with_extension("old.log"));
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(
            f,
            "{} [{}] {}",
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f"),
            std::process::id(),
            msg.as_ref()
        );
    }
}

/// A plain Windows message box (no Tauri, no WebView): the one thing that still works when the window can't open.
pub fn error_box(text: &str) {
    #[cfg(windows)]
    {
        use windows::core::PCWSTR;
        use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK, MB_SETFOREGROUND};
        let wide = |s: &str| s.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
        let (t, c) = (wide(text), wide("Zirtola Launcher"));
        unsafe {
            MessageBoxW(None, PCWSTR(t.as_ptr()), PCWSTR(c.as_ptr()), MB_OK | MB_ICONERROR | MB_SETFOREGROUND);
        }
    }
    #[cfg(not(windows))]
    eprintln!("{text}");
}

fn log_hint() -> String {
    format!(
        "\n\nA log of this start was saved here:\n{}\n\nPlease send that file to Zirtola support (Discord: #help).",
        log_path().display()
    )
}

/// Called first thing in `run()`: the start line, the panic hook, and the WebView2 check.
/// Returns false when the launcher can't start (the user has been told why).
pub fn begin() -> bool {
    let args: Vec<String> = std::env::args().skip(1).map(|a| if a.starts_with("zirtola://") { "zirtola://...".into() } else { a }).collect();
    note(format!(
        "---- start: launcher {} on {} ({}), exe {}, args {:?}",
        env!("CARGO_PKG_VERSION"),
        os_version(),
        std::env::consts::ARCH,
        std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_default(),
        args
    ));
    std::panic::set_hook(Box::new(|info| {
        let msg = info.to_string();
        note(format!("PANIC: {msg}"));
        // (only the first panic gets a box: a panic while closing shouldn't stack dialogs)
        static SHOWN: AtomicBool = AtomicBool::new(false);
        if !SHOWN.swap(true, Ordering::SeqCst) {
            error_box(&format!("The Zirtola Launcher hit an error and has to close.\n\n{msg}{}", log_hint()));
        }
    }));
    match tauri::webview_version() {
        Ok(v) => {
            note(format!("WebView2 runtime {v}"));
            match webview_block(&v) {
                Some(b) => note(format!("WebView2 BLOCKED: {b}")),
                None => note("WebView2 not blocked (no Image File Execution Options entry, runtime files present)"),
            }
            true
        }
        Err(e) => {
            note(format!("WebView2 runtime NOT FOUND: {e}"));
            error_box(&format!(
                "The Zirtola Launcher needs Microsoft Edge WebView2, and it is missing or damaged on this PC.\n\n\
                 Fix: download and run the \"Evergreen Standalone Installer\" from\n\
                 https://developer.microsoft.com/microsoft-edge/webview2/\nthen start the launcher again.{}",
                log_hint()
            ));
            false
        }
    }
}

/// The interface answered (its first command arrived): the launcher is really on screen.
pub fn ui_alive() {
    if !UI_ALIVE.swap(true, Ordering::SeqCst) {
        note("interface is running (first command received)");
    }
}

/// After the window is created: if the interface hasn't said anything in 25 s, the WebView didn't come up.
pub fn watch_ui() {
    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_secs(25));
        if !UI_ALIVE.load(Ordering::SeqCst) {
            note("interface did NOT start within 25 s (WebView2 not rendering: damaged runtime, graphics driver, or blocked by security software)");
            error_box(&format!(
                "The Zirtola Launcher's window did not come up.\n\n\
                 This is usually Microsoft Edge WebView2 (repair it under Windows Settings > Apps > Installed apps > \
                 Microsoft Edge WebView2 Runtime > Modify), an outdated graphics driver, or security software blocking it.{}",
                log_hint()
            ));
        }
    });
}

/// Tauri's own extra WebView2 switches; setting our own replaces them, so they are repeated here.
const BASE_ARGS: &str = "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection";

/// Make the main window. When WebView2 won't start its browser process (seen: 0x80080005 "Server execution
/// failed"), try again with a second profile folder (a stuck or damaged one is the usual cause), then with a
/// brand-new folder and the GPU off. The folder that worked is remembered for the next start.
pub fn create_main_window(app: &tauri::App) -> Result<(), String> {
    use tauri::Manager;
    let cfg = app
        .config()
        .app
        .windows
        .iter()
        .find(|w| w.label == "main")
        .cloned()
        .ok_or_else(|| "no main window in the config".to_string())?;
    let base = app.path().app_local_data_dir().map_err(|e| e.to_string())?;
    let marker = base.join("webview_profile.txt");
    let remembered = std::fs::read_to_string(&marker).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty());

    // (profile folder name or None for the default, extra switches)
    let mut tries: Vec<(Option<String>, String)> = Vec::new();
    if let Some(r) = &remembered {
        tries.push((Some(r.clone()), BASE_ARGS.to_string()));
    }
    tries.push((None, BASE_ARGS.to_string()));
    if remembered.as_deref() != Some("EBWebView2") {
        tries.push((Some("EBWebView2".into()), BASE_ARGS.to_string()));
    }
    let stamp = chrono::Local::now().format("%Y%m%d%H%M%S").to_string();
    tries.push((Some(format!("EBWebView-{stamp}")), format!("{BASE_ARGS} --disable-gpu")));
    // last: a fresh folder with none of our switches at all, as a plain WebView2 app would start
    tries.push((Some(format!("EBWebView-{stamp}-plain")), String::new()));

    // Retries (only reached when the first try failed) also write WebView2's own browser log, so a failure says why.
    let logs = log_path().parent().map(|p| p.to_path_buf()).unwrap_or_else(std::env::temp_dir);
    for (i, t) in tries.iter_mut().enumerate().skip(1) {
        let f = logs.join(format!("webview2_try{}.log", i + 1));
        let _ = std::fs::remove_file(&f);
        t.1 = format!("{} --enable-logging --v=1 --log-file=\"{}\"", t.1, f.display()).trim().to_string();
    }

    let mut last = String::new();
    for (i, (folder, args)) in tries.iter().enumerate() {
        let mut b = tauri::WebviewWindowBuilder::from_config(app, &cfg).map_err(|e| e.to_string())?;
        let test_fail = std::env::var("ZIRTOLA_TEST_WEBVIEW_FAIL").ok();
        if folder.is_some() && test_fail.as_deref() != Some("all") {
            b = b.data_directory(base.join(folder.as_deref().unwrap_or_default()));
        } else if test_fail.is_some() {
            // (test only: a profile folder under a file can't be made, so the first try fails like the real fault)
            let block = base.join("webview_test_block");
            let _ = std::fs::create_dir_all(&base);
            let _ = std::fs::write(&block, "");
            b = b.data_directory(block.join("x"));
        }
        b = b.additional_browser_args(args);
        match b.build() {
            Ok(_) => {
                note(format!(
                    "window created (try {} of {}, profile {}, switches {args})",
                    i + 1,
                    tries.len(),
                    folder.as_deref().unwrap_or("default")
                ));
                if remembered.as_deref() != folder.as_deref() {
                    let _ = std::fs::write(&marker, folder.as_deref().unwrap_or(""));
                }
                return Ok(());
            }
            Err(e) => {
                last = e.to_string();
                note(format!("window try {} failed (profile {}): {last}", i + 1, folder.as_deref().unwrap_or("default")));
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.destroy();
                }
            }
        }
    }
    diagnose(&base, &logs, tries.len());
    Err(last)
}

/// Every try failed: put what WebView2 itself said into the startup log, any crash reports its browser process left,
/// and the programs known to inject into other apps' processes (a common reason WebView2 dies on one PC).
fn diagnose(base: &std::path::Path, logs: &std::path::Path, n: usize) {
    for i in 2..=n {
        let f = logs.join(format!("webview2_try{i}.log"));
        match std::fs::read_to_string(&f) {
            Ok(s) => {
                let lines: Vec<&str> = s.lines().filter(|l| !l.trim().is_empty()).collect();
                note(format!("WebView2's own log, try {i}: {} lines; last ones:", lines.len()));
                for l in lines.iter().rev().take(25).rev() {
                    note(format!("    {}", l.chars().take(300).collect::<String>()));
                }
            }
            Err(_) => note(format!("WebView2's own log, try {i}: none written (its browser process never started)")),
        }
    }
    let mut dumps: Vec<(std::time::SystemTime, String)> = Vec::new();
    fn walk(d: &std::path::Path, depth: u32, out: &mut Vec<(std::time::SystemTime, String)>) {
        if depth > 6 {
            return;
        }
        if let Ok(rd) = std::fs::read_dir(d) {
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(&p, depth + 1, out);
                } else if p.extension().map(|x| x == "dmp").unwrap_or(false) {
                    let t = e.metadata().and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH);
                    out.push((t, p.display().to_string()));
                }
            }
        }
    }
    walk(base, 0, &mut dumps);
    dumps.sort();
    note(format!("WebView2 crash reports under {}: {}", base.display(), dumps.len()));
    for (_, p) in dumps.iter().rev().take(3) {
        note(format!("    {p}"));
    }
    let inj = injectors();
    note(if inj.is_empty() {
        "known injecting programs running: none".to_string()
    } else {
        format!("known injecting programs running: {}", inj.join(", "))
    });
}

/// Running programs (by exe name) that are known to inject into other processes: overlays, audio "enhancers",
/// RGB/peripheral suites, some security tools. Names only; nothing else about them is read.
pub fn injectors() -> Vec<String> {
    const KNOWN: &[&str] = &[
        "rtss", "msiafterburner", "nahimic", "a-volute", "sonicstudio", "sonic studio", "rzsynapse", "razer",
        "overwolf", "wallpaper32", "wallpaper64", "fraps", "bdagent", "vsserv", "avp.exe", "avastui", "avgui",
        "mcshield", "egui.exe", "ekrn", "sbiesvc", "sandboxie", "eac_", "battleye", "vanguard", "vgc.exe",
        "faceit", "dragoncenter", "armoury", "lghub", "icue", "steelseries", "displayfusion", "stardock",
        "windowblinds", "start11", "fences", "explorerpatcher", "startallback", "medal", "obs64", "action_x64",
        "reshade", "specialk", "presentmon",
    ];
    let mut out: Vec<String> = Vec::new();
    #[cfg(windows)]
    unsafe {
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
        };
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return out };
        let mut e = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut ok = Process32FirstW(snap, &mut e).is_ok();
        while ok {
            let len = e.szExeFile.iter().position(|&c| c == 0).unwrap_or(e.szExeFile.len());
            let name = String::from_utf16_lossy(&e.szExeFile[..len]);
            let low = name.to_lowercase();
            if KNOWN.iter().any(|k| low.contains(k)) && !out.contains(&name) {
                out.push(name);
            }
            ok = Process32NextW(snap, &mut e).is_ok();
        }
        let _ = CloseHandle(snap);
    }
    out
}

/// Why WebView2 can't run on this PC, when we can tell. Tools that "disable" or "debloat" Microsoft Edge commonly
/// stop msedgewebview2.exe with an Image File Execution Options "Debugger" entry, or delete the runtime's files;
/// Windows then refuses to start it (0x80080005 "Server execution failed").
pub fn webview_block(version: &str) -> Option<String> {
    if std::env::var_os("ZIRTOLA_TEST_WEBVIEW_BLOCKED").is_some() {
        return Some("test: pretend a tool blocked msedgewebview2.exe".into());
    }
    #[cfg(windows)]
    {
        use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY};
        let ifeo = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Image File Execution Options\msedgewebview2.exe";
        let hklm = winreg::RegKey::predef(HKEY_LOCAL_MACHINE);
        for view in [KEY_WOW64_64KEY, KEY_WOW64_32KEY] {
            if let Ok(k) = hklm.open_subkey_with_flags(ifeo, KEY_READ | view) {
                if let Ok(d) = k.get_value::<String, _>("Debugger") {
                    return Some(format!("an Image File Execution Options entry stops msedgewebview2.exe from starting (Debugger = \"{d}\")"));
                }
            }
        }
        // the runtime's own exe, where the registry says the runtime is
        let mut roots: Vec<std::path::PathBuf> = Vec::new();
        for var in ["ProgramFiles(x86)", "ProgramFiles"] {
            if let Some(p) = std::env::var_os(var) {
                roots.push(std::path::PathBuf::from(p).join(r"Microsoft\EdgeWebView\Application"));
            }
        }
        if let Some(p) = std::env::var_os("LOCALAPPDATA") {
            roots.push(std::path::PathBuf::from(p).join(r"Microsoft\EdgeWebView\Application"));
        }
        let present = roots.iter().any(|r| r.join(version).join("msedgewebview2.exe").is_file());
        let any_root = roots.iter().any(|r| r.is_dir());
        if any_root && !present {
            return Some(format!("WebView2 {version} is registered but its msedgewebview2.exe is missing (removed by a tool or a broken install)"));
        }
    }
    None
}

/// Every window try failed: say what to do in plain steps.
pub fn failed_webview(err: &str) {
    note(format!("FAILED creating its window after every try: {err}"));
    let block = tauri::webview_version().ok().and_then(|v| webview_block(&v));
    if let Some(b) = block {
        error_box(&format!(
            "The Zirtola Launcher can't open because Microsoft Edge WebView2 is blocked on this PC.\n\n\
             Found: {b}.\n\n\
             This usually comes from a tool used to disable or \"debloat\" Microsoft Edge. You don't have to use Edge \
             as your browser, but the launcher's window is drawn by WebView2 (as in many other apps), so it must be allowed to run.\n\n\
             To fix it:\n\
             1. In the tool you used to disable Edge, re-enable Edge / WebView2 (or undo its Edge changes).\n\
             2. Reinstall the WebView2 Runtime: https://developer.microsoft.com/microsoft-edge/webview2/ (Evergreen Standalone Installer, x64).\n\
             3. Start the launcher again.{}",
            log_hint()
        ));
        return;
    }
    error_box(&format!(
        "The Zirtola Launcher's window could not start, because Windows wouldn't start Microsoft Edge WebView2 for it.\n\n\
         Please try, in order:\n\
         1. Restart the PC (this clears a stuck WebView2), then open the launcher normally (not \"Run as administrator\").\n\
         2. Windows Settings > Apps > Installed apps > Microsoft Edge WebView2 Runtime > Modify > Repair.\n\
         3. If you use antivirus other than Windows Security, allow zirtola-launcher.exe and msedgewebview2.exe.\n\
         4. Close overlay, audio-enhancer or RGB programs for a moment and try again{}.\n\n\
         Details: {err}{}",
        {
            let inj = injectors();
            if inj.is_empty() {
                String::new()
            } else {
                let more = if inj.len() > 5 { format!(" and {} more", inj.len() - 5) } else { String::new() };
                format!(" (running now: {}{more})", inj[..inj.len().min(5)].join(", "))
            }
        },
        log_hint()
    ));
}

/// `run()` returned an error instead of a window.
pub fn failed(what: &str, err: &dyn std::fmt::Display) {
    note(format!("FAILED {what}: {err}"));
    error_box(&format!("The Zirtola Launcher could not start ({what}).\n\n{err}{}", log_hint()));
}

fn os_version() -> String {
    #[cfg(windows)]
    {
        let key = winreg::RegKey::predef(winreg::enums::HKEY_LOCAL_MACHINE)
            .open_subkey(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion");
        if let Ok(k) = key {
            let name: String = k.get_value("ProductName").unwrap_or_default();
            let build: String = k.get_value("CurrentBuildNumber").unwrap_or_default();
            let disp: String = k.get_value("DisplayVersion").unwrap_or_default();
            return format!("{name} {disp} build {build}");
        }
    }
    std::env::consts::OS.to_string()
}
