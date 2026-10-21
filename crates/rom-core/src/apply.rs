use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::db::Database;
use crate::mods::{self, ModId};
use crate::profiles::{self, ProfileId};
use crate::{Error, Result};

#[must_use = "a failed apply must be retried or cancelled"]
#[derive(Debug)]
pub enum ApplyOutcome {
    Done,
    Failed(ApplyInProgress),
}

#[derive(Debug)]
pub enum ApplyFailure {
    Rename(RenameFailure),
    Activate(Error),
}

impl ApplyFailure {
    pub fn filename(&self) -> Option<&str> {
        match self {
            Self::Rename(r) => Some(&r.filename),
            Self::Activate(_) => None,
        }
    }

    pub fn error(&self) -> &Error {
        match self {
            Self::Rename(r) => &r.error,
            Self::Activate(e) => e,
        }
    }
}

#[derive(Debug)]
pub struct RenameFailure {
    pub filename: String,
    pub error: Error,
}

type Rename = (String, bool);

#[must_use = "a failed apply must be retried or cancelled"]
#[derive(Debug)]
pub struct ApplyInProgress {
    mod_folder: PathBuf,
    target: Option<ProfileId>,
    done: Vec<Rename>,
    remaining: Vec<Rename>,
    failure: ApplyFailure,
}

pub fn apply(db: &Database, mod_folder: &Path, profile: ProfileId) -> Result<ApplyOutcome> {
    if profiles::get(db, profile)?.is_none() {
        return Err(profiles::ProfileError::NotFound.into());
    }
    let members: HashSet<ModId> = profiles::mods(db, profile)?.into_iter().collect();
    let plan: Vec<Rename> = mods::all(db)?
        .into_iter()
        .map(|(id, filename)| (filename, members.contains(&id)))
        .collect();
    Ok(run(
        db,
        mod_folder.to_owned(),
        Some(profile),
        Vec::new(),
        plan,
    ))
}

impl ApplyInProgress {
    pub fn failure(&self) -> &ApplyFailure {
        &self.failure
    }

    pub fn target(&self) -> Option<ProfileId> {
        self.target
    }

    pub fn retry(self, db: &Database) -> ApplyOutcome {
        run(db, self.mod_folder, self.target, self.done, self.remaining)
    }

    pub fn cancel(self) -> ApplyOutcome {
        let undo = self
            .done
            .into_iter()
            .rev()
            .map(|(filename, enabled)| (filename, !enabled))
            .collect();
        match renames(self.mod_folder, None, Vec::new(), undo) {
            Renamed::All { .. } => ApplyOutcome::Done,
            Renamed::Stopped(in_progress) => ApplyOutcome::Failed(in_progress),
        }
    }
}

fn run(
    db: &Database,
    mod_folder: PathBuf,
    target: Option<ProfileId>,
    done: Vec<Rename>,
    remaining: Vec<Rename>,
) -> ApplyOutcome {
    let (mod_folder, done) = match renames(mod_folder, target, done, remaining) {
        Renamed::All { mod_folder, done } => (mod_folder, done),
        Renamed::Stopped(in_progress) => return ApplyOutcome::Failed(in_progress),
    };
    if let Some(profile) = target
        && let Err(error) = profiles::set_active(db, profile)
    {
        return ApplyOutcome::Failed(ApplyInProgress {
            mod_folder,
            target,
            done,
            remaining: Vec::new(),
            failure: ApplyFailure::Activate(error),
        });
    }
    ApplyOutcome::Done
}

enum Renamed {
    All {
        mod_folder: PathBuf,
        done: Vec<Rename>,
    },
    Stopped(ApplyInProgress),
}

fn renames(
    mod_folder: PathBuf,
    target: Option<ProfileId>,
    mut done: Vec<Rename>,
    remaining: Vec<Rename>,
) -> Renamed {
    let mut queue = remaining.into_iter();
    while let Some((filename, enabled)) = queue.next() {
        if is_in_state(&mod_folder, &filename, enabled) {
            continue;
        }
        match mods::set_enabled(&mod_folder, &filename, enabled) {
            Ok(()) => done.push((filename, enabled)),
            Err(error) => {
                let failure = ApplyFailure::Rename(RenameFailure {
                    filename: filename.clone(),
                    error,
                });
                let remaining = std::iter::once((filename, enabled)).chain(queue).collect();
                return Renamed::Stopped(ApplyInProgress {
                    mod_folder,
                    target,
                    done,
                    remaining,
                    failure,
                });
            }
        }
    }
    Renamed::All { mod_folder, done }
}

fn is_in_state(mod_folder: &Path, filename: &str, enabled: bool) -> bool {
    let want = mods::ModState::from_enabled(enabled);
    mods::path_in_state(mod_folder, filename, want).is_file()
        && !mods::path_in_state(mod_folder, filename, want.toggled()).exists()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{mods, profiles};
    use std::fs;
    use tempfile::TempDir;

    struct Fixture {
        db: Database,
        dir: TempDir,
        default: ProfileId,
        coop: ProfileId,
    }

    fn fixture() -> Fixture {
        let mut db = Database::open_in_memory().unwrap();
        profiles::init(&mut db).unwrap();
        crate::settings::set_auto_enable(&db, false).unwrap();
        let dir = tempfile::tempdir().unwrap();
        for name in ["a.pak", "b.pak", "c.pak"] {
            fs::write(dir.path().join(name), name).unwrap();
        }
        mods::refresh(&mut db, dir.path()).unwrap();
        let default = profiles::active(&db).unwrap();
        let coop = profiles::create(&db, "Co-op").unwrap();
        profiles::add_mod(&db, default, mod_id(&db, "a.pak")).unwrap();
        profiles::add_mod(&db, coop, mod_id(&db, "b.pak")).unwrap();
        mods::enable(dir.path(), "a.pak").unwrap();
        Fixture {
            db,
            dir,
            default,
            coop,
        }
    }

    fn mod_id(db: &Database, filename: &str) -> ModId {
        mods::id_of(db, filename).unwrap().unwrap()
    }

    fn files(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap())
            .filter(|e| e.file_type().unwrap().is_file())
            .map(|e| e.file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn applying_a_profile_enables_its_mods_disables_the_rest_and_makes_it_active() {
        let f = fixture();

        let outcome = apply(&f.db, f.dir.path(), f.coop).unwrap();

        assert!(matches!(outcome, ApplyOutcome::Done));
        assert_eq!(
            files(f.dir.path()),
            ["a.pak.disabled", "b.pak", "c.pak.disabled"]
        );
        assert_eq!(profiles::active(&f.db).unwrap(), f.coop);
    }

    #[test]
    fn applying_vanilla_disables_every_mod() {
        let f = fixture();
        let vanilla = profiles::vanilla(&f.db).unwrap();

        let outcome = apply(&f.db, f.dir.path(), vanilla).unwrap();

        assert!(matches!(outcome, ApplyOutcome::Done));
        assert_eq!(
            files(f.dir.path()),
            ["a.pak.disabled", "b.pak.disabled", "c.pak.disabled"]
        );
        assert_eq!(profiles::active(&f.db).unwrap(), vanilla);
    }

    fn block(dir: &Path, target: &str) {
        fs::create_dir_all(dir.join(target).join("blocker")).unwrap();
    }

    fn unblock(dir: &Path, target: &str) {
        fs::remove_dir_all(dir.join(target)).unwrap();
    }

    fn failed(outcome: ApplyOutcome) -> ApplyInProgress {
        match outcome {
            ApplyOutcome::Failed(in_progress) => in_progress,
            ApplyOutcome::Done => panic!("expected the apply to fail"),
        }
    }

    #[test]
    fn a_failed_rename_stops_the_apply_and_cancel_restores_the_previous_filenames() {
        let f = fixture();
        let before = files(f.dir.path());
        block(f.dir.path(), "b.pak");

        let in_progress = failed(apply(&f.db, f.dir.path(), f.coop).unwrap());

        assert_eq!(in_progress.failure().filename(), Some("b.pak"));
        assert_eq!(profiles::active(&f.db).unwrap(), f.default);
        assert!(matches!(in_progress.cancel(), ApplyOutcome::Done));
        assert_eq!(files(f.dir.path()), before);
        assert_eq!(profiles::active(&f.db).unwrap(), f.default);
    }

    #[test]
    fn retry_fails_again_while_the_cause_remains_and_completes_once_it_is_removed() {
        let f = fixture();
        block(f.dir.path(), "b.pak");
        let in_progress = failed(apply(&f.db, f.dir.path(), f.coop).unwrap());

        let in_progress = failed(in_progress.retry(&f.db));
        assert_eq!(in_progress.failure().filename(), Some("b.pak"));
        assert_eq!(profiles::active(&f.db).unwrap(), f.default);

        unblock(f.dir.path(), "b.pak");
        let outcome = in_progress.retry(&f.db);

        assert!(matches!(outcome, ApplyOutcome::Done));
        assert_eq!(
            files(f.dir.path()),
            ["a.pak.disabled", "b.pak", "c.pak.disabled"]
        );
        assert_eq!(profiles::active(&f.db).unwrap(), f.coop);
    }

    #[test]
    fn a_failed_cancel_can_be_retried_until_the_previous_filenames_are_restored() {
        let f = fixture();
        let before = files(f.dir.path());
        block(f.dir.path(), "b.pak");
        let in_progress = failed(apply(&f.db, f.dir.path(), f.coop).unwrap());
        block(f.dir.path(), "a.pak");

        let rollback = failed(in_progress.cancel());
        assert_eq!(rollback.failure().filename(), Some("a.pak"));
        assert_eq!(rollback.target(), None);

        unblock(f.dir.path(), "a.pak");
        let outcome = rollback.retry(&f.db);

        assert!(matches!(outcome, ApplyOutcome::Done));
        assert_eq!(files(f.dir.path()), before);
        assert_eq!(profiles::active(&f.db).unwrap(), f.default);
    }

    fn break_settings(db: &Database, broken: bool) {
        let sql = if broken {
            "CREATE TEMP TRIGGER no_insert BEFORE INSERT ON settings
               BEGIN SELECT RAISE(ABORT, 'injected'); END;
             CREATE TEMP TRIGGER no_update BEFORE UPDATE ON settings
               BEGIN SELECT RAISE(ABORT, 'injected'); END;"
        } else {
            "DROP TRIGGER temp.no_insert; DROP TRIGGER temp.no_update;"
        };
        db.conn().execute_batch(sql).unwrap();
    }

    #[test]
    fn failing_to_make_the_profile_active_after_every_rename_can_be_retried() {
        let f = fixture();
        break_settings(&f.db, true);

        let in_progress = failed(apply(&f.db, f.dir.path(), f.coop).unwrap());
        assert!(
            matches!(in_progress.failure(), ApplyFailure::Activate(_)),
            "{:?}",
            in_progress.failure()
        );
        assert_eq!(profiles::active(&f.db).unwrap(), f.default);
        let in_progress = failed(in_progress.retry(&f.db));

        break_settings(&f.db, false);
        let outcome = in_progress.retry(&f.db);

        assert!(matches!(outcome, ApplyOutcome::Done));
        assert_eq!(
            files(f.dir.path()),
            ["a.pak.disabled", "b.pak", "c.pak.disabled"]
        );
        assert_eq!(profiles::active(&f.db).unwrap(), f.coop);
    }

    #[test]
    fn failing_to_make_the_profile_active_can_be_cancelled() {
        let f = fixture();
        let before = files(f.dir.path());
        break_settings(&f.db, true);
        let in_progress = failed(apply(&f.db, f.dir.path(), f.coop).unwrap());

        assert!(matches!(in_progress.cancel(), ApplyOutcome::Done));

        assert_eq!(files(f.dir.path()), before);
        assert_eq!(profiles::active(&f.db).unwrap(), f.default);
    }

    #[test]
    fn applying_an_unknown_profile_renames_nothing() {
        let f = fixture();
        let before = files(f.dir.path());

        let err = apply(&f.db, f.dir.path(), ProfileId(999)).unwrap_err();

        assert!(matches!(
            err,
            Error::Profile(profiles::ProfileError::NotFound)
        ));
        assert_eq!(files(f.dir.path()), before);
    }
}
