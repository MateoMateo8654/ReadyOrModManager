use std::path::Path;

use rom_core::db::Database;
use rom_core::mods::{self, ModState};
use rom_core::profiles::{self, Profile, ProfileId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModRow {
    pub filename: String,
    pub enabled: bool,
    pub size: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct View {
    pub mods: Vec<ModRow>,
    pub profiles: Vec<Profile>,
    active: ProfileId,
}

impl View {
    pub fn load(db: &Database, mod_folder: &Path) -> rom_core::Result<Self> {
        let mut rows = Vec::new();
        for filename in mods::filenames(db)? {
            let state = mods::state_on_disk(mod_folder, &filename)?;
            let size = state
                .and_then(|s| std::fs::metadata(mods::path_in_state(mod_folder, &filename, s)).ok())
                .map(|m| m.len());
            rows.push(ModRow {
                enabled: state == Some(ModState::Enabled),
                filename,
                size,
            });
        }
        Ok(Self {
            mods: rows,
            profiles: profiles::list(db)?,
            active: profiles::active(db)?,
        })
    }

    pub fn profile_name(&self, id: ProfileId) -> Option<&str> {
        self.profiles
            .iter()
            .find(|p| p.id == id)
            .map(|p| p.name.as_str())
    }

    pub fn active(&self) -> &Profile {
        self.profiles
            .iter()
            .find(|p| p.id == self.active)
            .expect("the Active Profile is listed")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rom_core::db::Database;
    use rom_core::{mods, profiles};

    #[test]
    fn lists_mods_with_their_state_and_size_and_the_active_profile() {
        let mut db = Database::open_in_memory().unwrap();
        profiles::init(&mut db).unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.pak"), b"12345").unwrap();
        std::fs::write(dir.path().join("b.pak"), b"12").unwrap();
        mods::refresh(&mut db, dir.path()).unwrap();
        mods::disable(dir.path(), "b.pak").unwrap();

        let view = View::load(&db, dir.path()).unwrap();

        assert_eq!(
            view.mods,
            vec![
                ModRow {
                    filename: "a.pak".into(),
                    enabled: true,
                    size: Some(5)
                },
                ModRow {
                    filename: "b.pak".into(),
                    enabled: false,
                    size: Some(2)
                },
            ]
        );
        let names: Vec<&str> = view.profiles.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["Vanilla", "Default"]);
        assert_eq!(view.active().name, "Default");
        assert!(!view.active().builtin);
    }
}
