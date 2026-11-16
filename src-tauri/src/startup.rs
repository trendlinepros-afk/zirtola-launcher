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

    let mut last = String::new();
    for (i, (folder, args)) in tries.iter().enumerate() {
        let mut b = tauri::WebviewWindowBuilder::from_config(app, &cfg).map_err(|e| e.to_string())?;
        if let Some(f) = folder {
            b = b.data_directory(base.join(f));
        } else if std::env::var_os("ZIRTOLA_TEST_WEBVIEW_FAIL").is_some() {
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
    Err(last)
}

/// Every window try failed: say what to do in plain steps.
pub fn failed_webview(err: &str) {
    note(format!("FAILED creating its window after every try: {err}"));
    error_box(&format!(
        "The Zirtola Launcher's window could not start, because Windows wouldn't start Microsoft Edge WebView2 for it.\n\n\
         Please try, in order:\n\
         1. Restart the PC (this clears a stuck WebView2), then open the launcher normally (not \"Run as administrator\").\n\
         2. Windows Settings > Apps > Installed apps > Microsoft Edge WebView2 Runtime > Modify > Repair.\n\
         3. If you use antivirus other than Windows Security, allow zirtola-launcher.exe and msedgewebview2.exe.\n\n\
         Details: {err}{}",
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
