//! Where a game keeps its saves (outside the install folder). The launcher
//! never touches this folder except when the player explicitly asks
//! ("Also delete saves?" when uninstalling, unticked by default).

use std::path::{Component, Path, PathBuf};

/// Expand `%LOCALAPPDATA%`, `%APPDATA%` or `%USERPROFILE%` at the start of a
/// catalogue save location and check it stays inside the player's own
/// profile, at least two folders below the base (e.g. `GranitePit\Saved`), so
/// a bad catalogue entry can never point the delete at a whole profile or drive.
pub fn resolve_save_dir(raw: &str) -> Result<PathBuf, String> {
    resolve_with(raw, |k| std::env::var_os(k).map(PathBuf::from))
}

fn resolve_with(raw: &str, env: impl Fn(&str) -> Option<PathBuf>) -> Result<PathBuf, String> {
    let raw = raw.trim().replace('/', "\\");
    const BASES: [&str; 3] = ["LOCALAPPDATA", "APPDATA", "USERPROFILE"];
    let (var, rest) = BASES
        .iter()
        .find_map(|v| {
            let prefix = format!("%{v}%\\");
            raw.get(..prefix.len())
                .filter(|p| p.eq_ignore_ascii_case(&prefix))
                .map(|_| (*v, &raw[prefix.len()..]))
        })
        .ok_or_else(|| format!("save location must start with %LOCALAPPDATA%, %APPDATA% or %USERPROFILE%: {raw}"))?;
    let base = env(var).ok_or_else(|| format!("%{var}% is not set"))?;
    let rel = Path::new(rest);
    let mut parts = 0;
    for c in rel.components() {
        match c {
            Component::Normal(s) if !s.is_empty() && !s.to_string_lossy().contains('%') => parts += 1,
            _ => return Err(format!("unsafe save location: {raw}")),
        }
    }
    if parts < 2 {
        return Err(format!("save location is too broad: {raw}"));
    }
    Ok(base.join(rel))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(k: &str) -> Option<PathBuf> {
        match k {
            "LOCALAPPDATA" => Some(PathBuf::from(r"C:\Users\p\AppData\Local")),
            "APPDATA" => Some(PathBuf::from(r"C:\Users\p\AppData\Roaming")),
            "USERPROFILE" => Some(PathBuf::from(r"C:\Users\p")),
            _ => None,
        }
    }

    #[test]
    fn save_locations_stay_inside_the_profile() {
        assert_eq!(
            resolve_with(r"%LOCALAPPDATA%\GranitePit\Saved", env).unwrap(),
            PathBuf::from(r"C:\Users\p\AppData\Local\GranitePit\Saved")
        );
        assert_eq!(
            resolve_with("%localappdata%/GranitePit/Saved", env).unwrap(),
            PathBuf::from(r"C:\Users\p\AppData\Local\GranitePit\Saved")
        );
        for bad in [
            r"C:\Windows",
            r"%LOCALAPPDATA%\GranitePit",
            r"%LOCALAPPDATA%\",
            r"%LOCALAPPDATA%\..\..\Saved",
            r"%LOCALAPPDATA%\GranitePit\..\Saved",
            r"%SYSTEMROOT%\System32\x",
            r"%LOCALAPPDATA%\%APPDATA%\x",
            r"\\server\share\a\b",
        ] {
            assert!(resolve_with(bad, env).is_err(), "{bad} should be refused");
        }
    }
}
