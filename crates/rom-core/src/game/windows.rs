use std::path::PathBuf;

use winreg::RegKey;
use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};

pub(super) fn steam_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let candidates = [
        (HKEY_CURRENT_USER, r"Software\Valve\Steam", "SteamPath"),
        (
            HKEY_LOCAL_MACHINE,
            r"SOFTWARE\WOW6432Node\Valve\Steam",
            "InstallPath",
        ),
        (HKEY_LOCAL_MACHINE, r"SOFTWARE\Valve\Steam", "InstallPath"),
    ];
    for (hive, key, value) in candidates {
        if let Ok(path) = RegKey::predef(hive)
            .open_subkey(key)
            .and_then(|k| k.get_value::<String, _>(value))
        {
            let path = PathBuf::from(path.replace('/', "\\"));
            if !roots.contains(&path) {
                roots.push(path);
            }
        }
    }
    let default = PathBuf::from(r"C:\Program Files (x86)\Steam");
    if !roots.contains(&default) {
        roots.push(default);
    }
    roots
}
