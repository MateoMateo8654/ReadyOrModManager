use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use crate::db::Database;
use crate::profiles::{self, ProfileError, ProfileId};
use crate::{Error, Result, settings};

pub const DISABLED_SUFFIX: &str = ".disabled";

#[derive(Debug, thiserror::Error)]
pub enum ModError {
    #[error("Mod \"{filename}\" is not in the Mod Folder")]
    Missing { filename: String },
    #[error("Mod \"{filename}\" is in use by another program: {source}")]
    Locked {
        filename: String,
        #[source]
        source: std::io::Error,
    },
    #[error("no permission to rename Mod \"{filename}\": {source}")]
    PermissionDenied {
        filename: String,
        #[source]
        source: std::io::Error,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModState {
    Enabled,
    Disabled,
}

impl ModState {
    pub fn from_enabled(enabled: bool) -> Self {
        if enabled {
            Self::Enabled
        } else {
            Self::Disabled
        }
    }

    pub fn is_enabled(self) -> bool {
        self == Self::Enabled
    }

    pub fn toggled(self) -> Self {
        Self::from_enabled(!self.is_enabled())
    }
}

pub fn enabled_path(mod_folder: &Path, filename: &str) -> PathBuf {
    mod_folder.join(filename)
}

pub fn disabled_path(mod_folder: &Path, filename: &str) -> PathBuf {
    mod_folder.join(format!("{filename}{DISABLED_SUFFIX}"))
}

pub fn path_in_state(mod_folder: &Path, filename: &str, state: ModState) -> PathBuf {
    match state {
        ModState::Enabled => enabled_path(mod_folder, filename),
        ModState::Disabled => disabled_path(mod_folder, filename),
    }
}

pub fn state_on_disk(mod_folder: &Path, filename: &str) -> Result<Option<ModState>> {
    if enabled_path(mod_folder, filename).try_exists()? {
        Ok(Some(ModState::Enabled))
    } else if disabled_path(mod_folder, filename).try_exists()? {
        Ok(Some(ModState::Disabled))
    } else {
        Ok(None)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ModId(pub i64);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mod {
    pub filename: String,
    pub enabled: bool,
}

pub fn refresh(db: &mut Database, mod_folder: &Path) -> Result<Vec<Mod>> {
    std::fs::create_dir_all(mod_folder)?;
    let scan = scan(mod_folder)?;

    let tx = db.conn().unchecked_transaction()?;
    let known: HashSet<String> = filenames(db)?.into_iter().collect();

    for filename in &known {
        if !scan.mods.contains_key(filename) && !scan.conflicts.contains(filename) {
            delete(db, filename)?;
        }
    }

    let auto_enable_into = auto_enable_profile(db)?;
    let mut mods = Vec::with_capacity(scan.mods.len());
    for (filename, &on_disk_enabled) in &scan.mods {
        let mut enabled = on_disk_enabled;
        if !known.contains(filename) {
            let mod_id = insert(db, filename)?;
            if let Some(profile_id) = auto_enable_into {
                profiles::add_mod(db, profile_id, mod_id)?;
            }
            enabled = auto_enable_into.is_some();
            set_enabled(mod_folder, filename, enabled)?;
        }
        mods.push(Mod {
            filename: filename.clone(),
            enabled,
        });
    }
    tx.commit()?;

    if scan.conflicts.is_empty() {
        Ok(mods)
    } else {
        Err(Error::ModConflict {
            filenames: scan.conflicts,
        })
    }
}

pub(crate) const STAGING_DIR: &str = ".rom-staging";

pub(crate) fn with_staging<T>(mod_folder: &Path, f: impl FnOnce(&Path) -> Result<T>) -> Result<T> {
    let staging = mod_folder.join(STAGING_DIR);
    let result = f(&staging);
    let _ = std::fs::remove_dir_all(&staging);
    result
}

pub fn all(db: &Database) -> Result<Vec<(ModId, String)>> {
    let mut stmt = db
        .conn()
        .prepare("SELECT id, filename FROM mods ORDER BY filename")?;
    let rows = stmt.query_map([], |r| Ok((ModId(r.get(0)?), r.get(1)?)))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub fn filenames(db: &Database) -> Result<Vec<String>> {
    Ok(all(db)?.into_iter().map(|(_, f)| f).collect())
}

pub fn id_of(db: &Database, filename: &str) -> Result<Option<ModId>> {
    use rusqlite::OptionalExtension;
    Ok(db
        .conn()
        .query_row("SELECT id FROM mods WHERE filename = ?1", [filename], |r| {
            Ok(ModId(r.get(0)?))
        })
        .optional()?)
}

pub(crate) fn insert(db: &Database, filename: &str) -> Result<ModId> {
    db.conn()
        .execute("INSERT INTO mods (filename) VALUES (?1)", [filename])?;
    Ok(ModId(db.conn().last_insert_rowid()))
}

pub(crate) fn ensure(db: &Database, filename: &str) -> Result<ModId> {
    match id_of(db, filename)? {
        Some(id) => Ok(id),
        None => insert(db, filename),
    }
}

pub(crate) fn delete(db: &Database, filename: &str) -> Result<()> {
    db.conn()
        .execute("DELETE FROM mods WHERE filename = ?1", [filename])?;
    Ok(())
}

pub fn enable(mod_folder: &Path, filename: &str) -> Result<()> {
    set_enabled(mod_folder, filename, true)
}

pub fn disable(mod_folder: &Path, filename: &str) -> Result<()> {
    set_enabled(mod_folder, filename, false)
}

pub fn set_enabled(mod_folder: &Path, filename: &str, enabled: bool) -> Result<()> {
    let state = ModState::from_enabled(enabled);
    let src = path_in_state(mod_folder, filename, state.toggled());
    let dst = path_in_state(mod_folder, filename, state);
    if dst.try_exists()? {
        if src.try_exists()? {
            return Err(Error::ModConflict {
                filenames: vec![filename.to_owned()],
            });
        }
        return Ok(());
    }
    std::fs::rename(src, dst).map_err(|e| rename_error(filename, e))
}

pub fn set_enabled_in_active_profile(
    db: &Database,
    mod_folder: &Path,
    filename: &str,
    enabled: bool,
) -> Result<()> {
    let active = profiles::active(db)?;
    let mod_id = id_of(db, filename)?.ok_or_else(|| ModError::Missing {
        filename: filename.to_owned(),
    })?;
    let tx = db.conn().unchecked_transaction()?;
    if enabled {
        profiles::add_mod(db, active, mod_id)?;
    } else {
        profiles::remove_mod(db, active, mod_id)?;
    }
    set_enabled(mod_folder, filename, enabled)?;
    tx.commit()?;
    Ok(())
}

fn rename_error(filename: &str, source: std::io::Error) -> Error {
    use std::io::ErrorKind;
    let filename = filename.to_owned();
    let err = match (source.kind(), source.raw_os_error()) {
        (ErrorKind::NotFound, _) => ModError::Missing { filename },
        (_, Some(32 | 33)) if cfg!(windows) => ModError::Locked { filename, source },
        (ErrorKind::PermissionDenied, _) => ModError::PermissionDenied { filename, source },
        _ => return Error::Io(source),
    };
    Error::Mod(err)
}

struct Scan {
    mods: BTreeMap<String, bool>,
    conflicts: Vec<String>,
}

fn scan(mod_folder: &Path) -> Result<Scan> {
    let mut mods = BTreeMap::new();
    let mut conflicts = Vec::new();
    for entry in std::fs::read_dir(mod_folder)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        let (filename, enabled) = match name.strip_suffix(DISABLED_SUFFIX) {
            Some(base) if !base.is_empty() => (base.to_owned(), false),
            _ => (name, true),
        };
        if mods.insert(filename.clone(), enabled).is_some() {
            conflicts.push(filename);
        }
    }
    for filename in &conflicts {
        mods.remove(filename);
    }
    conflicts.sort();
    Ok(Scan { mods, conflicts })
}

pub(crate) fn auto_enable_profile(db: &Database) -> Result<Option<ProfileId>> {
    if !settings::auto_enable(db)? {
        return Ok(None);
    }
    let active = match profiles::active(db) {
        Ok(id) => id,
        Err(Error::Profile(ProfileError::NoActiveProfile)) => return Ok(None),
        Err(e) => return Err(e),
    };
    let editable = profiles::get(db, active)?.is_some_and(|p| !p.builtin);
    Ok(editable.then_some(active))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn setup() -> (Database, TempDir) {
        let mut db = Database::open_in_memory().unwrap();
        profiles::init(&mut db).unwrap();
        assert_eq!(profiles::active(&db).unwrap(), ProfileId(2));
        (db, tempfile::tempdir().unwrap())
    }

    fn touch(dir: &Path, name: &str) {
        fs::write(dir.join(name), b"").unwrap();
    }

    fn profile_members(db: &Database, profile_id: i64) -> Vec<String> {
        profiles::mod_filenames(db, ProfileId(profile_id)).unwrap()
    }

    fn enabled(name: &str) -> Mod {
        Mod {
            filename: name.into(),
            enabled: true,
        }
    }

    #[test]
    fn new_file_becomes_an_enabled_mod_in_the_active_profile() {
        let (mut db, dir) = setup();
        touch(dir.path(), "a.pak");

        let mods = refresh(&mut db, dir.path()).unwrap();

        assert_eq!(mods, vec![enabled("a.pak")]);
        assert_eq!(profile_members(&db, 2), vec!["a.pak"]);
    }

    fn disabled(name: &str) -> Mod {
        Mod {
            filename: name.into(),
            enabled: false,
        }
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
    fn new_disabled_file_is_auto_enabled_into_the_active_profile() {
        let (mut db, dir) = setup();
        touch(dir.path(), "b.pak.disabled");

        let mods = refresh(&mut db, dir.path()).unwrap();

        assert_eq!(mods, vec![enabled("b.pak")]);
        assert_eq!(files(dir.path()), vec!["b.pak"]);
        assert_eq!(profile_members(&db, 2), vec!["b.pak"]);
    }

    #[test]
    fn with_auto_enable_off_new_files_become_disabled_mods_outside_the_profile() {
        let (mut db, dir) = setup();
        crate::settings::set_auto_enable(&db, false).unwrap();
        touch(dir.path(), "a.pak");
        touch(dir.path(), "b.pak.disabled");

        let mods = refresh(&mut db, dir.path()).unwrap();

        assert_eq!(mods, vec![disabled("a.pak"), disabled("b.pak")]);
        assert_eq!(files(dir.path()), vec!["a.pak.disabled", "b.pak.disabled"]);
        assert!(profile_members(&db, 2).is_empty());
    }

    #[test]
    fn with_vanilla_active_new_files_become_disabled_and_vanilla_stays_empty() {
        let (mut db, dir) = setup();
        profiles::set_active(&db, profiles::vanilla(&db).unwrap()).unwrap();
        touch(dir.path(), "a.pak");

        let mods = refresh(&mut db, dir.path()).unwrap();

        assert_eq!(mods, vec![disabled("a.pak")]);
        assert!(profile_members(&db, 1).is_empty());
    }

    #[test]
    fn vanished_file_removes_the_mod_from_the_database_and_every_profile() {
        let (mut db, dir) = setup();
        touch(dir.path(), "a.pak");
        touch(dir.path(), "b.pak");
        refresh(&mut db, dir.path()).unwrap();
        let other = profiles::create(&db, "Other").unwrap();
        assert_eq!(other, ProfileId(3));
        let ids: Vec<ModId> = all(&db).unwrap().into_iter().map(|(id, _)| id).collect();
        profiles::set_mods(&db, other, &ids).unwrap();
        fs::remove_file(dir.path().join("a.pak")).unwrap();

        let mods = refresh(&mut db, dir.path()).unwrap();

        assert_eq!(mods, vec![enabled("b.pak")]);
        assert_eq!(profile_members(&db, 2), vec!["b.pak"]);
        assert_eq!(profile_members(&db, 3), vec!["b.pak"]);
    }

    #[test]
    fn known_mod_keeps_its_disk_state_and_memberships() {
        let (mut db, dir) = setup();
        touch(dir.path(), "a.pak");
        refresh(&mut db, dir.path()).unwrap();
        fs::rename(dir.path().join("a.pak"), dir.path().join("a.pak.disabled")).unwrap();

        let mods = refresh(&mut db, dir.path()).unwrap();

        assert_eq!(mods, vec![disabled("a.pak")]);
        assert_eq!(files(dir.path()), vec!["a.pak.disabled"]);
        assert_eq!(profile_members(&db, 2), vec!["a.pak"]);
    }

    #[test]
    fn both_enabled_and_disabled_file_is_reported_as_an_error_and_left_alone() {
        let (mut db, dir) = setup();
        touch(dir.path(), "dup.pak");
        touch(dir.path(), "dup.pak.disabled");
        touch(dir.path(), "ok.pak");

        let err = refresh(&mut db, dir.path()).unwrap_err();

        match err {
            Error::ModConflict { filenames } => assert_eq!(filenames, vec!["dup.pak"]),
            other => panic!("expected ModConflict, got {other:?}"),
        }
        assert_eq!(
            files(dir.path()),
            vec!["dup.pak", "dup.pak.disabled", "ok.pak"]
        );
        assert_eq!(profile_members(&db, 2), vec!["ok.pak"]);
    }

    #[test]
    fn conflict_on_a_known_mod_keeps_it_in_the_database() {
        let (mut db, dir) = setup();
        touch(dir.path(), "dup.pak");
        refresh(&mut db, dir.path()).unwrap();
        touch(dir.path(), "dup.pak.disabled");

        assert!(refresh(&mut db, dir.path()).is_err());

        assert_eq!(profile_members(&db, 2), vec!["dup.pak"]);
    }

    #[test]
    fn disable_renames_an_enabled_mod_to_its_disabled_file() {
        let dir = tempfile::tempdir().unwrap();
        touch(dir.path(), "a.pak");

        disable(dir.path(), "a.pak").unwrap();

        assert_eq!(files(dir.path()), vec!["a.pak.disabled"]);
    }

    #[test]
    fn enable_renames_a_disabled_mod_to_its_plain_filename() {
        let dir = tempfile::tempdir().unwrap();
        touch(dir.path(), "a.pak.disabled");

        enable(dir.path(), "a.pak").unwrap();

        assert_eq!(files(dir.path()), vec!["a.pak"]);
    }

    #[test]
    fn enabling_an_enabled_mod_is_a_no_op() {
        let dir = tempfile::tempdir().unwrap();
        touch(dir.path(), "a.pak");

        enable(dir.path(), "a.pak").unwrap();

        assert_eq!(files(dir.path()), vec!["a.pak"]);
    }

    #[test]
    fn disabling_a_disabled_mod_is_a_no_op() {
        let dir = tempfile::tempdir().unwrap();
        touch(dir.path(), "a.pak.disabled");

        disable(dir.path(), "a.pak").unwrap();

        assert_eq!(files(dir.path()), vec!["a.pak.disabled"]);
    }

    #[test]
    fn toggling_a_mod_present_both_enabled_and_disabled_is_refused_and_loses_no_file() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.pak"), b"enabled").unwrap();
        fs::write(dir.path().join("a.pak.disabled"), b"disabled").unwrap();

        for result in [disable(dir.path(), "a.pak"), enable(dir.path(), "a.pak")] {
            match result.unwrap_err() {
                Error::ModConflict { filenames } => assert_eq!(filenames, vec!["a.pak"]),
                other => panic!("expected ModConflict, got {other:?}"),
            }
        }
        assert_eq!(fs::read(dir.path().join("a.pak")).unwrap(), b"enabled");
        assert_eq!(
            fs::read(dir.path().join("a.pak.disabled")).unwrap(),
            b"disabled"
        );
    }

    #[test]
    fn toggling_a_mod_with_no_file_is_a_missing_error() {
        let dir = tempfile::tempdir().unwrap();

        for result in [enable(dir.path(), "a.pak"), disable(dir.path(), "a.pak")] {
            match result.unwrap_err() {
                Error::Mod(ModError::Missing { filename }) => assert_eq!(filename, "a.pak"),
                other => panic!("expected Missing, got {other:?}"),
            }
        }
    }

    #[test]
    fn refresh_ignores_the_staging_folder() {
        let (mut db, dir) = setup();
        let staging = dir.path().join(STAGING_DIR);
        fs::create_dir(&staging).unwrap();
        touch(&staging, "a.pak.part");

        let mods = refresh(&mut db, dir.path()).unwrap();

        assert!(mods.is_empty());
        assert!(staging.join("a.pak.part").exists());
    }

    #[test]
    fn the_staging_folder_is_removed_after_success_and_after_failure() {
        let dir = tempfile::tempdir().unwrap();
        let staging = dir.path().join(STAGING_DIR);
        let fill = |s: &Path| -> Result<()> {
            fs::create_dir_all(s)?;
            touch(s, "a.pak.part");
            Ok(())
        };

        with_staging(dir.path(), fill).unwrap();
        assert!(!staging.exists());

        let err = with_staging(dir.path(), |s| -> Result<()> {
            fill(s)?;
            Err(ModError::Missing {
                filename: "a.pak".into(),
            }
            .into())
        });
        assert!(err.is_err());
        assert!(!staging.exists());
    }

    #[test]
    fn missing_mod_folder_is_created() {
        let (mut db, dir) = setup();
        let folder = dir.path().join("Paks").join("RoM");

        let mods = refresh(&mut db, &folder).unwrap();

        assert!(mods.is_empty());
        assert!(folder.is_dir());
    }

    #[test]
    fn enabling_in_the_active_profile_renames_the_file_and_adds_the_mod() {
        let (mut db, dir) = setup();
        settings::set_auto_enable(&db, false).unwrap();
        touch(dir.path(), "a.pak.disabled");
        refresh(&mut db, dir.path()).unwrap();

        set_enabled_in_active_profile(&db, dir.path(), "a.pak", true).unwrap();

        assert_eq!(files(dir.path()), vec!["a.pak"]);
        assert_eq!(profile_members(&db, 2), vec!["a.pak"]);
    }

    #[test]
    fn toggling_while_vanilla_is_active_is_refused_and_renames_nothing() {
        let (mut db, dir) = setup();
        settings::set_auto_enable(&db, false).unwrap();
        touch(dir.path(), "a.pak.disabled");
        refresh(&mut db, dir.path()).unwrap();
        profiles::set_active(&db, profiles::vanilla(&db).unwrap()).unwrap();

        let err = set_enabled_in_active_profile(&db, dir.path(), "a.pak", true).unwrap_err();

        assert!(matches!(
            err,
            Error::Profile(ProfileError::VanillaImmutable)
        ));
        assert_eq!(files(dir.path()), vec!["a.pak.disabled"]);
    }

    #[test]
    fn toggling_an_unknown_mod_reports_it_missing() {
        let (db, dir) = setup();

        let err = set_enabled_in_active_profile(&db, dir.path(), "nope.pak", true).unwrap_err();

        assert!(matches!(err, Error::Mod(ModError::Missing { .. })));
    }

    #[test]
    fn a_failed_rename_leaves_the_active_profile_unchanged() {
        let (mut db, dir) = setup();
        settings::set_auto_enable(&db, false).unwrap();
        touch(dir.path(), "a.pak.disabled");
        refresh(&mut db, dir.path()).unwrap();
        fs::remove_file(dir.path().join("a.pak.disabled")).unwrap();

        let err = set_enabled_in_active_profile(&db, dir.path(), "a.pak", true).unwrap_err();

        assert!(matches!(err, Error::Mod(ModError::Missing { .. })));
        assert!(profile_members(&db, 2).is_empty());
    }
}
