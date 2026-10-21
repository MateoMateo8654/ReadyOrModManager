use crate::db::{Database, setting};
use std::collections::HashSet;

use crate::Result;
use crate::mods::ModId;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProfileError {
    #[error("the Vanilla Profile cannot be changed")]
    VanillaImmutable,
    #[error("a Profile named \"{0}\" already exists")]
    NameTaken(String),
    #[error("Profile name must not be empty")]
    EmptyName,
    #[error("no such Profile")]
    NotFound,
    #[error("the Active Profile cannot be deleted")]
    DeleteActive,
    #[error("no Active Profile is set (profiles::init was not run)")]
    NoActiveProfile,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProfileId(pub i64);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    pub id: ProfileId,
    pub name: String,
    pub builtin: bool,
}

pub const VANILLA: &str = "Vanilla";
pub const DEFAULT: &str = "Default";

pub fn init(db: &mut Database) -> Result<()> {
    let tx = db.conn_mut().transaction()?;
    tx.execute(
        "INSERT OR IGNORE INTO profiles (name, builtin) VALUES (?1, 1)",
        [VANILLA],
    )?;
    let has_active: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM settings s JOIN profiles p
                       ON CAST(s.value AS INTEGER) = p.id
                       WHERE s.key = ?1)",
        [setting::ACTIVE_PROFILE],
        |r| r.get(0),
    )?;
    if !has_active {
        tx.execute(
            "INSERT OR IGNORE INTO profiles (name, builtin) VALUES (?1, 0)",
            [DEFAULT],
        )?;
        let id: i64 = tx.query_row("SELECT id FROM profiles WHERE name = ?1", [DEFAULT], |r| {
            r.get(0)
        })?;
        tx.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [setting::ACTIVE_PROFILE, &id.to_string()],
        )?;
    }
    tx.commit()?;
    Ok(())
}

pub fn reset(db: &mut Database) -> Result<()> {
    db.conn()
        .execute("DELETE FROM profiles WHERE builtin = 0", [])?;
    db.conn().execute(
        "DELETE FROM settings WHERE key = ?1",
        [setting::ACTIVE_PROFILE],
    )?;
    init(db)
}

pub fn list(db: &Database) -> Result<Vec<Profile>> {
    let mut stmt = db
        .conn()
        .prepare("SELECT id, name, builtin FROM profiles ORDER BY builtin DESC, id")?;
    let rows = stmt.query_map([], |r| {
        Ok(Profile {
            id: ProfileId(r.get(0)?),
            name: r.get(1)?,
            builtin: r.get(2)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub fn get(db: &Database, id: ProfileId) -> Result<Option<Profile>> {
    use rusqlite::OptionalExtension;
    Ok(db
        .conn()
        .query_row(
            "SELECT id, name, builtin FROM profiles WHERE id = ?1",
            [id.0],
            |r| {
                Ok(Profile {
                    id: ProfileId(r.get(0)?),
                    name: r.get(1)?,
                    builtin: r.get(2)?,
                })
            },
        )
        .optional()?)
}

pub fn by_name(db: &Database, name: &str) -> Result<Option<ProfileId>> {
    use rusqlite::OptionalExtension;
    let id = db
        .conn()
        .query_row(
            "SELECT id FROM profiles WHERE name = ?1 COLLATE NOCASE",
            [name],
            |r| r.get(0),
        )
        .optional()?;
    Ok(id.map(ProfileId))
}

pub(crate) fn free_name(db: &Database, name: &str) -> Result<String> {
    let mut taken: HashSet<String> = list(db)?.into_iter().map(|p| name_key(&p.name)).collect();
    taken.insert(name_key(name));
    Ok(unique_name(name, &mut taken))
}

pub(crate) fn unique_name(base: &str, taken: &mut HashSet<String>) -> String {
    let mut name = base.to_owned();
    let mut n = 1;
    while !taken.insert(name_key(&name)) {
        n += 1;
        name = format!("{base} ({n})");
    }
    name
}

pub(crate) fn name_key(name: &str) -> String {
    name.to_lowercase()
}

pub fn create(db: &Database, name: &str) -> Result<ProfileId> {
    let name = available_name(db, name, None)?;
    db.conn().execute(
        "INSERT INTO profiles (name, builtin) VALUES (?1, 0)",
        [name],
    )?;
    Ok(ProfileId(db.conn().last_insert_rowid()))
}

pub fn duplicate(db: &Database, from: ProfileId, name: &str) -> Result<ProfileId> {
    let mods = mods(db, from)?;
    let tx = db.conn().unchecked_transaction()?;
    let id = create(db, name)?;
    set_mods(db, id, &mods)?;
    tx.commit()?;
    Ok(id)
}

pub fn rename(db: &Database, id: ProfileId, name: &str) -> Result<()> {
    editable(db, id)?;
    let name = available_name(db, name, Some(id))?;
    db.conn().execute(
        "UPDATE profiles SET name = ?1 WHERE id = ?2",
        rusqlite::params![name, id.0],
    )?;
    Ok(())
}

pub fn delete(db: &Database, id: ProfileId) -> Result<()> {
    editable(db, id)?;
    if active(db)? == id {
        return Err(ProfileError::DeleteActive.into());
    }
    db.conn()
        .execute("DELETE FROM profiles WHERE id = ?1", [id.0])?;
    Ok(())
}

pub fn set_active(db: &Database, id: ProfileId) -> Result<()> {
    if get(db, id)?.is_none() {
        return Err(ProfileError::NotFound.into());
    }
    db.set_setting(setting::ACTIVE_PROFILE, &id.0.to_string())
}

pub fn add_mod(db: &Database, id: ProfileId, mod_id: ModId) -> Result<()> {
    editable(db, id)?;
    db.conn().execute(
        "INSERT OR IGNORE INTO profile_mods (profile_id, mod_id) VALUES (?1, ?2)",
        [id.0, mod_id.0],
    )?;
    Ok(())
}

pub fn remove_mod(db: &Database, id: ProfileId, mod_id: ModId) -> Result<()> {
    editable(db, id)?;
    db.conn().execute(
        "DELETE FROM profile_mods WHERE profile_id = ?1 AND mod_id = ?2",
        [id.0, mod_id.0],
    )?;
    Ok(())
}

pub fn set_mods(db: &Database, id: ProfileId, mods: &[ModId]) -> Result<()> {
    editable(db, id)?;
    let conn = db.conn();
    conn.execute_batch("SAVEPOINT set_mods")?;
    let result = (|| -> rusqlite::Result<()> {
        conn.execute("DELETE FROM profile_mods WHERE profile_id = ?1", [id.0])?;
        let mut insert = conn
            .prepare("INSERT OR IGNORE INTO profile_mods (profile_id, mod_id) VALUES (?1, ?2)")?;
        for m in mods {
            insert.execute([id.0, m.0])?;
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = conn.execute_batch("ROLLBACK TO set_mods");
    }
    conn.execute_batch("RELEASE set_mods")?;
    Ok(result?)
}

pub fn mod_filenames(db: &Database, id: ProfileId) -> Result<Vec<String>> {
    let mut stmt = db.conn().prepare(
        "SELECT m.filename FROM profile_mods pm JOIN mods m ON m.id = pm.mod_id
         WHERE pm.profile_id = ?1 ORDER BY m.filename",
    )?;
    let rows = stmt.query_map([id.0], |r| r.get(0))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub fn mods(db: &Database, id: ProfileId) -> Result<Vec<ModId>> {
    let mut stmt = db
        .conn()
        .prepare("SELECT mod_id FROM profile_mods WHERE profile_id = ?1 ORDER BY mod_id")?;
    let rows = stmt.query_map([id.0], |r| Ok(ModId(r.get(0)?)))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub fn vanilla(db: &Database) -> Result<ProfileId> {
    use rusqlite::OptionalExtension;
    let id = db
        .conn()
        .query_row("SELECT id FROM profiles WHERE builtin = 1", [], |r| {
            r.get(0)
        })
        .optional()?
        .ok_or(ProfileError::NotFound)?;
    Ok(ProfileId(id))
}

fn editable(db: &Database, id: ProfileId) -> Result<()> {
    match get(db, id)? {
        None => Err(ProfileError::NotFound.into()),
        Some(p) if p.builtin => Err(ProfileError::VanillaImmutable.into()),
        Some(_) => Ok(()),
    }
}

fn available_name<'n>(db: &Database, name: &'n str, except: Option<ProfileId>) -> Result<&'n str> {
    let name = name.trim();
    if name.is_empty() {
        return Err(ProfileError::EmptyName.into());
    }
    let taken: bool = db.conn().query_row(
        "SELECT EXISTS(SELECT 1 FROM profiles WHERE name = ?1 COLLATE NOCASE AND id IS NOT ?2)",
        rusqlite::params![name, except.map(|p| p.0)],
        |r| r.get(0),
    )?;
    if taken {
        return Err(ProfileError::NameTaken(name.to_owned()).into());
    }
    Ok(name)
}

pub fn active(db: &Database) -> Result<ProfileId> {
    let value = db
        .setting(setting::ACTIVE_PROFILE)?
        .ok_or(ProfileError::NoActiveProfile)?;
    let id = value.parse().map_err(|_| ProfileError::NoActiveProfile)?;
    Ok(ProfileId(id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Error;

    fn fresh() -> Database {
        let mut db = Database::open_in_memory().unwrap();
        init(&mut db).unwrap();
        db
    }

    fn names(db: &Database) -> Vec<String> {
        list(db).unwrap().into_iter().map(|p| p.name).collect()
    }

    fn id_of(db: &Database, name: &str) -> ProfileId {
        list(db)
            .unwrap()
            .into_iter()
            .find(|p| p.name == name)
            .unwrap()
            .id
    }

    #[test]
    fn first_launch_creates_vanilla_and_an_active_default() {
        let db = fresh();
        assert_eq!(names(&db), ["Vanilla", "Default"]);
        assert_eq!(active(&db).unwrap(), id_of(&db, "Default"));
    }

    fn refused(r: Result<impl std::fmt::Debug>) -> ProfileError {
        match r {
            Err(Error::Profile(e)) => e,
            other => panic!("expected a ProfileError, got {other:?}"),
        }
    }

    #[test]
    fn created_profiles_need_a_unique_non_empty_name() {
        let db = fresh();
        let coop = create(&db, "  Co-op ").unwrap();
        assert_eq!(id_of(&db, "Co-op"), coop);

        assert_eq!(
            refused(create(&db, "Co-op")),
            ProfileError::NameTaken("Co-op".into())
        );
        assert_eq!(
            refused(create(&db, "co-op")),
            ProfileError::NameTaken("co-op".into())
        );
        assert_eq!(
            refused(create(&db, "vanilla")),
            ProfileError::NameTaken("vanilla".into())
        );
        assert_eq!(refused(create(&db, "   ")), ProfileError::EmptyName);
        assert_eq!(names(&db), ["Vanilla", "Default", "Co-op"]);
    }

    #[test]
    fn renaming_keeps_names_unique_and_vanilla_fixed() {
        let db = fresh();
        let default = id_of(&db, "Default");
        create(&db, "Co-op").unwrap();

        rename(&db, default, "Solo").unwrap();
        rename(&db, default, "SOLO").unwrap();
        assert_eq!(
            refused(rename(&db, default, "co-op")),
            ProfileError::NameTaken("co-op".into())
        );
        assert_eq!(
            refused(rename(&db, vanilla(&db).unwrap(), "Plain")),
            ProfileError::VanillaImmutable
        );
        assert_eq!(
            refused(rename(&db, ProfileId(999), "Ghost")),
            ProfileError::NotFound
        );
        assert_eq!(names(&db), ["Vanilla", "SOLO", "Co-op"]);
    }

    fn install(db: &Database, filename: &str) -> ModId {
        crate::mods::insert(db, filename).unwrap()
    }

    #[test]
    fn mods_can_be_added_to_and_removed_from_a_profile_but_never_vanilla() {
        let db = fresh();
        let default = id_of(&db, "Default");
        let a = install(&db, "a.pak");
        let b = install(&db, "b.pak");

        add_mod(&db, default, b).unwrap();
        add_mod(&db, default, a).unwrap();
        add_mod(&db, default, a).unwrap();
        assert_eq!(mods(&db, default).unwrap(), [a, b]);

        remove_mod(&db, default, b).unwrap();
        remove_mod(&db, default, b).unwrap();
        assert_eq!(mods(&db, default).unwrap(), [a]);

        let vanilla = vanilla(&db).unwrap();
        assert_eq!(
            refused(add_mod(&db, vanilla, a)),
            ProfileError::VanillaImmutable
        );
        assert_eq!(
            refused(add_mod(&db, ProfileId(999), a)),
            ProfileError::NotFound
        );
        assert!(mods(&db, vanilla).unwrap().is_empty());
    }

    #[test]
    fn set_mods_replaces_a_profiles_whole_mod_list_but_never_vanillas() {
        let db = fresh();
        let default = id_of(&db, "Default");
        let a = install(&db, "a.pak");
        let b = install(&db, "b.pak");
        add_mod(&db, default, a).unwrap();

        set_mods(&db, default, &[b]).unwrap();
        assert_eq!(mod_filenames(&db, default).unwrap(), ["b.pak"]);

        set_mods(&db, default, &[]).unwrap();
        assert!(mods(&db, default).unwrap().is_empty());
        assert_eq!(
            refused(set_mods(&db, vanilla(&db).unwrap(), &[a])),
            ProfileError::VanillaImmutable
        );
    }

    #[test]
    fn profiles_can_be_looked_up_by_id_and_case_insensitively_by_name() {
        let db = fresh();
        let coop = create(&db, "Co-op").unwrap();

        assert_eq!(by_name(&db, "CO-OP").unwrap(), Some(coop));
        assert_eq!(by_name(&db, "Solo").unwrap(), None);
        assert_eq!(get(&db, coop).unwrap().unwrap().name, "Co-op");
        assert!(get(&db, vanilla(&db).unwrap()).unwrap().unwrap().builtin);
        assert_eq!(get(&db, ProfileId(999)).unwrap(), None);
    }

    #[test]
    fn deleting_refuses_vanilla_and_the_active_profile() {
        let db = fresh();
        let default = id_of(&db, "Default");
        let coop = create(&db, "Co-op").unwrap();
        let a = install(&db, "a.pak");
        add_mod(&db, coop, a).unwrap();

        delete(&db, coop).unwrap();
        assert!(mods(&db, coop).unwrap().is_empty());
        assert_eq!(refused(delete(&db, default)), ProfileError::DeleteActive);
        assert_eq!(
            refused(delete(&db, vanilla(&db).unwrap())),
            ProfileError::VanillaImmutable
        );
        assert_eq!(refused(delete(&db, coop)), ProfileError::NotFound);

        set_active(&db, vanilla(&db).unwrap()).unwrap();
        delete(&db, default).unwrap();
        assert_eq!(names(&db), ["Vanilla"]);
    }

    fn assert_one_active(db: &Database) {
        let active = active(db).unwrap();
        let hits = list(db).unwrap().iter().filter(|p| p.id == active).count();
        assert_eq!(hits, 1, "Active Profile {active:?} is not a listed Profile");
    }

    #[test]
    fn there_is_always_exactly_one_active_profile() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rom.db");
        let mut db = Database::open(&path).unwrap();
        init(&mut db).unwrap();
        assert_one_active(&db);

        let coop = create(&db, "Co-op").unwrap();
        set_active(&db, coop).unwrap();
        assert_eq!(active(&db).unwrap(), coop);
        assert_one_active(&db);

        assert_eq!(
            refused(set_active(&db, ProfileId(999))),
            ProfileError::NotFound
        );
        assert_eq!(active(&db).unwrap(), coop);

        delete(&db, id_of(&db, "Default")).unwrap();
        let _ = delete(&db, coop);
        assert_one_active(&db);
        drop(db);

        let mut db = Database::open(&path).unwrap();
        init(&mut db).unwrap();
        assert_eq!(active(&db).unwrap(), coop);
        assert_eq!(names(&db), ["Vanilla", "Co-op"]);
    }

    #[test]
    fn reset_leaves_vanilla_and_a_new_empty_active_default() {
        let mut db = fresh();
        db.conn()
            .execute("INSERT INTO mods (filename) VALUES ('a.pak')", [])
            .unwrap();
        add_mod(&db, id_of(&db, "Default"), ModId(1)).unwrap();
        let other = create(&db, "Other").unwrap();
        set_active(&db, other).unwrap();

        reset(&mut db).unwrap();

        assert_eq!(names(&db), ["Vanilla", "Default"]);
        let default = id_of(&db, "Default");
        assert_eq!(active(&db).unwrap(), default);
        assert!(mods(&db, default).unwrap().is_empty());
    }

    #[test]
    fn reset_while_vanilla_is_active_still_creates_an_active_default() {
        let mut db = fresh();
        set_active(&db, vanilla(&db).unwrap()).unwrap();

        reset(&mut db).unwrap();

        assert_eq!(names(&db), ["Vanilla", "Default"]);
        assert_eq!(active(&db).unwrap(), id_of(&db, "Default"));
    }

    #[test]
    fn duplicate_creates_a_profile_with_the_same_mods_and_leaves_the_source_alone() {
        let db = fresh();
        db.conn()
            .execute(
                "INSERT INTO mods (filename) VALUES ('a.pak'), ('b.pak')",
                [],
            )
            .unwrap();
        let default = id_of(&db, "Default");
        set_mods(&db, default, &[ModId(1), ModId(2)]).unwrap();

        let copy = duplicate(&db, default, "Copy").unwrap();

        assert_eq!(names(&db), ["Vanilla", "Default", "Copy"]);
        assert_eq!(mods(&db, copy).unwrap(), [ModId(1), ModId(2)]);
        assert_eq!(mods(&db, default).unwrap(), [ModId(1), ModId(2)]);
        assert_eq!(active(&db).unwrap(), default);
    }
}
