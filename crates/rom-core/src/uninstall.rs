use std::io;
use std::path::Path;

use crate::Result;
use crate::db::Database;
use crate::mods;

pub fn uninstall(db: &mut Database, mod_folder: &Path, filename: &str) -> Result<()> {
    for path in [
        mods::enabled_path(mod_folder, filename),
        mods::disabled_path(mod_folder, filename),
    ] {
        match std::fs::remove_file(path) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e.into()),
            _ => {}
        }
    }
    mods::delete(db, filename)
}

pub fn uninstall_all(db: &mut Database, mod_folder: &Path) -> Result<()> {
    for filename in mods::filenames(db)? {
        uninstall(db, mod_folder, &filename)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profiles::{self, ProfileId};
    use std::fs;
    use tempfile::TempDir;

    fn setup(names: &[&str]) -> (Database, TempDir) {
        let mut db = Database::open_in_memory().unwrap();
        profiles::init(&mut db).unwrap();
        let other = profiles::create(&db, "Other").unwrap();
        crate::settings::set_auto_enable(&db, false).unwrap();
        let dir = tempfile::tempdir().unwrap();
        for name in names {
            fs::write(dir.path().join(name), b"").unwrap();
        }
        mods::refresh(&mut db, dir.path()).unwrap();
        let ids: Vec<_> = mods::all(&db)
            .unwrap()
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        for profile in [ProfileId(2), other] {
            profiles::set_mods(&db, profile, &ids).unwrap();
        }
        (db, dir)
    }

    fn members(db: &Database, profile_id: i64) -> Vec<String> {
        profiles::mod_filenames(db, ProfileId(profile_id)).unwrap()
    }

    fn files(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn uninstalling_a_disabled_mod_deletes_its_file_and_removes_it_from_every_profile() {
        let (mut db, dir) = setup(&["a.pak", "b.pak"]);

        uninstall(&mut db, dir.path(), "a.pak").unwrap();

        assert_eq!(files(dir.path()), vec!["b.pak.disabled"]);
        assert_eq!(members(&db, 2), vec!["b.pak"]);
        assert_eq!(members(&db, 3), vec!["b.pak"]);
    }

    #[test]
    fn uninstalling_an_enabled_mod_deletes_its_file_and_removes_it_from_every_profile() {
        let (mut db, dir) = setup(&["a.pak", "b.pak"]);
        fs::rename(dir.path().join("a.pak.disabled"), dir.path().join("a.pak")).unwrap();

        uninstall(&mut db, dir.path(), "a.pak").unwrap();

        assert_eq!(files(dir.path()), vec!["b.pak.disabled"]);
        assert_eq!(members(&db, 2), vec!["b.pak"]);
        assert_eq!(members(&db, 3), vec!["b.pak"]);
    }

    #[test]
    fn uninstalling_a_mod_whose_file_is_already_gone_still_removes_it_from_every_profile() {
        let (mut db, dir) = setup(&["a.pak", "b.pak"]);
        fs::remove_file(dir.path().join("a.pak.disabled")).unwrap();

        uninstall(&mut db, dir.path(), "a.pak").unwrap();

        assert_eq!(members(&db, 2), vec!["b.pak"]);
        assert_eq!(members(&db, 3), vec!["b.pak"]);
    }

    #[test]
    fn when_the_file_cannot_be_deleted_the_mod_stays_in_every_profile() {
        let (mut db, dir) = setup(&["a.pak"]);
        fs::create_dir(dir.path().join("a.pak")).unwrap();

        assert!(uninstall(&mut db, dir.path(), "a.pak").is_err());

        assert_eq!(members(&db, 2), vec!["a.pak"]);
        assert_eq!(members(&db, 3), vec!["a.pak"]);
    }

    #[test]
    fn uninstalling_all_deletes_every_mod_file_and_empties_every_profile() {
        let (mut db, dir) = setup(&["a.pak", "b.pak"]);
        mods::enable(dir.path(), "a.pak").unwrap();

        uninstall_all(&mut db, dir.path()).unwrap();

        assert!(files(dir.path()).is_empty());
        assert!(mods::all(&db).unwrap().is_empty());
        assert!(members(&db, 2).is_empty());
        assert!(members(&db, 3).is_empty());
    }
}
