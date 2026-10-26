//! System information the launcher reports when "Share hardware info" is on.
//!
//! Exactly these fields and nothing else: OS name/version/build, CPU model and
//! core/thread counts, RAM, GPU models with VRAM and driver version, display
//! resolutions, free space on the install drive and the launcher version.
//! No serial numbers, MAC addresses, user or computer names, or file paths.

use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Os {
    pub name: String,
    pub version: String,
    pub build: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Cpu {
    pub model: String,
    pub cores: u32,
    pub threads: u32,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Gpu {
    pub name: String,
    pub vram_mb: u64,
    pub driver: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Display {
    pub width: u32,
    pub height: u32,
    pub scale: f64,
    pub refresh_hz: u32,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct HardwareReport {
    pub os: Os,
    pub cpu: Cpu,
    pub ram_mb: u64,
    pub gpus: Vec<Gpu>,
    pub displays: Vec<Display>,
    pub install_drive_free_mb: Option<u64>,
    pub launcher_version: String,
}

impl HardwareReport {
    /// Short, non-identifying label for the device list ("Windows 11 Pro · NVIDIA GeForce RTX 3070").
    pub fn label(&self) -> String {
        let gpu = self
            .gpus
            .iter()
            .max_by_key(|g| g.vram_mb)
            .map(|g| g.name.clone())
            .unwrap_or_default();
        [self.os.name.clone(), gpu]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" · ")
    }

    /// Stable hash of everything except free disk space (which changes constantly),
    /// used to decide whether the report changed.
    pub fn fingerprint(&self) -> String {
        use sha2::{Digest, Sha256};
        let mut copy = self.clone();
        copy.install_drive_free_mb = copy.install_drive_free_mb.map(|mb| mb / (50 * 1024));
        let bytes = serde_json::to_vec(&copy).unwrap_or_default();
        format!("{:x}", Sha256::digest(&bytes))
    }
}

fn reg_str(key: &winreg::RegKey, name: &str) -> String {
    key.get_value::<String, _>(name)
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn os_info() -> Os {
    use winreg::enums::HKEY_LOCAL_MACHINE;
    let hklm = winreg::RegKey::predef(HKEY_LOCAL_MACHINE);
    let Ok(k) = hklm.open_subkey("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion") else {
        return Os {
            name: "Windows".into(),
            version: String::new(),
            build: String::new(),
        };
    };
    let mut name = reg_str(&k, "ProductName");
    let build = reg_str(&k, "CurrentBuildNumber");
    let ubr: u32 = k.get_value("UBR").unwrap_or(0);
    // Windows 11 still reports "Windows 10" in ProductName; build 22000+ is 11.
    if build.parse::<u32>().unwrap_or(0) >= 22000 {
        name = name.replace("Windows 10", "Windows 11");
    }
    Os {
        name,
        version: reg_str(&k, "DisplayVersion"),
        build: if ubr > 0 {
            format!("{build}.{ubr}")
        } else {
            build
        },
    }
}

fn cpu_info() -> Cpu {
    use winreg::enums::HKEY_LOCAL_MACHINE;
    let model = winreg::RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey("HARDWARE\\DESCRIPTION\\System\\CentralProcessor\\0")
        .map(|k| reg_str(&k, "ProcessorNameString"))
        .unwrap_or_default();
    let threads = std::thread::available_parallelism()
        .map(|n| n.get() as u32)
        .unwrap_or(0);
    Cpu {
        model: model.split_whitespace().collect::<Vec<_>>().join(" "),
        cores: physical_cores().unwrap_or(threads),
        threads,
    }
}

fn physical_cores() -> Option<u32> {
    use windows::Win32::System::SystemInformation::{
        GetLogicalProcessorInformation, RelationProcessorCore, SYSTEM_LOGICAL_PROCESSOR_INFORMATION,
    };
    let mut len = 0u32;
    unsafe {
        let _ = GetLogicalProcessorInformation(None, &mut len);
    }
    let size = std::mem::size_of::<SYSTEM_LOGICAL_PROCESSOR_INFORMATION>() as u32;
    if len == 0 || size == 0 {
        return None;
    }
    let mut buf = vec![SYSTEM_LOGICAL_PROCESSOR_INFORMATION::default(); (len / size) as usize + 1];
    unsafe { GetLogicalProcessorInformation(Some(buf.as_mut_ptr()), &mut len).ok()? };
    let n = buf
        .iter()
        .take((len / size) as usize)
        .filter(|i| i.Relationship == RelationProcessorCore)
        .count() as u32;
    (n > 0).then_some(n)
}

fn ram_mb() -> u64 {
    use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    let mut m = MEMORYSTATUSEX {
        dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    unsafe {
        if GlobalMemoryStatusEx(&mut m).is_ok() {
            return m.ullTotalPhys / (1024 * 1024);
        }
    }
    0
}

fn gpus() -> Vec<Gpu> {
    use windows::core::Interface;
    use windows::Win32::Graphics::Dxgi::{
        CreateDXGIFactory1, IDXGIDevice, IDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE,
    };
    let mut out: Vec<Gpu> = Vec::new();
    let Ok(factory) = (unsafe { CreateDXGIFactory1::<IDXGIFactory1>() }) else {
        return out;
    };
    let mut i = 0u32;
    while let Ok(adapter) = unsafe { factory.EnumAdapters1(i) } {
        i += 1;
        let Ok(desc) = (unsafe { adapter.GetDesc1() }) else {
            continue;
        };
        if desc.Flags & (DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32) != 0 {
            continue; // Microsoft Basic Render Driver
        }
        let end = desc
            .Description
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(desc.Description.len());
        let name = String::from_utf16_lossy(&desc.Description[..end])
            .trim()
            .to_string();
        let driver = unsafe { adapter.CheckInterfaceSupport(&IDXGIDevice::IID) }
            .map(|v| {
                let v = v as u64;
                format!(
                    "{}.{}.{}.{}",
                    (v >> 48) & 0xffff,
                    (v >> 32) & 0xffff,
                    (v >> 16) & 0xffff,
                    v & 0xffff
                )
            })
            .unwrap_or_default();
        let vram_mb = desc.DedicatedVideoMemory as u64 / (1024 * 1024);
        if !name.is_empty() && !out.iter().any(|g| g.name == name && g.vram_mb == vram_mb) {
            out.push(Gpu {
                name,
                vram_mb,
                driver,
            });
        }
    }
    out
}

/// Collect the report. `displays` come from the window system (Tauri monitors).
pub fn collect(
    displays: Vec<Display>,
    install_dir: &std::path::Path,
    launcher_version: &str,
) -> HardwareReport {
    HardwareReport {
        os: os_info(),
        cpu: cpu_info(),
        ram_mb: ram_mb(),
        gpus: gpus(),
        displays,
        install_drive_free_mb: zirtola_core::install::available_space(install_dir)
            .ok()
            .map(|b| b / (1024 * 1024)),
        launcher_version: launcher_version.to_string(),
    }
}

/// SMBIOS system UUID (type 1 table), read with GetSystemFirmwareTable.
fn smbios_uuid() -> Option<[u8; 16]> {
    use windows::Win32::System::SystemInformation::{GetSystemFirmwareTable, FIRMWARE_TABLE_PROVIDER};
    let rsmb = FIRMWARE_TABLE_PROVIDER(u32::from_be_bytes(*b"RSMB"));
    let size = unsafe { GetSystemFirmwareTable(rsmb, 0, None) };
    if size == 0 {
        return None;
    }
    let mut buf = vec![0u8; size as usize];
    let got = unsafe { GetSystemFirmwareTable(rsmb, 0, Some(&mut buf)) };
    if got == 0 || got as usize > buf.len() || buf.len() < 8 {
        return None;
    }
    // RawSMBIOSData header: 4 bytes of versions, u32 length, then the tables.
    let len = u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]) as usize;
    let data = &buf[8..(8 + len).min(buf.len())];
    let mut i = 0usize;
    while i + 4 <= data.len() {
        let ty = data[i];
        let hlen = data[i + 1] as usize;
        if hlen < 4 || i + hlen > data.len() {
            break;
        }
        if ty == 1 && hlen >= 0x19 {
            let mut uuid = [0u8; 16];
            uuid.copy_from_slice(&data[i + 8..i + 24]);
            let blank = uuid.iter().all(|&b| b == 0) || uuid.iter().all(|&b| b == 0xff);
            // A UUID some boards ship with for every unit.
            let generic = uuid == [0x03, 0, 0x02, 0, 0x04, 0, 0x05, 0, 0x00, 0x06, 0x00, 0x07, 0x00, 0x08, 0x00, 0x09];
            return (!blank && !generic).then_some(uuid);
        }
        if ty == 127 {
            break;
        }
        // Skip the formatted area and the string set (ends with two NULs).
        let mut j = i + hlen;
        while j + 1 < data.len() && !(data[j] == 0 && data[j + 1] == 0) {
            j += 1;
        }
        i = j + 2;
    }
    None
}

fn machine_guid() -> Option<String> {
    use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_64KEY};
    winreg::RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey_with_flags("SOFTWARE\\Microsoft\\Cryptography", KEY_READ | KEY_WOW64_64KEY)
        .ok()
        .and_then(|k| k.get_value::<String, _>("MachineGuid").ok())
        .filter(|g| g.len() >= 16)
}

/// Stable, non-reversible device id for licence activations.
pub fn stable_device_id() -> Option<String> {
    use sha2::{Digest, Sha256};
    let basis = if let Some(u) = smbios_uuid() {
        format!("smbios:{}", u.iter().map(|b| format!("{b:02x}")).collect::<String>())
    } else {
        format!("machineguid:{}", machine_guid()?.to_ascii_lowercase())
    };
    let digest = Sha256::digest(format!("zirtola-device-v2|{basis}").as_bytes());
    Some(zirtola_core::b64::encode_url(&digest))
}

#[cfg(test)]
mod tests {
    #[test]
    fn device_id_is_stable_and_opaque() {
        // Same machine: same id every time (reinstalling the launcher reads the
        // same hardware ids), and it is a 32-byte hash, not the raw UUID.
        let a = super::stable_device_id().expect("a hardware id on this PC");
        let b = super::stable_device_id().unwrap();
        assert_eq!(a, b);
        assert_eq!(a.len(), 43);
        assert!(a.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
    }
}
