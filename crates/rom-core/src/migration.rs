use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::Result;
use crate::apply::{self, ApplyOutcome};
use crate::db::{Database, setting};
use crate::import::{self, IncomingProfile, Merged};
use crate::mods::{self, ModState};
use crate::profiles::{self, ProfileId};

const DONE: &str = "done";
const DECLINED: &str = "declined";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct V1Profile {
    pub name: String,
    pub mods: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct V1Install {
    pub dir: PathBuf,
    pub mods: Vec<String>,
    pub profiles: Vec<V1Profile>,
    pub active: Option<usize>,
}

pub fn v1_dir(home: &Path) -> PathBuf {
    home.join("Documents").join("RoM")
}

pub fn detect(home: &Path) -> Result<Option<V1Install>> {
    let dir = v1_dir(home);
    let bytes = match std::fs::read(dir.join("Data").join("profiles.json")) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let Ok(Value::Array(entries)) = serde_json::from_slice::<Value>(&bytes) else {
        return Ok(None);
    };
    let mut profiles = Vec::new();
    let mut active = None;
    for entry in &entries {
        if let Some(p) = parse_profile(entry) {
            if active.is_none() && entry.get("active") == Some(&Value::Bool(true)) {
                active = Some(profiles.len());
            }
            profiles.push(p);
        }
    }
    if active.is_none() && !profiles.is_empty() {
        active = Some(0);
    }
    let mods = v1_mods(&dir.join("Mods"))?;
    Ok(Some(V1Install {
        dir,
        mods,
        profiles,
        active,
    }))
}

#[derive(Debug)]
#[must_use = "the Active Profile's apply may have failed and must be retried or cancelled"]
pub struct Migrated {
    pub profiles: Vec<ProfileId>,
    pub left_out: Vec<(String, String)>,
    pub apply: ApplyOutcome,
}

pub fn migrate(db: &mut Database, mod_folder: &Path, v1: &V1Install) -> Result<Migrated> {
    std::fs::create_dir_all(mod_folder)?;
    mods::with_staging(mod_folder, |staging| {
        move_mods(&v1.dir.join("Mods"), mod_folder, staging, &v1.mods)
    })?;

    let tx = db.conn().unchecked_transaction()?;
    for filename in &v1.mods {
        mods::ensure(db, filename)?;
    }
    let Merged { ids, left_out } = import::merge_profiles(
        db,
        mod_folder,
        v1.profiles.iter().map(|p| IncomingProfile {
            name: &p.name,
            mods: &p.mods,
            replace: true,
        }),
    )?;
    db.set_setting(setting::V1_MIGRATION, DONE)?;
    tx.commit()?;

    let target = match v1.active.and_then(|i| ids.get(i)) {
        Some(&id) => id,
        None => profiles::active(db)?,
    };
    let apply = apply::apply(db, mod_folder, target)?;
    Ok(Migrated {
        profiles: ids,
        left_out,
        apply,
    })
}

fn move_mods(from: &Path, mod_folder: &Path, staging: &Path, mods: &[String]) -> Result<()> {
    for filename in mods {
        let state = mods::state_on_disk(mod_folder, filename)?.unwrap_or(ModState::Disabled);
        let dst = mods::path_in_state(mod_folder, filename, state);
        move_file(&from.join(filename), &dst, staging, |a, b| {
            std::fs::rename(a, b)
        })?;
    }
    Ok(())
}

fn move_file(
    src: &Path,
    dst: &Path,
    staging: &Path,
    rename: impl Fn(&Path, &Path) -> std::io::Result<()>,
) -> std::io::Result<()> {
    match rename(src, dst) {
        Err(e) if e.kind() == std::io::ErrorKind::CrossesDevices => {
            std::fs::create_dir_all(staging)?;
            let mut part = src.file_name().unwrap_or_default().to_owned();
            part.push(".part");
            let part = staging.join(part);
            let copied = std::fs::copy(src, &part).and_then(|n| {
                if n == std::fs::metadata(src)?.len() {
                    std::fs::rename(&part, dst)
                } else {
                    Err(std::io::Error::other("incomplete copy"))
                }
            });
            if copied.is_err() {
                let _ = std::fs::remove_file(&part);
            }
            copied?;
            std::fs::remove_file(src)
        }
        other => other,
    }
}

pub fn should_offer(db: &Database, home: &Path) -> Result<Option<V1Install>> {
    if cfg!(windows) {
        offer(db, home)
    } else {
        Ok(None)
    }
}

fn offer(db: &Database, home: &Path) -> Result<Option<V1Install>> {
    if db.setting(setting::V1_MIGRATION)?.is_some() {
        return Ok(None);
    }
    detect(home)
}

pub fn decline(db: &Database) -> Result<()> {
    db.set_setting(setting::V1_MIGRATION, DECLINED)
}

pub fn delete_v1(home: &Path) -> Result<()> {
    match std::fs::remove_dir_all(v1_dir(home)) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
        _ => Ok(()),
    }
}

fn parse_profile(v: &Value) -> Option<V1Profile> {
    let name = v.get("name")?.as_str()?.trim();
    if name.is_empty() {
        return None;
    }
    let mut mods: Vec<String> = Vec::new();
    for m in v.get("mods")?.as_array()? {
        let m = m.as_str()?;
        if !mods.iter().any(|x| x == m) {
            mods.push(m.to_owned());
        }
    }
    Some(V1Profile {
        name: name.to_owned(),
        mods,
    })
}

fn v1_mods(dir: &Path) -> Result<Vec<String>> {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };
    let mut mods = Vec::new();
    for entry in entries {
        let entry = entry?;
        if entry.file_type()?.is_file()
            && let Ok(name) = entry.file_name().into_string()
        {
            mods.push(name);
        }
    }
    mods.sort();
    Ok(mods)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::fs;

    #[test]
    fn no_v1_folder_means_nothing_to_migrate() {
        let home = tempfile::tempdir().unwrap();
        assert_eq!(detect(home.path()).unwrap(), None);
    }

    fn v1(home: &Path, profiles_json: &str, mods: &[(&str, &[u8])]) {
        let rom = home.join("Documents").join("RoM");
        fs::create_dir_all(rom.join("Data")).unwrap();
        fs::create_dir_all(rom.join("Mods")).unwrap();
        fs::write(rom.join("Data").join("profiles.json"), profiles_json).unwrap();
        for (name, bytes) in mods {
            fs::write(rom.join("Mods").join(name), bytes).unwrap();
        }
    }

    fn profile(name: &str, mods: &[&str]) -> V1Profile {
        V1Profile {
            name: name.to_owned(),
            mods: mods.iter().map(|m| m.to_string()).collect(),
        }
    }

    #[test]
    fn detect_reads_v1_profiles_and_mods_ignoring_unknown_fields() {
        let home = tempfile::tempdir().unwrap();
        v1(
            home.path(),
            r#"[{"name": "Co-op", "mods": ["a.pak", "b.pak"], "created": 123},
                {"name": "PvP", "mods": []}]"#,
            &[("b.pak", b"B"), ("a.pak", b"A")],
        );

        let found = detect(home.path()).unwrap().unwrap();

        assert_eq!(found.dir, home.path().join("Documents").join("RoM"));
        assert_eq!(found.mods, ["a.pak", "b.pak"]);
        assert_eq!(
            found.profiles,
            [profile("Co-op", &["a.pak", "b.pak"]), profile("PvP", &[])]
        );
        assert_eq!(found.active, Some(0));
    }

    #[test]
    fn an_active_flag_picks_the_active_profile() {
        let home = tempfile::tempdir().unwrap();
        v1(
            home.path(),
            r#"[{"name": "A", "mods": []}, {"name": "B", "mods": [], "active": true}]"#,
            &[],
        );

        assert_eq!(detect(home.path()).unwrap().unwrap().active, Some(1));
    }

    #[test]
    fn a_profiles_json_that_is_not_v1s_shape_is_not_a_v1_install() {
        let home = tempfile::tempdir().unwrap();
        v1(
            home.path(),
            r#"{"name": "A", "mods": []}"#,
            &[("a.pak", b"A")],
        );

        assert_eq!(detect(home.path()).unwrap(), None);
    }

    struct Setup {
        _dir: tempfile::TempDir,
        db: Database,
        home: PathBuf,
        mod_folder: PathBuf,
    }

    impl Setup {
        fn new() -> Self {
            let mut db = Database::open_in_memory().unwrap();
            profiles::init(&mut db).unwrap();
            let dir = tempfile::tempdir().unwrap();
            let home = dir.path().join("home");
            let mod_folder = dir.path().join("game").join("RoM");
            fs::create_dir_all(&home).unwrap();
            fs::create_dir_all(&mod_folder).unwrap();
            Self {
                _dir: dir,
                db,
                home,
                mod_folder,
            }
        }

        fn migrate(&mut self) -> Result<Migrated> {
            let found = detect(&self.home).unwrap().expect("a v1 install");
            migrate(&mut self.db, &self.mod_folder, &found)
        }

        fn v1_mods_dir(&self) -> PathBuf {
            v1_dir(&self.home).join("Mods")
        }
    }

    fn folder(dir: &Path) -> BTreeMap<String, Vec<u8>> {
        fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap())
            .filter(|e| e.file_type().unwrap().is_file())
            .map(|e| {
                (
                    e.file_name().into_string().unwrap(),
                    fs::read(e.path()).unwrap(),
                )
            })
            .collect()
    }

    fn files(entries: &[(&str, &[u8])]) -> BTreeMap<String, Vec<u8>> {
        entries
            .iter()
            .map(|(n, b)| (n.to_string(), b.to_vec()))
            .collect()
    }

    fn memberships(db: &Database) -> BTreeMap<String, Vec<String>> {
        profiles::list(db)
            .unwrap()
            .into_iter()
            .filter(|p| !p.builtin)
            .map(|p| (p.name, profiles::mod_filenames(db, p.id).unwrap()))
            .collect()
    }

    fn active_name(db: &Database) -> String {
        let active = profiles::active(db).unwrap();
        profiles::get(db, active).unwrap().unwrap().name
    }

    const PROFILES: &str = r#"[
        {"name": "Co-op", "mods": ["a.pak", "b.pak"]},
        {"name": "PvP", "mods": ["c.pak", "gone.pak"], "active": true}
    ]"#;

    #[test]
    fn migrate_moves_mods_converts_profiles_and_applies_the_active_one() {
        let mut s = Setup::new();
        v1(
            &s.home,
            PROFILES,
            &[("a.pak", b"A"), ("b.pak", b"B"), ("c.pak", b"C")],
        );
        fs::write(s.mod_folder.join("a.pak"), b"old copy").unwrap();

        let migrated = s.migrate().unwrap();

        assert!(matches!(migrated.apply, ApplyOutcome::Done));
        assert_eq!(
            folder(&s.mod_folder),
            files(&[
                ("a.pak.disabled", b"A"),
                ("b.pak.disabled", b"B"),
                ("c.pak", b"C"),
            ])
        );
        assert!(folder(&s.v1_mods_dir()).is_empty());
        assert_eq!(
            memberships(&s.db),
            BTreeMap::from([
                ("Default".to_owned(), vec![]),
                (
                    "Co-op".to_owned(),
                    vec!["a.pak".to_owned(), "b.pak".to_owned()]
                ),
                ("PvP".to_owned(), vec!["c.pak".to_owned()]),
            ])
        );
        assert_eq!(active_name(&s.db), "PvP");
        assert_eq!(
            migrated.left_out,
            [("PvP".to_owned(), "gone.pak".to_owned())]
        );
    }

    #[test]
    fn an_interrupted_migration_loses_no_mod_and_can_be_finished() {
        let mut s = Setup::new();
        v1(
            &s.home,
            PROFILES,
            &[("a.pak", b"A"), ("b.pak", b"B"), ("c.pak", b"C")],
        );
        let blocker = s.mod_folder.join("b.pak.disabled");
        fs::create_dir_all(blocker.join("x")).unwrap();

        assert!(s.migrate().is_err());

        let mut all = folder(&s.mod_folder);
        all.extend(folder(&s.v1_mods_dir()));
        assert_eq!(
            all,
            files(&[("a.pak.disabled", b"A"), ("b.pak", b"B"), ("c.pak", b"C")]),
            "every Mod is in one place or the other"
        );
        assert!(folder(&s.v1_mods_dir()).contains_key("b.pak"));

        fs::remove_dir_all(&blocker).unwrap();
        let migrated = s.migrate().unwrap();

        assert!(matches!(migrated.apply, ApplyOutcome::Done));
        assert!(folder(&s.v1_mods_dir()).is_empty());
        assert_eq!(
            memberships(&s.db)["Co-op"],
            ["a.pak".to_owned(), "b.pak".to_owned()]
        );
    }

    #[test]
    fn a_cross_drive_move_copies_then_deletes_the_original() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("a.pak");
        let dst = dir.path().join("RoM").join("a.pak");
        let staging = dir.path().join("RoM").join(mods::STAGING_DIR);
        fs::create_dir_all(dst.parent().unwrap()).unwrap();
        fs::write(&src, b"new").unwrap();
        fs::write(&dst, b"old").unwrap();
        let other_drive = |_: &Path, _: &Path| -> std::io::Result<()> {
            Err(std::io::ErrorKind::CrossesDevices.into())
        };

        move_file(&src, &dst, &staging, other_drive).unwrap();

        assert_eq!(fs::read(&dst).unwrap(), b"new");
        assert!(!src.exists());
        assert!(folder(&staging).is_empty());
    }

    #[test]
    fn a_failed_cross_drive_copy_keeps_the_original() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("a.pak");
        fs::write(&src, b"new").unwrap();
        let dst = dir.path().join("RoM").join("a.pak");
        fs::create_dir_all(dst.join("x")).unwrap();
        let other_drive = |_: &Path, _: &Path| -> std::io::Result<()> {
            Err(std::io::ErrorKind::CrossesDevices.into())
        };

        let staging = dir.path().join("RoM").join(mods::STAGING_DIR);
        assert!(move_file(&src, &dst, &staging, other_drive).is_err());

        assert_eq!(fs::read(&src).unwrap(), b"new");
        assert!(folder(&staging).is_empty());
    }

    #[test]
    fn v1_profiles_replace_same_named_ones_but_never_vanilla() {
        let mut s = Setup::new();
        v1(
            &s.home,
            r#"[{"name": "default", "mods": ["a.pak"]},
                {"name": "vanilla", "mods": ["a.pak"]},
                {"name": "Default", "mods": []}]"#,
            &[("a.pak", b"A")],
        );

        let migrated = s.migrate().unwrap();

        assert!(matches!(migrated.apply, ApplyOutcome::Done));
        assert_eq!(
            memberships(&s.db),
            BTreeMap::from([
                ("Default".to_owned(), vec!["a.pak".to_owned()]),
                ("vanilla (2)".to_owned(), vec!["a.pak".to_owned()]),
                ("Default (2)".to_owned(), vec![]),
            ])
        );
        assert_eq!(active_name(&s.db), "Default");
        assert_eq!(folder(&s.mod_folder), files(&[("a.pak", b"A")]));
    }

    #[test]
    fn a_failed_apply_is_handed_to_the_caller() {
        let mut s = Setup::new();
        v1(&s.home, PROFILES, &[("c.pak", b"C")]);
        fs::write(s.mod_folder.join("z.pak"), b"Z").unwrap();
        crate::mods::refresh(&mut s.db, &s.mod_folder).unwrap();
        fs::create_dir_all(s.mod_folder.join("z.pak.disabled").join("x")).unwrap();

        let migrated = s.migrate().unwrap();

        let ApplyOutcome::Failed(in_progress) = migrated.apply else {
            panic!("expected a failed apply");
        };
        assert_eq!(in_progress.failure().filename(), Some("z.pak"));
        assert_eq!(active_name(&s.db), "Default");
        assert!(matches!(in_progress.cancel(), ApplyOutcome::Done));
        assert_eq!(fs::read(s.mod_folder.join("c.pak.disabled")).unwrap(), b"C");
    }

    const MODS: &[(&str, &[u8])] = &[("a.pak", b"A"), ("c.pak", b"C")];

    #[test]
    fn a_detected_v1_install_is_offered_until_declined() {
        let s = Setup::new();
        v1(&s.home, PROFILES, MODS);
        let v1_folder = || folder(&s.v1_mods_dir());
        let before = v1_folder();

        assert!(offer(&s.db, &s.home).unwrap().is_some());
        decline(&s.db).unwrap();

        assert_eq!(offer(&s.db, &s.home).unwrap(), None);
        assert_eq!(v1_folder(), before, "declining leaves v1 untouched");
        assert!(v1_dir(&s.home).join("Data").join("profiles.json").exists());
        assert!(folder(&s.mod_folder).is_empty());
    }

    #[test]
    fn a_migrated_v1_install_is_not_offered_again() {
        let mut s = Setup::new();
        v1(&s.home, PROFILES, MODS);

        let _ = s.migrate().unwrap();

        assert_eq!(offer(&s.db, &s.home).unwrap(), None);
    }

    #[test]
    fn the_automatic_offer_is_windows_only() {
        let s = Setup::new();
        v1(&s.home, PROFILES, MODS);

        let offered = should_offer(&s.db, &s.home).unwrap();

        assert_eq!(offered.is_some(), cfg!(windows));
    }

    #[test]
    fn delete_v1_removes_only_v1s_folder() {
        let s = Setup::new();
        v1(&s.home, PROFILES, MODS);
        fs::write(s.home.join("Documents").join("keep.txt"), b"k").unwrap();

        delete_v1(&s.home).unwrap();

        assert!(!v1_dir(&s.home).exists());
        assert!(s.home.join("Documents").join("keep.txt").exists());
        assert_eq!(detect(&s.home).unwrap(), None);
    }
}
