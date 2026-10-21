use std::collections::HashSet;
use std::fs::File;
use std::io::Read;
use std::path::Path;

use serde_json::Value;

use crate::Result;
use crate::db::Database;
use crate::install::{self, ArchiveFormat, Candidate, InstallError, InstallPlan, Source};
use crate::mods::{self, Mod};
use crate::profiles::{self, ProfileError, ProfileId};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ImportError {
    #[error("no decision for the Profile name clash on \"{0}\"")]
    UnresolvedClash(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Detected {
    Export(ExportContents),
    Install(install::Inspection),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportContents {
    pub mods: Vec<Candidate>,
    pub profiles: Vec<ImportedProfile>,
    pub rejected: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedProfile {
    pub name: String,
    pub mods: Vec<String>,
    pub missing: Vec<String>,
    pub clash: Option<ProfileId>,
    pub on_clash: Option<ClashResolution>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClashResolution {
    Replace,
    KeepBoth,
}

pub fn detect(db: &Database, path: &Path) -> Result<Detected> {
    let inspection = install::inspect(&[path.to_owned()])?;
    if ArchiveFormat::of(path) != Some(ArchiveFormat::Zip) {
        return Ok(Detected::Install(inspection));
    }
    let mut rejected = inspection.rejected.clone();
    let mut profiles = Vec::new();
    for c in inspection.candidates.iter().filter(|c| is_profile_entry(c)) {
        match read_profile(path, entry_name(c))? {
            Some(p) => profiles.push(p),
            None => rejected.push(entry_name(c).to_owned()),
        }
    }
    if profiles.is_empty() {
        return Ok(Detected::Install(inspection));
    }

    let mods: Vec<Candidate> = inspection
        .candidates
        .into_iter()
        .filter(|c| !is_profile_entry(c))
        .map(|c| Candidate {
            preselected: !c.filename.starts_with("._"),
            ..c
        })
        .collect();
    let present: HashSet<&str> = mods.iter().map(|c| c.filename.as_str()).collect();
    for p in &mut profiles {
        p.missing = p
            .mods
            .iter()
            .filter(|m| !present.contains(m.as_str()))
            .cloned()
            .collect();
        p.clash = profiles::by_name(db, &p.name)?;
    }
    Ok(Detected::Export(ExportContents {
        mods,
        profiles,
        rejected,
    }))
}

fn entry_name(c: &Candidate) -> &str {
    match &c.source {
        Source::Archive { entry, .. } => entry,
        Source::File(_) => "",
    }
}

fn is_profile_entry(c: &Candidate) -> bool {
    let name = entry_name(c).replace('\\', "/");
    let name = name.trim_start_matches("./");
    name == "profile.json"
        || name.strip_prefix("profiles/").is_some_and(|rest| {
            !rest.contains('/')
                && Path::new(rest)
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("json"))
        })
}

const MAX_PROFILE_JSON: u64 = 16 * 1024 * 1024;

fn read_profile(archive: &Path, entry: &str) -> Result<Option<ImportedProfile>> {
    let err = |e: zip::result::ZipError| -> crate::Error {
        InstallError::Archive {
            archive: archive.to_owned(),
            message: e.to_string(),
        }
        .into()
    };
    let mut za = zip::ZipArchive::new(File::open(archive)?).map_err(err)?;
    let mut bytes = Vec::new();
    za.by_name(entry)
        .map_err(err)?
        .take(MAX_PROFILE_JSON + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_PROFILE_JSON {
        return Ok(None);
    }
    Ok(parse_profile(&bytes))
}

fn parse_profile(bytes: &[u8]) -> Option<ImportedProfile> {
    let v: Value = serde_json::from_slice(bytes).ok()?;
    let name = v.get("name")?.as_str()?.trim();
    if name.is_empty() {
        return None;
    }
    let mut seen = HashSet::new();
    let mut mods = Vec::new();
    for m in v.get("mods")?.as_array()? {
        let m = m.as_str()?;
        if seen.insert(m) {
            mods.push(m.to_owned());
        }
    }
    Some(ImportedProfile {
        name: name.to_owned(),
        mods,
        missing: Vec::new(),
        clash: None,
        on_clash: None,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use = "if active_profile_changed, the Active Profile must be re-applied"]
pub struct Imported {
    pub mods: Vec<Mod>,
    pub profiles: Vec<ProfileId>,
    pub left_out: Vec<(String, String)>,

    pub active_profile_changed: bool,
}

pub fn import(
    db: &mut Database,
    mod_folder: &Path,
    mods: InstallPlan,
    profiles: Vec<ImportedProfile>,
) -> Result<Imported> {
    let vanilla = profiles::vanilla(db)?;
    for p in &profiles {
        match (profiles::by_name(db, &p.name)?, p.on_clash) {
            (None, _) | (Some(_), Some(ClashResolution::KeepBoth)) => {}
            (Some(id), Some(ClashResolution::Replace)) if id == vanilla => {
                return Err(ProfileError::VanillaImmutable.into());
            }
            (Some(_), Some(ClashResolution::Replace)) => {}
            (Some(_), None) => return Err(ImportError::UnresolvedClash(p.name.clone()).into()),
        }
    }

    let installed = install::install_into(db, mod_folder, mods, None)?;

    let tx = db.conn().unchecked_transaction()?;
    let merged = merge_profiles(
        db,
        mod_folder,
        profiles.iter().map(|p| IncomingProfile {
            name: &p.name,
            mods: &p.mods,
            replace: p.on_clash == Some(ClashResolution::Replace),
        }),
    )?;
    tx.commit()?;
    let active = profiles::active(db)?;
    Ok(Imported {
        mods: installed,
        active_profile_changed: merged.ids.contains(&active),
        profiles: merged.ids,
        left_out: merged.left_out,
    })
}

#[derive(Debug)]
#[must_use = "a failed apply must be retried or cancelled"]
pub struct ImportedAndApplied {
    pub imported: Imported,

    pub apply: Option<Result<crate::apply::ApplyOutcome>>,
}

pub fn import_and_apply(
    db: &mut Database,
    mod_folder: &Path,
    mods: InstallPlan,
    profiles: Vec<ImportedProfile>,
) -> Result<ImportedAndApplied> {
    let imported = import(db, mod_folder, mods, profiles)?;
    let apply = imported.active_profile_changed.then(|| {
        let active = profiles::active(db)?;
        crate::apply::apply(db, mod_folder, active)
    });
    Ok(ImportedAndApplied { imported, apply })
}

pub(crate) struct IncomingProfile<'a> {
    pub name: &'a str,

    pub mods: &'a [String],
    pub replace: bool,
}

pub(crate) struct Merged {
    pub ids: Vec<ProfileId>,
    pub left_out: Vec<(String, String)>,
}

pub(crate) fn merge_profiles<'a>(
    db: &Database,
    mod_folder: &Path,
    incoming: impl IntoIterator<Item = IncomingProfile<'a>>,
) -> Result<Merged> {
    let vanilla = profiles::vanilla(db)?;
    let mut ids: Vec<ProfileId> = Vec::new();
    let mut left_out = Vec::new();
    for p in incoming {
        let id = match profiles::by_name(db, p.name)? {
            Some(id) if p.replace && id != vanilla && !ids.contains(&id) => id,
            Some(_) => profiles::create(db, &profiles::free_name(db, p.name)?)?,
            None => profiles::create(db, p.name)?,
        };
        let mut members = Vec::with_capacity(p.mods.len());
        for filename in p.mods {
            if install::is_safe_filename(filename)
                && mods::state_on_disk(mod_folder, filename)?.is_some()
            {
                members.push(mods::ensure(db, filename)?);
            } else {
                left_out.push((p.name.to_owned(), filename.clone()));
            }
        }
        profiles::set_mods(db, id, &members)?;
        ids.push(id);
    }
    Ok(Merged { ids, left_out })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use super::*;
    use crate::install::fixtures;
    use crate::profiles;

    fn fresh_db() -> Database {
        let mut db = Database::open_in_memory().unwrap();
        profiles::init(&mut db).unwrap();
        db
    }

    fn export(contents: Detected) -> ExportContents {
        match contents {
            Detected::Export(e) => e,
            Detected::Install(i) => panic!("detected as an Install: {i:?}"),
        }
    }

    fn filenames(mods: &[Candidate]) -> Vec<&str> {
        mods.iter().map(|c| c.filename.as_str()).collect()
    }

    const ALL: &[(&str, &[u8])] = &[
        ("mods/a.pak", b"AAA"),
        ("mods/b.utoc", b"BBB"),
        (
            "profiles/Co-op.json",
            br#"{"name": "Co-op", "mods": ["a.pak", "b.utoc"]}"#,
        ),
        (
            "profiles/PvP_ 1.json",
            br#"{"name": "PvP: 1", "mods": ["b.utoc"]}"#,
        ),
    ];

    const ONE: &[(&str, &[u8])] = &[
        ("a.pak", b"AAA"),
        ("profile.json", br#"{"name": "Solo", "mods": ["a.pak"]}"#),
    ];

    #[test]
    fn a_plain_mod_archive_is_an_install() {
        let dir = tempfile::tempdir().unwrap();
        let zip = fixtures::zip(dir.path(), "mod.zip", fixtures::TREE);

        let detected = detect(&fresh_db(), &zip).unwrap();

        assert!(matches!(detected, Detected::Install(_)), "{detected:?}");
    }

    #[test]
    fn an_archive_with_json_that_is_not_a_v1_profile_is_an_install() {
        let dir = tempfile::tempdir().unwrap();
        let zip = fixtures::zip(
            dir.path(),
            "mod.zip",
            &[
                ("x.pak", b"x"),
                ("profile.json", br#"{"title": "not a profile"}"#),
                ("profiles/config.json", b"[1, 2]"),
            ],
        );

        let detected = detect(&fresh_db(), &zip).unwrap();

        assert!(matches!(detected, Detected::Install(_)), "{detected:?}");
    }

    #[test]
    fn an_export_all_zip_lists_its_mods_and_profiles_by_json_name() {
        let dir = tempfile::tempdir().unwrap();
        let zip = fixtures::zip(dir.path(), "export.zip", ALL);

        let e = export(detect(&fresh_db(), &zip).unwrap());

        assert_eq!(filenames(&e.mods), ["a.pak", "b.utoc"]);
        assert!(e.mods.iter().all(|c| c.preselected));
        let names: Vec<&str> = e.profiles.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["Co-op", "PvP: 1"]);
        assert_eq!(e.profiles[0].mods, ["a.pak", "b.utoc"]);
        assert!(
            e.profiles
                .iter()
                .all(|p| p.missing.is_empty() && p.clash.is_none())
        );
    }

    #[test]
    fn a_single_profile_export_lists_its_flat_mods_and_profile() {
        let dir = tempfile::tempdir().unwrap();
        let zip = fixtures::zip(dir.path(), "export.zip", ONE);

        let e = export(detect(&fresh_db(), &zip).unwrap());

        assert_eq!(filenames(&e.mods), ["a.pak"]);
        assert_eq!(e.profiles.len(), 1);
        assert_eq!(e.profiles[0].name, "Solo");
        assert_eq!(e.profiles[0].mods, ["a.pak"]);
    }

    #[test]
    fn profile_entries_without_a_file_in_the_export_are_reported() {
        let dir = tempfile::tempdir().unwrap();
        let zip = fixtures::zip(
            dir.path(),
            "export.zip",
            &[
                ("mods/a.pak", b"AAA"),
                (
                    "profiles/Default.json",
                    br#"{"name": "Default", "mods": ["a.pak", "gone.pak"]}"#,
                ),
            ],
        );

        let e = export(detect(&fresh_db(), &zip).unwrap());

        assert_eq!(e.profiles[0].missing, ["gone.pak"]);
    }

    #[test]
    fn a_profile_named_like_an_existing_one_is_a_clash() {
        let db = fresh_db();
        let coop = profiles::create(&db, "co-OP").unwrap();
        let dir = tempfile::tempdir().unwrap();
        let zip = fixtures::zip(dir.path(), "export.zip", ALL);

        let e = export(detect(&db, &zip).unwrap());

        assert_eq!(e.profiles[0].clash, Some(coop));
        assert_eq!(e.profiles[1].clash, None);
    }

    fn members(db: &Database, profile: ProfileId) -> Vec<String> {
        profiles::mod_filenames(db, profile).unwrap()
    }

    fn profile_id(db: &Database, name: &str) -> ProfileId {
        profiles::list(db)
            .unwrap()
            .into_iter()
            .find(|p| p.name == name)
            .unwrap_or_else(|| panic!("no Profile {name}"))
            .id
    }

    fn profile_names(db: &Database) -> Vec<String> {
        profiles::list(db)
            .unwrap()
            .into_iter()
            .map(|p| p.name)
            .collect()
    }

    fn folder(mod_folder: &Path) -> BTreeMap<String, Vec<u8>> {
        std::fs::read_dir(mod_folder)
            .unwrap()
            .map(|e| e.unwrap())
            .filter(|e| e.file_type().unwrap().is_file())
            .map(|e| {
                (
                    e.file_name().into_string().unwrap(),
                    std::fs::read(e.path()).unwrap(),
                )
            })
            .collect()
    }

    struct Setup {
        dir: tempfile::TempDir,
        db: Database,
        mod_folder: PathBuf,
    }

    impl Setup {
        fn new() -> Self {
            let db = fresh_db();
            crate::settings::set_auto_enable(&db, false).unwrap();
            let dir = tempfile::tempdir().unwrap();
            let mod_folder = dir.path().join("RoM");
            std::fs::create_dir_all(&mod_folder).unwrap();
            Self {
                dir,
                db,
                mod_folder,
            }
        }

        fn detect(&self, tree: &[(&str, &[u8])]) -> ExportContents {
            let zip = fixtures::zip(self.dir.path(), "export.zip", tree);
            export(detect(&self.db, &zip).unwrap())
        }

        fn import(&mut self, e: ExportContents) -> Result<Imported> {
            let plan = install::plan(&self.mod_folder, e.mods).unwrap();
            assert!(plan.collisions().is_empty());
            import(&mut self.db, &self.mod_folder, plan, e.profiles)
        }
    }

    #[test]
    fn importing_an_export_installs_its_mods_and_creates_its_profiles() {
        let mut s = Setup::new();
        let e = s.detect(ALL);

        let _ = s.import(e).unwrap();

        assert_eq!(
            folder(&s.mod_folder),
            BTreeMap::from([
                ("a.pak.disabled".to_owned(), b"AAA".to_vec()),
                ("b.utoc.disabled".to_owned(), b"BBB".to_vec()),
            ])
        );
        assert_eq!(
            profile_names(&s.db),
            ["Vanilla", "Default", "Co-op", "PvP: 1"]
        );
        assert_eq!(
            members(&s.db, profile_id(&s.db, "Co-op")),
            ["a.pak", "b.utoc"]
        );
        assert_eq!(members(&s.db, profile_id(&s.db, "PvP: 1")), ["b.utoc"]);
    }

    #[test]
    fn imported_mods_arrive_disabled_and_outside_the_active_profile_even_with_auto_enable_on() {
        let mut s = Setup::new();
        crate::settings::set_auto_enable(&s.db, true).unwrap();
        let e = s.detect(ALL);

        let imported = s.import(e).unwrap();

        assert!(imported.mods.iter().all(|m| !m.enabled), "{imported:?}");
        assert_eq!(
            folder(&s.mod_folder).into_keys().collect::<Vec<_>>(),
            ["a.pak.disabled", "b.utoc.disabled"]
        );
        assert!(members(&s.db, profile_id(&s.db, "Default")).is_empty());
    }

    const DEFAULT_EXPORT: &[(&str, &[u8])] = &[
        ("a.pak", b"AAA"),
        ("profile.json", br#"{"name": "default", "mods": ["a.pak"]}"#),
    ];

    #[test]
    fn an_unresolved_profile_clash_is_refused_and_writes_nothing() {
        let mut s = Setup::new();
        let e = s.detect(DEFAULT_EXPORT);

        let err = s.import(e).unwrap_err();

        assert!(
            matches!(&err, crate::Error::Import(ImportError::UnresolvedClash(n)) if n == "default"),
            "{err:?}"
        );
        assert!(folder(&s.mod_folder).is_empty());
        assert_eq!(profile_names(&s.db), ["Vanilla", "Default"]);
    }

    #[test]
    fn replace_overwrites_the_clashing_profiles_mods_and_keeps_it_active() {
        let mut s = Setup::new();
        let default = profile_id(&s.db, "Default");
        std::fs::write(s.mod_folder.join("old.pak"), b"old").unwrap();
        crate::mods::refresh(&mut s.db, &s.mod_folder).unwrap();
        let old = mods::id_of(&s.db, "old.pak").unwrap().unwrap();
        profiles::add_mod(&s.db, default, old).unwrap();
        let mut e = s.detect(DEFAULT_EXPORT);
        e.profiles[0].on_clash = Some(ClashResolution::Replace);

        let imported = s.import(e).unwrap();

        assert_eq!(imported.profiles, [default]);
        assert_eq!(profile_names(&s.db), ["Vanilla", "Default"]);
        assert_eq!(members(&s.db, default), ["a.pak"]);
        assert_eq!(profiles::active(&s.db).unwrap(), default);
        assert!(imported.active_profile_changed);
    }

    #[test]
    fn import_and_apply_renames_files_to_match_a_replaced_active_profile() {
        let mut s = Setup::new();
        let default = profile_id(&s.db, "Default");
        std::fs::write(s.mod_folder.join("old.pak"), b"old").unwrap();
        crate::mods::refresh(&mut s.db, &s.mod_folder).unwrap();
        let old = mods::id_of(&s.db, "old.pak").unwrap().unwrap();
        profiles::add_mod(&s.db, default, old).unwrap();
        crate::mods::enable(&s.mod_folder, "old.pak").unwrap();
        let mut e = s.detect(DEFAULT_EXPORT);
        e.profiles[0].on_clash = Some(ClashResolution::Replace);
        let plan = install::plan(&s.mod_folder, e.mods).unwrap();

        let done = import_and_apply(&mut s.db, &s.mod_folder, plan, e.profiles).unwrap();

        assert!(done.imported.active_profile_changed);
        assert!(matches!(
            done.apply,
            Some(Ok(crate::apply::ApplyOutcome::Done))
        ));
        let files: Vec<String> = folder(&s.mod_folder).into_keys().collect();
        assert_eq!(files, ["a.pak", "old.pak.disabled"]);
    }

    #[test]
    fn keep_both_imports_under_a_numbered_name() {
        let mut s = Setup::new();
        profiles::create(&s.db, "Default (2)").unwrap();
        let mut e = s.detect(DEFAULT_EXPORT);
        e.profiles[0].on_clash = Some(ClashResolution::KeepBoth);

        let imported = s.import(e).unwrap();

        assert!(!imported.active_profile_changed);
        assert_eq!(
            profile_names(&s.db),
            ["Vanilla", "Default", "Default (2)", "default (3)"]
        );
        assert_eq!(members(&s.db, profile_id(&s.db, "default (3)")), ["a.pak"]);
        assert!(members(&s.db, profile_id(&s.db, "Default")).is_empty());
    }

    #[test]
    fn replacing_vanilla_is_refused() {
        let mut s = Setup::new();
        let mut e = s.detect(&[
            ("a.pak", b"AAA"),
            ("profile.json", br#"{"name": "Vanilla", "mods": ["a.pak"]}"#),
        ]);
        e.profiles[0].on_clash = Some(ClashResolution::Replace);

        let err = s.import(e).unwrap_err();

        assert!(
            matches!(err, crate::Error::Profile(ProfileError::VanillaImmutable)),
            "{err:?}"
        );
        assert!(folder(&s.mod_folder).is_empty());
    }

    #[test]
    fn profiles_only_reference_mods_that_ended_up_installed() {
        let mut s = Setup::new();
        let mut e = s.detect(&[
            ("mods/a.pak", b"AAA"),
            ("mods/b.pak", b"BBB"),
            (
                "profiles/P.json",
                br#"{"name": "P", "mods": ["a.pak", "b.pak", "gone.pak"]}"#,
            ),
        ]);
        e.mods.retain(|c| c.filename == "a.pak");

        let imported = s.import(e).unwrap();

        assert_eq!(members(&s.db, profile_id(&s.db, "P")), ["a.pak"]);
        assert_eq!(
            imported.left_out,
            [
                ("P".to_owned(), "b.pak".to_owned()),
                ("P".to_owned(), "gone.pak".to_owned())
            ]
        );
    }

    #[test]
    fn a_profile_entry_matching_an_already_installed_mod_is_kept() {
        let mut s = Setup::new();
        std::fs::write(s.mod_folder.join("a.pak.disabled"), b"mine").unwrap();
        crate::mods::refresh(&mut s.db, &s.mod_folder).unwrap();
        let e = s.detect(&[("profile.json", br#"{"name": "P", "mods": ["a.pak"]}"#)]);

        let imported = s.import(e).unwrap();

        assert_eq!(members(&s.db, profile_id(&s.db, "P")), ["a.pak"]);
        assert!(imported.left_out.is_empty());
    }

    #[test]
    fn two_profiles_of_one_export_with_the_same_name_are_both_kept() {
        let mut s = Setup::new();
        let e = s.detect(&[
            ("profiles/P.json", br#"{"name": "P", "mods": []}"#),
            ("profiles/P (1).json", br#"{"name": "P", "mods": []}"#),
        ]);

        let _ = s.import(e).unwrap();
        assert_eq!(profile_names(&s.db), ["Vanilla", "Default", "P", "P (2)"]);
    }

    fn memberships(db: &Database) -> BTreeMap<String, Vec<String>> {
        profiles::list(db)
            .unwrap()
            .into_iter()
            .filter(|p| !p.builtin)
            .map(|p| (p.name.clone(), members(db, p.id)))
            .collect()
    }

    fn mod_contents(mod_folder: &Path) -> BTreeMap<String, Vec<u8>> {
        folder(mod_folder)
            .into_iter()
            .map(|(name, bytes)| {
                let name = name
                    .strip_suffix(mods::DISABLED_SUFFIX)
                    .unwrap_or(&name)
                    .to_owned();
                (name, bytes)
            })
            .collect()
    }

    fn source() -> Setup {
        let mut s = Setup::new();
        for (name, bytes) in [
            ("a.pak", &b"AAA"[..]),
            ("b.utoc", b"BBB"),
            ("c.txt.disabled", b"CCC"),
        ] {
            std::fs::write(s.mod_folder.join(name), bytes).unwrap();
        }
        crate::mods::refresh(&mut s.db, &s.mod_folder).unwrap();
        let mod_id =
            |db: &Database, f: &str| -> mods::ModId { mods::id_of(db, f).unwrap().unwrap() };
        let default = profile_id(&s.db, "Default");
        profiles::add_mod(&s.db, default, mod_id(&s.db, "a.pak")).unwrap();
        let pvp = profiles::create(&s.db, "PvP: 1/2").unwrap();
        profiles::add_mod(&s.db, pvp, mod_id(&s.db, "b.utoc")).unwrap();
        profiles::add_mod(&s.db, pvp, mod_id(&s.db, "c.txt")).unwrap();
        profiles::create(&s.db, "pvp_ 1_2").unwrap();
        s
    }

    fn import_fresh(zip: &Path) -> Setup {
        let mut t = Setup::new();
        let mut e = export(detect(&t.db, zip).unwrap());
        for p in &mut e.profiles {
            p.on_clash = Some(ClashResolution::Replace);
        }
        let imported = t.import(e).unwrap();
        assert!(imported.left_out.is_empty(), "{:?}", imported.left_out);
        t
    }

    #[test]
    fn export_all_then_import_round_trips_mods_and_profiles() {
        let s = source();
        let zip = s.dir.path().join("all.zip");
        crate::export::export_all(&s.db, &s.mod_folder, &zip).unwrap();

        let t = import_fresh(&zip);

        assert_eq!(memberships(&t.db), memberships(&s.db));
        assert_eq!(mod_contents(&t.mod_folder), mod_contents(&s.mod_folder));
    }

    #[test]
    fn export_profile_then_import_round_trips_that_profile() {
        let s = source();
        let zip = s.dir.path().join("one.zip");
        let pvp = profile_id(&s.db, "PvP: 1/2");
        crate::export::export_profile(&s.db, &s.mod_folder, pvp, &zip).unwrap();

        let t = import_fresh(&zip);

        assert_eq!(
            memberships(&t.db),
            BTreeMap::from([
                ("Default".to_owned(), vec![]),
                (
                    "PvP: 1/2".to_owned(),
                    vec!["b.utoc".to_owned(), "c.txt".to_owned()]
                ),
            ])
        );
        assert_eq!(
            mod_contents(&t.mod_folder),
            BTreeMap::from([
                ("b.utoc".to_owned(), b"BBB".to_vec()),
                ("c.txt".to_owned(), b"CCC".to_vec()),
            ])
        );
    }
}
