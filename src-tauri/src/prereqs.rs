//! Requirements check before a game runs: Windows version, the Microsoft
//! Visual C++ runtime, a DirectX 12 GPU with Shader Model 6 and its driver,
//! and free disk space. Only the VC++ runtime can be installed from here; the
//! rest show advice.
//!
//! The VC++ installer comes only from official, signed sources: the copy
//! Unreal ships inside the game (Engine/Extras/Redist/en-us/vc_redist.x64.exe,
//! listed in the game's signed manifest and checked against its SHA-256), or
//! Microsoft's own download (aka.ms). Either way its Authenticode signature
//! must be valid and issued to Microsoft Corporation before it runs, with
//! Windows' own administrator prompt.
//!
//! Testing (DEV builds only): ZIRTOLA_PREREQ_SIMULATE=vcredist,gpu,disk,windows
//! reports those as missing; ZIRTOLA_PREREQ_SIMULATE_FAIL=vcredist makes the
//! first simulated install fail. Simulated installs never run anything.

use serde::Serialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Oldest VC++ runtime the games accept (Unreal 5.8 ships 14.50).
const MIN_VCRT: (u32, u32, u32) = (14, 50, 0);
/// Windows 10 22H2.
const MIN_WINDOWS_BUILD: u32 = 19045;
const VCREDIST_URL: &str = "https://aka.ms/vs/17/release/vc_redist.x64.exe";
const VCREDIST_INFO_URL: &str = "https://learn.microsoft.com/cpp/windows/latest-supported-vc-redist";
const VCREDIST_IN_GAME: &str = "Engine/Extras/Redist/en-us/vc_redist.x64.exe";

#[derive(Debug, Clone, Serialize)]
pub struct Item {
    pub id: &'static str,
    pub name: String,
    pub description: String,
    /// "ok", "missing" (can be installed here) or "unsupported" (advice only).
    pub status: &'static str,
    pub found: String,
    pub required: bool,
    pub fixable: bool,
    /// Rough size of what installing it downloads or runs.
    pub size_bytes: Option<u64>,
    pub source: Option<String>,
    pub advice: Option<String>,
    pub advice_url: Option<String>,
    pub manual_url: Option<String>,
}

// ---------------------------------------------------------------- simulation (DEV only)

static SIM_INSTALLED: Mutex<Option<HashSet<String>>> = Mutex::new(None);
static SIM_FAILED_ONCE: Mutex<Option<HashSet<String>>> = Mutex::new(None);

fn sim_list(var: &str) -> HashSet<String> {
    if !crate::state::dev_mode() {
        return HashSet::new();
    }
    std::env::var(var)
        .unwrap_or_default()
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn simulated_missing(id: &str) -> bool {
    sim_list("ZIRTOLA_PREREQ_SIMULATE").contains(id)
        && !SIM_INSTALLED.lock().unwrap().get_or_insert_with(HashSet::new).contains(id)
}

// ---------------------------------------------------------------- checks

pub struct CheckInput {
    /// Bytes the game still needs on disk (0 when installed and current).
    pub need_bytes: u64,
    /// Where it is or will be installed.
    pub install_dir: PathBuf,
    /// Installed game folder, to find the VC++ installer Unreal ships.
    pub game_root: Option<PathBuf>,
}

pub fn check(input: &CheckInput) -> Vec<Item> {
    vec![windows_item(), vcredist_item(input), gpu_item(), disk_item(input)]
}

fn windows_item() -> Item {
    let (name, display, build) = windows_version();
    let ok = build >= MIN_WINDOWS_BUILD && !simulated_missing("windows");
    Item {
        id: "windows",
        name: "Windows 10 (22H2) or Windows 11, 64-bit".into(),
        description: "The game needs a recent 64-bit Windows.".into(),
        status: if ok { "ok" } else { "unsupported" },
        found: if simulated_missing("windows") { "Windows 10 1909 (18363) (simulated)".into() } else { format!("{name} {display} (build {build})").replace("  ", " ") },
        required: true,
        fixable: false,
        size_bytes: None,
        source: None,
        advice: (!ok).then(|| "Update Windows: Settings → Windows Update.".into()),
        advice_url: (!ok).then(|| "https://support.microsoft.com/windows/update-windows-3c5ae7fc-9fb6-9af1-1984-b5e0412c556a".into()),
        manual_url: None,
    }
}

fn windows_version() -> (String, String, u32) {
    #[cfg(windows)]
    {
        use winreg::enums::HKEY_LOCAL_MACHINE;
        let hklm = winreg::RegKey::predef(HKEY_LOCAL_MACHINE);
        if let Ok(k) = hklm.open_subkey("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion") {
            let build: u32 = k.get_value::<String, _>("CurrentBuildNumber").ok().and_then(|b| b.trim().parse().ok()).unwrap_or(0);
            let mut name: String = k.get_value("ProductName").unwrap_or_else(|_| "Windows".into());
            if build >= 22000 {
                name = name.replace("Windows 10", "Windows 11");
            }
            let display: String = k.get_value("DisplayVersion").unwrap_or_default();
            return (name, display, build);
        }
    }
    ("Windows".into(), String::new(), 0)
}

/// Installed VC++ 2015–2022 x64 runtime version, from the registry key the
/// redistributable writes, double-checked against the DLLs in System32.
fn installed_vcrt() -> Option<(u32, u32, u32)> {
    #[cfg(windows)]
    {
        use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_64KEY};
        let hklm = winreg::RegKey::predef(HKEY_LOCAL_MACHINE);
        let k = hklm
            .open_subkey_with_flags("SOFTWARE\\Microsoft\\VisualStudio\\14.0\\VC\\Runtimes\\x64", KEY_READ | KEY_WOW64_64KEY)
            .ok()?;
        let installed: u32 = k.get_value("Installed").unwrap_or(0);
        if installed != 1 {
            return None;
        }
        let major: u32 = k.get_value("Major").ok()?;
        let minor: u32 = k.get_value("Minor").ok()?;
        let bld: u32 = k.get_value("Bld").unwrap_or(0);
        let sys = std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("C:\\Windows")).join("System32");
        let dlls_present = ["vcruntime140.dll", "vcruntime140_1.dll", "msvcp140.dll"].iter().all(|d| sys.join(d).is_file());
        return dlls_present.then_some((major, minor, bld));
    }
    #[allow(unreachable_code)]
    None
}

fn vcredist_item(input: &CheckInput) -> Item {
    let found = if simulated_missing("vcredist") { None } else { installed_vcrt() };
    let ok = found.is_some_and(|v| v >= MIN_VCRT);
    let (source, size) = match in_game_installer(input.game_root.as_deref()) {
        Some((_, size)) => ("included with the game".to_string(), size),
        None => ("download from Microsoft (aka.ms)".to_string(), 25 * 1024 * 1024),
    };
    Item {
        id: "vcredist",
        name: "Microsoft Visual C++ Runtime (2015–2022, x64)".into(),
        description: "Code libraries from Microsoft that games built with Visual Studio need to start.".into(),
        status: if ok { "ok" } else { "missing" },
        found: match found {
            Some((a, b, c)) if ok => format!("Version {a}.{b}.{c} installed"),
            Some((a, b, c)) => format!("Version {a}.{b}.{c} installed; {}.{} or newer needed", MIN_VCRT.0, MIN_VCRT.1),
            None => "Not installed".into(),
        },
        required: true,
        fixable: !ok,
        size_bytes: (!ok).then_some(size),
        source: (!ok).then_some(source),
        advice: None,
        advice_url: None,
        manual_url: Some(VCREDIST_INFO_URL.into()),
    }
}

/// The VC++ installer inside an installed game, if the game's signed manifest
/// lists it and the file on disk matches the manifest's SHA-256.
fn in_game_installer(root: Option<&Path>) -> Option<(PathBuf, u64)> {
    let root = root?;
    let (_, m) = zirtola_core::install::installed_manifest(root).ok().flatten()?;
    let entry = m.files.iter().find(|f| f.path.eq_ignore_ascii_case(VCREDIST_IN_GAME))?;
    let path = zirtola_core::install::file_path(root, &entry.path).ok()?;
    let data = std::fs::read(&path).ok()?;
    use sha2::Digest;
    (format!("{:x}", sha2::Sha256::digest(&data)) == entry.sha256).then_some((path, entry.size))
}

struct GpuInfo {
    name: String,
    driver: String,
    vendor: u32,
    feature_level: Option<&'static str>,
    shader_model: Option<(u32, u32)>,
}

fn gpu_item() -> Item {
    let gpus = if simulated_missing("gpu") {
        vec![GpuInfo { name: "Intel(R) HD Graphics 4000 (simulated)".into(), driver: "10.18.10.4252".into(), vendor: 0x8086, feature_level: Some("11_0"), shader_model: None }]
    } else {
        dx12_gpus()
    };
    let best = gpus.iter().find(|g| g.shader_model.is_some_and(|s| s >= (6, 0)));
    let shown = best.or(gpus.first());
    let ok = best.is_some();
    let vendor_url = |v: u32| match v {
        0x10DE => "https://www.nvidia.com/drivers",
        0x1002 => "https://www.amd.com/en/support/download/drivers.html",
        0x8086 => "https://www.intel.com/content/www/us/en/support/detect.html",
        _ => "https://support.microsoft.com/windows/update-drivers-manually-in-windows-ec62f46c-ff14-c91d-eead-d7126dc1f7b6",
    };
    Item {
        id: "gpu",
        name: "Graphics card with DirectX 12 and Shader Model 6".into(),
        description: "The game draws with DirectX 12 (feature level 12_0, Shader Model 6).".into(),
        status: if ok { "ok" } else { "unsupported" },
        found: match shown {
            Some(g) => {
                let mut s = g.name.clone();
                if let Some(fl) = g.feature_level.filter(|fl| fl.starts_with("12")) {
                    s.push_str(&format!(" · DirectX 12 (feature level {fl})"));
                } else if let Some(fl) = g.feature_level {
                    s.push_str(&format!(" · no DirectX 12 (feature level {fl} only)"));
                } else {
                    s.push_str(" · no DirectX 12");
                }
                if let Some((a, b)) = g.shader_model {
                    s.push_str(&format!(" · Shader Model {a}.{b}"));
                }
                if !g.driver.is_empty() {
                    s.push_str(&format!(" · driver {}", g.driver));
                }
                s
            }
            None => "No graphics card found".into(),
        },
        required: true,
        fixable: false,
        size_bytes: None,
        source: None,
        advice: (!ok).then(|| match shown {
            Some(g) if g.feature_level.is_some_and(|fl| fl.starts_with("12")) => "Update your graphics driver. If that doesn't help, this graphics card is too old for the game.".into(),
            _ => "Update your graphics driver. Older graphics cards without DirectX 12 can't run the game.".into(),
        }),
        advice_url: (!ok).then(|| vendor_url(shown.map(|g| g.vendor).unwrap_or(0)).into()),
        manual_url: None,
    }
}

#[cfg(windows)]
fn dx12_gpus() -> Vec<GpuInfo> {
    use windows::core::Interface;
    use windows::Win32::Graphics::Direct3D::{D3D_FEATURE_LEVEL, D3D_FEATURE_LEVEL_12_0, D3D_FEATURE_LEVEL_12_1, D3D_FEATURE_LEVEL_12_2};
    use windows::Win32::Graphics::Direct3D12::{
        D3D12CreateDevice, ID3D12Device, D3D12_FEATURE_DATA_SHADER_MODEL, D3D12_FEATURE_SHADER_MODEL, D3D_SHADER_MODEL,
    };
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIDevice, IDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE};
    let mut out = Vec::new();
    let Ok(factory) = (unsafe { CreateDXGIFactory1::<IDXGIFactory1>() }) else {
        return out;
    };
    let mut i = 0u32;
    while let Ok(adapter) = unsafe { factory.EnumAdapters1(i) } {
        i += 1;
        let Ok(desc) = (unsafe { adapter.GetDesc1() }) else { continue };
        if desc.Flags & (DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32) != 0 {
            continue;
        }
        let end = desc.Description.iter().position(|&c| c == 0).unwrap_or(desc.Description.len());
        let name = String::from_utf16_lossy(&desc.Description[..end]).trim().to_string();
        let driver = unsafe { adapter.CheckInterfaceSupport(&IDXGIDevice::IID) }
            .map(|v| {
                let v = v as u64;
                format!("{}.{}.{}.{}", (v >> 48) & 0xffff, (v >> 32) & 0xffff, (v >> 16) & 0xffff, v & 0xffff)
            })
            .unwrap_or_default();
        let mut feature_level = None;
        let mut shader_model = None;
        let levels: [(D3D_FEATURE_LEVEL, &str); 3] = [(D3D_FEATURE_LEVEL_12_2, "12_2"), (D3D_FEATURE_LEVEL_12_1, "12_1"), (D3D_FEATURE_LEVEL_12_0, "12_0")];
        for (fl, label) in levels {
            let mut dev: Option<ID3D12Device> = None;
            if unsafe { D3D12CreateDevice(&adapter, fl, &mut dev) }.is_ok() {
                if let Some(dev) = dev {
                    feature_level = Some(label);
                    // Ask for the newest model first; the runtime refuses ones it doesn't know.
                    for sm in (0x60..=0x69).rev() {
                        let mut data = D3D12_FEATURE_DATA_SHADER_MODEL { HighestShaderModel: D3D_SHADER_MODEL(sm) };
                        let r = unsafe {
                            dev.CheckFeatureSupport(
                                D3D12_FEATURE_SHADER_MODEL,
                                &mut data as *mut _ as *mut core::ffi::c_void,
                                std::mem::size_of::<D3D12_FEATURE_DATA_SHADER_MODEL>() as u32,
                            )
                        };
                        if r.is_ok() {
                            let v = data.HighestShaderModel.0 as u32;
                            if v >= 0x60 {
                                shader_model = Some((v >> 4, v & 0xf));
                            }
                            break;
                        }
                    }
                }
                break;
            }
        }
        if !name.is_empty() && !out.iter().any(|g: &GpuInfo| g.name == name) {
            out.push(GpuInfo { name, driver, vendor: desc.VendorId, feature_level, shader_model });
        }
    }
    // Best card first.
    out.sort_by(|a, b| b.shader_model.cmp(&a.shader_model));
    out
}

#[cfg(not(windows))]
fn dx12_gpus() -> Vec<GpuInfo> {
    Vec::new()
}

fn disk_item(input: &CheckInput) -> Item {
    let free = zirtola_core::install::available_space(&input.install_dir).unwrap_or(0);
    let free = if simulated_missing("disk") { input.need_bytes / 4 } else { free };
    let need = input.need_bytes + input.need_bytes / 10;
    let ok = free >= need;
    let gb = |b: u64| format!("{:.1} GB", b as f64 / 1e9);
    let drive = input.install_dir.components().next().map(|c| c.as_os_str().to_string_lossy().to_string()).unwrap_or_default();
    Item {
        id: "disk",
        name: "Free disk space".into(),
        description: if input.need_bytes > 0 { format!("Room for the game files ({} plus a little working space).", gb(input.need_bytes)) } else { "Room for game files and updates.".into() },
        status: if ok { "ok" } else { "unsupported" },
        found: format!("{} free on {}", gb(free), drive),
        required: true,
        fixable: false,
        size_bytes: None,
        source: None,
        advice: (!ok).then(|| format!("Free up at least {} on {} (Settings → System → Storage), or install to another drive.", gb(need.saturating_sub(free)), drive)),
        advice_url: (!ok).then(|| "https://support.microsoft.com/windows/free-up-drive-space-in-windows-85529ccb-c365-490d-b548-831022bc9b32".into()),
        manual_url: None,
    }
}

// ---------------------------------------------------------------- installing

#[derive(Debug, Clone, Serialize)]
pub struct Progress {
    pub id: String,
    /// "download", "verify", "install", "done" or "error"
    pub phase: &'static str,
    pub done: u64,
    pub total: u64,
    pub message: Option<String>,
}

/// Install one fixable item. `emit` reports progress to the UI.
pub async fn install(
    id: &str,
    http: &reqwest::Client,
    game_root: Option<PathBuf>,
    emit: impl Fn(Progress) + Send + Sync,
) -> Result<(), String> {
    match id {
        "vcredist" => {}
        _ => return Err(format!("{id} can't be installed from the launcher.")),
    }
    let send = |phase: &'static str, done: u64, total: u64, message: Option<String>| emit(Progress { id: id.into(), phase, done, total, message });

    if sim_list("ZIRTOLA_PREREQ_SIMULATE").contains(id) {
        return simulate_install(id, &send).await;
    }

    let (installer, total) = match in_game_installer(game_root.as_deref()) {
        Some((p, size)) => {
            send("verify", size, size, Some("Using the copy included with the game".into()));
            (p, size)
        }
        None => download_vcredist(http, &send).await?,
    };
    send("verify", total, total, Some("Checking Microsoft's signature".into()));
    verify_microsoft_signature(&installer)?;
    send("install", 0, 0, Some("Installing (Windows may ask for permission)".into()));
    let code = run_elevated(&installer, &["/install", "/quiet", "/norestart"]).await?;
    match code {
        0 | 1638 => {}
        3010 => send("install", 0, 0, Some("Installed. Windows recommends a restart, but games can start now.".into())),
        1223 | 1602 => return Err("Windows' permission prompt was declined, so nothing was installed.".into()),
        1618 => return Err("Another installation is running. Wait for it to finish, then retry.".into()),
        other => return Err(format!("Microsoft's installer stopped with code {other}.")),
    }
    if installed_vcrt().is_some_and(|v| v >= MIN_VCRT) {
        send("done", total, total, None);
        Ok(())
    } else {
        Err("The installer finished, but the runtime still isn't there. Try installing it manually from Microsoft.".into())
    }
}

async fn simulate_install(id: &str, send: &(impl Fn(&'static str, u64, u64, Option<String>) + Sync)) -> Result<(), String> {
    let total = 25 * 1024 * 1024u64;
    for step in 1..=20u64 {
        tokio::time::sleep(std::time::Duration::from_millis(90)).await;
        send("download", total * step / 20, total, Some("Downloading from Microsoft (simulated)".into()));
    }
    send("verify", total, total, Some("Checking Microsoft's signature (simulated)".into()));
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    send("install", 0, 0, Some("Installing (simulated: Windows would ask for permission here)".into()));
    tokio::time::sleep(std::time::Duration::from_millis(1200)).await;
    let fail = sim_list("ZIRTOLA_PREREQ_SIMULATE_FAIL").contains(id)
        && SIM_FAILED_ONCE.lock().unwrap().get_or_insert_with(HashSet::new).insert(id.to_string());
    if fail {
        return Err("Microsoft's installer stopped with code 1603 (simulated failure).".into());
    }
    SIM_INSTALLED.lock().unwrap().get_or_insert_with(HashSet::new).insert(id.to_string());
    send("done", total, total, None);
    Ok(())
}

async fn download_vcredist(
    http: &reqwest::Client,
    send: &(impl Fn(&'static str, u64, u64, Option<String>) + Sync),
) -> Result<(PathBuf, u64), String> {
    use futures::StreamExt;
    let dir = std::env::temp_dir().join("zirtola-prereqs");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join("vc_redist.x64.exe");
    let resp = http
        .get(VCREDIST_URL)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| format!("Couldn't download from Microsoft: {e}"))?;
    let total = resp.content_length().unwrap_or(25 * 1024 * 1024);
    let mut file = std::fs::File::create(&path).map_err(|e| e.to_string())?;
    let mut done = 0u64;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("Download interrupted: {e}"))?;
        std::io::Write::write_all(&mut file, &chunk).map_err(|e| e.to_string())?;
        done += chunk.len() as u64;
        send("download", done, total, Some("Downloading from Microsoft".into()));
    }
    Ok((path, done))
}

/// Authenticode must be valid and signed by Microsoft Corporation.
fn verify_microsoft_signature(path: &Path) -> Result<(), String> {
    let out = powershell(
        "Import-Module Microsoft.PowerShell.Security; $s = Get-AuthenticodeSignature -LiteralPath $env:ZIRTOLA_FILE; Write-Output ('{0}|{1}' -f $s.Status, $s.SignerCertificate.Subject)",
        path,
    )
    .map_err(|e| format!("Couldn't check the installer's signature: {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let (status, subject) = text.split_once('|').unwrap_or((text.as_str(), ""));
    if status == "Valid" && subject.contains("O=Microsoft Corporation") {
        Ok(())
    } else {
        let _ = std::fs::remove_file(path).ok().filter(|_| path.starts_with(std::env::temp_dir()));
        let why = if status.is_empty() { String::from_utf8_lossy(&out.stderr).lines().next().unwrap_or("no result").trim().to_string() } else { status.to_string() };
        Err(format!("The installer isn't validly signed by Microsoft ({why}), so it was not run."))
    }
}

/// Run with Windows' administrator prompt (UAC); returns the exit code
/// (1223 when the prompt was declined).
async fn run_elevated(path: &Path, args: &[&str]) -> Result<i32, String> {
    let script = format!(
        "try {{ $p = Start-Process -FilePath $env:ZIRTOLA_FILE -ArgumentList {} -Verb RunAs -Wait -PassThru -WindowStyle Hidden; exit $p.ExitCode }} catch {{ exit 1223 }}",
        args.iter().map(|a| format!("'{a}'")).collect::<Vec<_>>().join(",")
    );
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || powershell(&script, &path))
        .await
        .map_err(|e| e.to_string())?
        .map(|o| o.status.code().unwrap_or(-1))
        .map_err(|e| e.to_string())
}

fn powershell(script: &str, file: &Path) -> std::io::Result<std::process::Output> {
    let mut cmd = std::process::Command::new("powershell.exe");
    // Windows PowerShell 5.1 started from PowerShell 7 inherits PS7's module
    // path and then can't load its own Security module (Get-AuthenticodeSignature):
    // give it its default module path.
    cmd.args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", script])
        .env("ZIRTOLA_FILE", file)
        .env_remove("PSModulePath");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    cmd.output()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn real_checks_run_and_describe_this_pc() {
        let dir = std::env::temp_dir();
        let items = check(&CheckInput { need_bytes: 1_000_000, install_dir: dir, game_root: None });
        assert_eq!(items.iter().map(|i| i.id).collect::<Vec<_>>(), ["windows", "vcredist", "gpu", "disk"]);
        for i in &items {
            assert!(["ok", "missing", "unsupported"].contains(&i.status));
            assert!(!i.found.is_empty(), "{} has no 'found' line", i.id);
            if i.status != "ok" && !i.fixable {
                assert!(i.advice.is_some(), "{} needs advice when red", i.id);
            }
        }
    }

    #[test]
    fn a_file_not_signed_by_microsoft_is_refused() {
        let f = std::env::temp_dir().join("zirtola-unsigned-test.exe");
        std::fs::write(&f, b"MZ not really an installer").unwrap();
        let r = verify_microsoft_signature(&f);
        let _ = std::fs::remove_file(&f);
        assert!(r.is_err());
    }

    #[test]
    fn microsofts_own_signed_binary_passes() {
        // powershell.exe itself is catalog-signed; the in-box vc_redist cache isn't
        // guaranteed, so use a binary every Windows has with an embedded MS signature.
        let sys = std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("C:\\Windows"));
        let candidate = sys.join("System32").join("WindowsPowerShell").join("v1.0").join("powershell.exe");
        if candidate.exists() {
            // Catalog signatures report Valid too; the subject check still applies.
            let r = verify_microsoft_signature(&candidate);
            assert!(r.is_ok(), "{r:?}");
        }
    }
}

#[cfg(test)]
mod print_tests {
    #[test]
    #[ignore]
    fn print_this_pc() {
        for i in super::check(&super::CheckInput { need_bytes: 423_245_046, install_dir: r"C:\Zirtola Games".into(), game_root: Some(r"C:\Zirtola Games\Dirt Digger".into()) }) {
            println!("{:9} {:12} {} | {:?}", i.id, i.status, i.found, i.source);
        }
        let g = super::in_game_installer(Some(std::path::Path::new(r"C:\Zirtola Games\Dirt Digger")));
        println!("in-game installer: {:?}", g);
        if let Some((p, _)) = g {
            println!("signature: {:?}", super::verify_microsoft_signature(&p));
        }
    }
}
