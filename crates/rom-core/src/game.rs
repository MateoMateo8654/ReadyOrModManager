mod vdf;
#[cfg(windows)]
mod windows;

use std::fs;
use std::path::{Path, PathBuf};

pub const APP_ID: u32 = 1144200;

pub fn detect_game() -> Option<PathBuf> {
    find_game_in_steam_roots(&steam_roots())
}

pub fn steam_roots() -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        windows::steam_roots()
    }
    #[cfg(not(windows))]
    {
        dirs::home_dir()
            .map(|home| linux_steam_roots(&home))
            .unwrap_or_default()
    }
}

pub fn linux_steam_roots(home: &Path) -> Vec<PathBuf> {
    [
        ".local/share/Steam",
        ".steam/steam",
        ".steam/root",
        ".var/app/com.valvesoftware.Steam/.local/share/Steam",
        ".var/app/com.valvesoftware.Steam/data/Steam",
    ]
    .iter()
    .map(|relative| home.join(relative))
    .collect()
}

pub fn find_game_in_steam_roots(steam_roots: &[PathBuf]) -> Option<PathBuf> {
    steam_roots
        .iter()
        .flat_map(|root| libraries(root))
        .find_map(|library| game_in_library(&library))
}

fn libraries(steam_root: &Path) -> Vec<PathBuf> {
    let mut libraries = vec![steam_root.to_path_buf()];
    for library in listed_libraries(steam_root).unwrap_or_default() {
        if !libraries.contains(&library) {
            libraries.push(library);
        }
    }
    libraries
}

fn listed_libraries(steam_root: &Path) -> Option<Vec<PathBuf>> {
    let text = fs::read_to_string(steam_root.join("steamapps").join("libraryfolders.vdf")).ok()?;
    let doc = vdf::parse(&text)?;
    let folders = doc.get("libraryfolders")?;
    let paths = folders
        .children()
        .iter()
        .filter(|(key, _)| key.bytes().all(|b| b.is_ascii_digit()))
        .filter_map(|(_, entry)| match entry {
            vdf::Value::Obj(_) => entry.get("path")?.as_str(),
            vdf::Value::Str(path) => Some(path.as_str()),
        })
        .map(PathBuf::from)
        .collect();
    Some(paths)
}

fn game_in_library(library: &Path) -> Option<PathBuf> {
    let steamapps = library.join("steamapps");
    let text = fs::read_to_string(steamapps.join(format!("appmanifest_{APP_ID}.acf"))).ok()?;
    let doc = vdf::parse(&text)?;
    let installdir = doc.get("AppState")?.get("installdir")?.as_str()?;
    Some(steamapps.join("common").join(installdir)).filter(|game| is_game_root(game))
}

pub fn is_game_root(path: &Path) -> bool {
    paks_dir(path).is_dir()
}

pub fn mod_folder(game_root: &Path) -> PathBuf {
    paks_dir(game_root).join("RoM")
}

fn paks_dir(game_root: &Path) -> PathBuf {
    game_root.join("ReadyOrNot").join("Content").join("Paks")
}
