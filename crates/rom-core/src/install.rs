use std::collections::{BTreeMap, HashMap};
use std::path::{Component, Path, PathBuf};

use crate::Result;
use crate::db::Database;
use crate::mods::{
    self, DISABLED_SUFFIX, Mod, ModState, auto_enable_profile, disabled_path, enabled_path,
    path_in_state, state_on_disk,
};
use crate::profiles::{self, ProfileId};

mod archive;
#[cfg(test)]
pub(crate) mod fixtures;

#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    #[error("could not read archive {archive}: {message}")]
    Archive { archive: PathBuf, message: String },
    #[error("archive {archive} has no entry {entry}")]
    MissingEntry { archive: PathBuf, entry: String },
    #[error("no valid decision for the collision on {0}")]
    UnresolvedCollision(String),
    #[error("{0:?} is not a safe Mod filename")]
    UnsafeFilename(String),
}

const MOD_EXTENSIONS: &[&str] = &["pak", "utoc", "ucas"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    File(PathBuf),
    Archive {
        archive: PathBuf,
        format: ArchiveFormat,
        entry: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArchiveFormat {
    Zip,
    SevenZ,
    Tar,
}

impl ArchiveFormat {
    pub fn of(path: &Path) -> Option<Self> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        match ext.as_str() {
            "zip" => Some(Self::Zip),
            "7z" => Some(Self::SevenZ),
            "tar" => Some(Self::Tar),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub filename: String,
    pub size: u64,
    pub preselected: bool,
    pub source: Source,
}

pub fn inspect(paths: &[PathBuf]) -> Result<Inspection> {
    let mut cands = Vec::new();
    let mut rejected = Vec::new();
    for path in paths {
        let Some(format) = ArchiveFormat::of(path) else {
            let filename = path
                .file_name()
                .map(|f| f.to_string_lossy().into_owned())
                .unwrap_or_default();
            if !is_safe_filename(&filename) {
                rejected.push(path.to_string_lossy().into_owned());
                continue;
            }
            cands.push(Candidate {
                preselected: preselect(&filename),
                filename,
                size: std::fs::metadata(path)?.len(),
                source: Source::File(path.clone()),
            });
            continue;
        };
        for e in archive::list(path, format)? {
            let Some(filename) = flatten(&e.name) else {
                rejected.push(e.name);
                continue;
            };
            cands.push(Candidate {
                preselected: preselect(&filename),
                filename,
                size: e.size,
                source: Source::Archive {
                    archive: path.clone(),
                    format,
                    entry: e.name,
                },
            });
        }
    }
    Ok(Inspection {
        candidates: cands,
        rejected,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inspection {
    pub candidates: Vec<Candidate>,
    pub rejected: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Collision {
    pub filename: String,
    pub candidates: Vec<Candidate>,
    pub existing: Option<ModState>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolution {
    Skip,
    Use(usize),
}

#[derive(Debug, Clone)]
#[must_use = "an InstallPlan does nothing until passed to install()"]
pub struct InstallPlan {
    clear: Vec<Candidate>,
    collisions: Vec<Collision>,
    resolutions: Vec<Option<Resolution>>,
}

impl InstallPlan {
    pub fn collisions(&self) -> &[Collision] {
        &self.collisions
    }

    pub fn resolve(&mut self, collision: usize, resolution: Resolution) {
        if let Some(r) = self.resolutions.get_mut(collision) {
            *r = Some(resolution);
        }
    }
}

pub fn plan(mod_folder: &Path, selection: Vec<Candidate>) -> Result<InstallPlan> {
    let mut by_name: BTreeMap<String, Vec<Candidate>> = BTreeMap::new();
    for c in selection {
        if !is_safe_filename(&c.filename) {
            return Err(InstallError::UnsafeFilename(c.filename).into());
        }
        by_name.entry(c.filename.clone()).or_default().push(c);
    }
    let mut clear = Vec::new();
    let mut collisions = Vec::new();
    for (filename, mut candidates) in by_name {
        let existing = state_on_disk(mod_folder, &filename)?;
        if candidates.len() == 1 && existing.is_none() {
            clear.append(&mut candidates);
        } else {
            collisions.push(Collision {
                filename,
                candidates,
                existing,
            });
        }
    }
    let resolutions = vec![None; collisions.len()];
    Ok(InstallPlan {
        clear,
        collisions,
        resolutions,
    })
}

pub fn install(db: &mut Database, mod_folder: &Path, plan: InstallPlan) -> Result<Vec<Mod>> {
    let auto_enable_into = auto_enable_profile(db)?;
    install_into(db, mod_folder, plan, auto_enable_into)
}

pub(crate) fn install_into(
    db: &mut Database,
    mod_folder: &Path,
    plan: InstallPlan,
    auto_enable_into: Option<ProfileId>,
) -> Result<Vec<Mod>> {
    let mut chosen: Vec<(Candidate, Option<ModState>)> =
        plan.clear.into_iter().map(|c| (c, None)).collect();
    for (collision, resolution) in plan.collisions.into_iter().zip(plan.resolutions) {
        match resolution {
            Some(Resolution::Skip) => {}
            Some(Resolution::Use(i)) if i < collision.candidates.len() => {
                let existing = collision.existing;
                let c = collision.candidates.into_iter().nth(i);
                chosen.extend(c.map(|c| (c, existing)));
            }
            _ => return Err(InstallError::UnresolvedCollision(collision.filename).into()),
        }
    }
    chosen.sort_by(|a, b| a.0.filename.cmp(&b.0.filename));

    std::fs::create_dir_all(mod_folder)?;
    mods::with_staging(mod_folder, |staging| {
        stage_and_commit(db, mod_folder, staging, &chosen, auto_enable_into)
    })
}

fn stage_and_commit(
    db: &mut Database,
    mod_folder: &Path,
    staging: &Path,
    chosen: &[(Candidate, Option<ModState>)],
    auto_enable_into: Option<ProfileId>,
) -> Result<Vec<Mod>> {
    let mut staged = Vec::with_capacity(chosen.len());
    for (c, _) in chosen {
        let s = staging.join(format!("{}.part", c.filename));
        let inside = s.parent() == Some(staging)
            && enabled_path(mod_folder, &c.filename).parent() == Some(mod_folder)
            && disabled_path(mod_folder, &c.filename).parent() == Some(mod_folder);
        if !inside {
            return Err(InstallError::UnsafeFilename(c.filename.clone()).into());
        }
        staged.push(s);
    }
    std::fs::create_dir_all(staging)?;
    extract_all(chosen.iter().map(|(c, _)| &c.source).zip(&staged))?;

    let tx = db.conn().unchecked_transaction()?;
    let mut installed = Vec::with_capacity(chosen.len());
    for ((c, existing), staged) in chosen.iter().zip(&staged) {
        let state = existing.unwrap_or(ModState::from_enabled(auto_enable_into.is_some()));
        let target = path_in_state(mod_folder, &c.filename, state);
        if mods::id_of(db, &c.filename)?.is_none() {
            let id = mods::insert(db, &c.filename)?;
            if existing.is_none()
                && let Some(profile) = auto_enable_into
            {
                profiles::add_mod(db, profile, id)?;
            }
        }
        std::fs::rename(staged, target)?;
        installed.push(Mod {
            filename: c.filename.clone(),
            enabled: state.is_enabled(),
        });
    }
    tx.commit()?;
    Ok(installed)
}

fn extract_all<'a>(jobs: impl Iterator<Item = (&'a Source, &'a PathBuf)>) -> Result<()> {
    let mut per_archive: HashMap<(&Path, ArchiveFormat), Vec<(&str, &Path)>> = HashMap::new();
    for (source, dest) in jobs {
        match source {
            Source::File(path) => {
                std::fs::copy(path, dest)?;
            }
            Source::Archive {
                archive,
                format,
                entry,
            } => per_archive
                .entry((archive, *format))
                .or_default()
                .push((entry, dest)),
        }
    }
    for ((path, format), wanted) in per_archive {
        archive::extract(path, format, &wanted)?;
    }
    Ok(())
}

fn flatten(name: &str) -> Option<String> {
    let last = name.rsplit(['/', '\\']).next()?;
    is_safe_filename(last).then(|| last.to_owned())
}

pub(crate) fn is_safe_filename(name: &str) -> bool {
    const INVALID: &[char] = &['<', '>', ':', '"', '/', '\\', '|', '?', '*'];
    const RESERVED: &[&str] = &["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"];
    if name.is_empty()
        || name.ends_with(['.', ' '])
        || ends_with_ignore_ascii_case(name, DISABLED_SUFFIX)
        || name.chars().any(|c| c.is_control() || INVALID.contains(&c))
    {
        return false;
    }

    let stem = name.split('.').next().unwrap_or(name).trim_end();
    let numbered_device = ["COM", "LPT"].iter().any(|prefix| {
        stem.len() > 3
            && stem.is_char_boundary(3)
            && stem[..3].eq_ignore_ascii_case(prefix)
            && matches!(
                &stem[3..],
                "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
            )
    });
    if numbered_device || RESERVED.iter().any(|r| stem.eq_ignore_ascii_case(r)) {
        return false;
    }
    let mut components = Path::new(name).components();
    matches!(
        (components.next(), components.next()),
        (Some(Component::Normal(_)), None)
    )
}

fn ends_with_ignore_ascii_case(s: &str, suffix: &str) -> bool {
    s.len() >= suffix.len()
        && s.is_char_boundary(s.len() - suffix.len())
        && s[s.len() - suffix.len()..].eq_ignore_ascii_case(suffix)
}

fn preselect(filename: &str) -> bool {
    !filename.starts_with("._") && has_extension(filename, MOD_EXTENSIONS)
}

fn has_extension(filename: &str, exts: &[&str]) -> bool {
    Path::new(filename)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| exts.iter().any(|x| e.eq_ignore_ascii_case(x)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary(cands: &[Candidate]) -> Vec<(&str, bool)> {
        cands
            .iter()
            .map(|c| (c.filename.as_str(), c.preselected))
            .collect()
    }

    const EXPECTED: &[(&str, bool)] = &[
        ("pakchunk99-MyMod_P.pak", true),
        ("pakchunk99-MyMod_P.utoc", true),
        ("pakchunk99-MyMod_P.ucas", true),
        ("readme.txt", false),
        ("._pakchunk99-MyMod_P.pak", false),
    ];

    #[test]
    fn inspecting_a_zip_lists_flattened_files_with_mod_files_preselected() {
        let dir = tempfile::tempdir().unwrap();
        let zip = fixtures::zip(dir.path(), "mod.zip", fixtures::TREE);

        let cands = inspect(&[zip]).unwrap().candidates;

        assert_eq!(summary(&cands), EXPECTED);
    }

    #[test]
    fn inspecting_loose_files_lists_each_with_mod_files_preselected() {
        let dir = tempfile::tempdir().unwrap();
        let pak = dir.path().join("a.PAK");
        let txt = dir.path().join("notes.txt");
        std::fs::write(&pak, b"12345").unwrap();
        std::fs::write(&txt, b"").unwrap();

        let cands = inspect(&[pak.clone(), txt]).unwrap().candidates;

        assert_eq!(summary(&cands), [("a.PAK", true), ("notes.txt", false)]);
        assert_eq!(cands[0].size, 5);
        assert_eq!(cands[0].source, Source::File(pak));
    }

    #[test]
    fn inspecting_a_tar_lists_flattened_files_with_mod_files_preselected() {
        let dir = tempfile::tempdir().unwrap();
        let tar = fixtures::tar(dir.path(), "mod.TAR", fixtures::TREE);

        let cands = inspect(&[tar]).unwrap().candidates;
        assert_eq!(summary(&cands), EXPECTED);
    }

    #[test]
    fn inspecting_a_7z_lists_flattened_files_with_mod_files_preselected() {
        let dir = tempfile::tempdir().unwrap();
        let sevenz = fixtures::sevenz(dir.path(), "mod.7z", fixtures::TREE);

        let mut cands = inspect(&[sevenz]).unwrap().candidates;

        cands.sort_by(|a, b| a.filename.cmp(&b.filename));
        let mut expected = EXPECTED.to_vec();
        expected.sort();
        assert_eq!(summary(&cands), expected);
    }

    fn setup() -> (Database, tempfile::TempDir, PathBuf) {
        let mut db = Database::open_in_memory().unwrap();
        profiles::init(&mut db).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let mod_folder = dir.path().join("RoM");
        (db, dir, mod_folder)
    }

    fn files(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }

    fn members(db: &Database, profile: ProfileId) -> Vec<String> {
        profiles::mod_filenames(db, profile).unwrap()
    }

    fn preselected(cands: Vec<Candidate>) -> Vec<Candidate> {
        cands.into_iter().filter(|c| c.preselected).collect()
    }

    fn enabled(name: &str) -> Mod {
        Mod {
            filename: name.into(),
            enabled: true,
        }
    }

    #[test]
    fn installing_from_a_zip_writes_enabled_mods_into_the_active_profile() {
        let (mut db, dir, mod_folder) = setup();
        let zip = fixtures::zip(dir.path(), "mod.zip", fixtures::TREE);
        let selection = preselected(inspect(&[zip]).unwrap().candidates);

        let plan = plan(&mod_folder, selection).unwrap();
        assert!(plan.collisions().is_empty());
        let installed = install(&mut db, &mod_folder, plan).unwrap();

        let names = [
            "pakchunk99-MyMod_P.pak",
            "pakchunk99-MyMod_P.ucas",
            "pakchunk99-MyMod_P.utoc",
        ];
        assert_eq!(installed, names.map(enabled));
        assert_eq!(files(&mod_folder), names);
        assert_eq!(
            std::fs::read(mod_folder.join("pakchunk99-MyMod_P.utoc")).unwrap(),
            b"utoc"
        );
        let active = profiles::active(&db).unwrap();
        assert_eq!(members(&db, active), names);
    }

    fn install_all_preselected(archive: PathBuf) -> (Vec<Mod>, PathBuf, tempfile::TempDir) {
        let (mut db, dir, mod_folder) = setup();
        let plan = plan(
            &mod_folder,
            preselected(inspect(&[archive]).unwrap().candidates),
        )
        .unwrap();
        let installed = install(&mut db, &mod_folder, plan).unwrap();
        (installed, mod_folder, dir)
    }

    #[test]
    fn installing_from_a_7z_and_a_tar_writes_each_selected_file() {
        let src = tempfile::tempdir().unwrap();
        for archive in [
            fixtures::sevenz(src.path(), "mod.7z", fixtures::TREE),
            fixtures::tar(src.path(), "mod.tar", fixtures::TREE),
        ] {
            let (installed, mod_folder, _dir) = install_all_preselected(archive);

            assert_eq!(installed.len(), 3);
            assert_eq!(
                std::fs::read(mod_folder.join("pakchunk99-MyMod_P.pak")).unwrap(),
                b"pak"
            );
            assert_eq!(
                std::fs::read(mod_folder.join("pakchunk99-MyMod_P.ucas")).unwrap(),
                b"ucas"
            );
        }
    }

    #[test]
    fn with_auto_enable_off_new_mods_are_disabled_and_outside_every_profile() {
        let (mut db, dir, mod_folder) = setup();
        crate::settings::set_auto_enable(&db, false).unwrap();
        let pak = dir.path().join("a.pak");
        std::fs::write(&pak, b"a").unwrap();

        let plan = plan(&mod_folder, inspect(&[pak]).unwrap().candidates).unwrap();
        let installed = install(&mut db, &mod_folder, plan).unwrap();

        assert!(!installed[0].enabled);
        assert_eq!(files(&mod_folder), ["a.pak.disabled"]);
        assert!(members(&db, profiles::active(&db).unwrap()).is_empty());
    }

    fn existing_disabled_mod(db: &mut Database, mod_folder: &Path) -> ProfileId {
        std::fs::create_dir_all(mod_folder).unwrap();
        std::fs::write(mod_folder.join("a.pak.disabled"), b"old").unwrap();
        crate::settings::set_auto_enable(db, false).unwrap();
        crate::mods::refresh(db, mod_folder).unwrap();
        crate::settings::set_auto_enable(db, true).unwrap();
        let other = profiles::create(db, "Other").unwrap();
        let id = mods::id_of(db, "a.pak").unwrap().unwrap();
        profiles::add_mod(db, other, id).unwrap();
        other
    }

    #[test]
    fn replacing_an_existing_mod_keeps_its_memberships_and_enabled_state() {
        let (mut db, dir, mod_folder) = setup();
        let other = existing_disabled_mod(&mut db, &mod_folder);
        let pak = dir.path().join("a.pak");
        std::fs::write(&pak, b"new").unwrap();

        let mut plan = plan(
            &mod_folder,
            inspect(std::slice::from_ref(&pak)).unwrap().candidates,
        )
        .unwrap();
        assert_eq!(
            plan.collisions(),
            [Collision {
                filename: "a.pak".into(),
                candidates: inspect(&[pak]).unwrap().candidates,
                existing: Some(ModState::Disabled),
            }]
        );
        plan.resolve(0, Resolution::Use(0));
        let installed = install(&mut db, &mod_folder, plan).unwrap();

        assert_eq!(
            installed,
            [Mod {
                filename: "a.pak".into(),
                enabled: false
            }]
        );
        assert_eq!(files(&mod_folder), ["a.pak.disabled"]);
        assert_eq!(
            std::fs::read(mod_folder.join("a.pak.disabled")).unwrap(),
            b"new"
        );
        assert_eq!(members(&db, other), ["a.pak"]);
        assert!(members(&db, profiles::active(&db).unwrap()).is_empty());
    }

    #[test]
    fn cancelling_at_the_collision_step_writes_nothing() {
        let (mut db, dir, mod_folder) = setup();
        existing_disabled_mod(&mut db, &mod_folder);
        let zip = fixtures::zip(
            dir.path(),
            "mod.zip",
            &[("x/a.pak", b"new"), ("x/b.pak", b"b")],
        );

        let plan = plan(&mod_folder, inspect(&[zip]).unwrap().candidates).unwrap();
        assert_eq!(plan.collisions().len(), 1);
        drop(plan);

        assert_eq!(files(&mod_folder), ["a.pak.disabled"]);
        assert_eq!(
            std::fs::read(mod_folder.join("a.pak.disabled")).unwrap(),
            b"old"
        );
    }

    #[test]
    fn installing_with_an_unresolved_collision_fails_and_writes_nothing() {
        let (mut db, dir, mod_folder) = setup();
        existing_disabled_mod(&mut db, &mod_folder);
        let zip = fixtures::zip(
            dir.path(),
            "mod.zip",
            &[("x/a.pak", b"new"), ("x/b.pak", b"b")],
        );

        let plan = plan(&mod_folder, inspect(&[zip]).unwrap().candidates).unwrap();
        let err = install(&mut db, &mod_folder, plan).unwrap_err();

        assert!(matches!(
            err,
            crate::Error::Install(InstallError::UnresolvedCollision(ref f)) if f == "a.pak"
        ));
        assert_eq!(files(&mod_folder), ["a.pak.disabled"]);
    }

    #[test]
    fn skipping_a_collision_leaves_the_existing_mod_and_installs_the_rest() {
        let (mut db, dir, mod_folder) = setup();
        existing_disabled_mod(&mut db, &mod_folder);
        let zip = fixtures::zip(
            dir.path(),
            "mod.zip",
            &[("x/a.pak", b"new"), ("x/b.pak", b"b")],
        );

        let mut plan = plan(&mod_folder, inspect(&[zip]).unwrap().candidates).unwrap();
        plan.resolve(0, Resolution::Skip);
        let installed = install(&mut db, &mod_folder, plan).unwrap();

        assert_eq!(installed, [enabled("b.pak")]);
        assert_eq!(files(&mod_folder), ["a.pak.disabled", "b.pak"]);
        assert_eq!(
            std::fs::read(mod_folder.join("a.pak.disabled")).unwrap(),
            b"old"
        );
    }

    #[test]
    fn same_name_files_in_one_selection_collide_and_the_chosen_one_is_installed() {
        let (mut db, dir, mod_folder) = setup();
        let zip = fixtures::zip(
            dir.path(),
            "mod.zip",
            &[("v1/a.pak", b"one"), ("v2/a.pak", b"two")],
        );

        let mut plan = plan(&mod_folder, inspect(&[zip]).unwrap().candidates).unwrap();
        let [collision] = plan.collisions() else {
            panic!("expected one collision, got {:?}", plan.collisions());
        };
        assert_eq!(collision.existing, None);
        assert_eq!(collision.candidates.len(), 2);
        plan.resolve(0, Resolution::Use(1));
        let installed = install(&mut db, &mod_folder, plan).unwrap();

        assert_eq!(installed, [enabled("a.pak")]);
        assert_eq!(std::fs::read(mod_folder.join("a.pak")).unwrap(), b"two");
        assert_eq!(members(&db, profiles::active(&db).unwrap()), ["a.pak"]);
    }

    #[test]
    fn a_failed_extraction_leaves_the_mod_folder_untouched() {
        let (mut db, dir, mod_folder) = setup();
        let pak = dir.path().join("a.pak");
        std::fs::write(&pak, b"a").unwrap();
        let mut selection = inspect(&[pak]).unwrap().candidates;
        selection.push(Candidate {
            filename: "gone.pak".into(),
            size: 0,
            preselected: true,
            source: Source::File(dir.path().join("gone.pak")),
        });

        let plan = plan(&mod_folder, selection).unwrap();
        assert!(install(&mut db, &mod_folder, plan).is_err());

        assert!(files(&mod_folder).is_empty());
        assert!(mods::all(&db).unwrap().is_empty());
    }

    #[test]
    fn inspecting_skips_and_reports_names_that_are_not_safe_filenames() {
        let dir = tempfile::tempdir().unwrap();
        let bad = [
            "x/C:evil.pak",
            "x.pak:stream",
            "a<b.pak",
            "a>b.pak",
            "a\"b.pak",
            "a|b.pak",
            "a?b.pak",
            "a*b.pak",
            "tab\there.pak",
            "x/CON",
            "x/nul.pak",
            "x/Com1.txt",
            "x/LPT9",
            "trailing.pak.",
            "trailing.pak ",
            "x/..",
            "x/foo.pak.disabled",
            "x/foo.pak.DISABLED",
        ];
        let mut tree: Vec<(&str, &[u8])> = bad.iter().map(|n| (*n, &b"x"[..])).collect();
        tree.push(("x/ok.pak", b"ok"));
        tree.push(("x/console.pak", b"ok"));
        let zip = fixtures::zip(dir.path(), "mod.zip", &tree);

        let inspection = inspect(&[zip]).unwrap();

        assert_eq!(
            summary(&inspection.candidates),
            [("ok.pak", true), ("console.pak", true)]
        );
        let expected: Vec<String> = bad.iter().map(|s| s.to_string()).collect();
        assert_eq!(inspection.rejected, expected);
    }

    #[test]
    fn planning_a_candidate_with_an_unsafe_filename_fails() {
        let (_db, dir, mod_folder) = setup();
        for filename in [
            "../escape.pak",
            "C:evil.pak",
            "a/b.pak",
            "NUL",
            "",
            "foo.pak.disabled",
        ] {
            let selection = vec![Candidate {
                filename: filename.into(),
                size: 0,
                preselected: true,
                source: Source::File(dir.path().join("whatever")),
            }];

            let err = plan(&mod_folder, selection).unwrap_err();

            assert!(
                matches!(err, crate::Error::Install(InstallError::UnsafeFilename(ref f)) if f == filename),
                "{filename}: {err:?}"
            );
        }
    }
}
