use std::path::{Path, PathBuf};

use rusqlite::Connection;

use crate::{Error, Result};

const MIGRATIONS: &[&str] = &["
    CREATE TABLE mods (
        id       INTEGER PRIMARY KEY,
        filename TEXT NOT NULL UNIQUE
    );
    CREATE TABLE profiles (
        id      INTEGER PRIMARY KEY,
        name    TEXT NOT NULL UNIQUE,
        builtin INTEGER NOT NULL DEFAULT 0
    );
    CREATE TABLE profile_mods (
        profile_id INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
        mod_id     INTEGER NOT NULL REFERENCES mods(id) ON DELETE CASCADE,
        PRIMARY KEY (profile_id, mod_id)
    );
    CREATE TABLE settings (
        key   TEXT PRIMARY KEY,
        value TEXT NOT NULL
    );
    "];

pub const LATEST_VERSION: u32 = MIGRATIONS.len() as u32;

pub mod setting {
    pub const GAME_PATH: &str = "game_path";
    pub const ACTIVE_PROFILE: &str = "active_profile";
    pub const AUTO_ENABLE: &str = "auto_enable";
    pub const UPDATE_CHECK: &str = "update_check";
    pub const V1_MIGRATION: &str = "v1_migration";
}

pub struct Database {
    conn: Connection,
}

impl Database {
    pub fn default_path() -> Result<PathBuf> {
        Ok(Self::default_dir()?.join("rom.db"))
    }

    pub fn default_dir() -> Result<PathBuf> {
        let dir = dirs::config_dir().ok_or(Error::NoConfigDir)?;
        Ok(dir.join("ReadyOrModManager"))
    }

    pub fn open_default() -> Result<Self> {
        Self::open(&Self::default_path()?)
    }

    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        Self::init(Connection::open(path)?)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "foreign_keys", true)?;
        let mut db = Self { conn };
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&mut self) -> Result<()> {
        let current = self.schema_version()?;
        if current > LATEST_VERSION {
            return Err(Error::SchemaTooNew {
                found: current,
                supported: LATEST_VERSION,
            });
        }
        for (i, sql) in MIGRATIONS.iter().enumerate().skip(current as usize) {
            let tx = self.conn.transaction()?;
            tx.execute_batch(sql)?;
            tx.pragma_update(None, "user_version", i as u32 + 1)?;
            tx.commit()?;
        }
        Ok(())
    }

    pub fn schema_version(&self) -> Result<u32> {
        Ok(self
            .conn
            .pragma_query_value(None, "user_version", |row| row.get(0))?)
    }

    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    pub fn conn_mut(&mut self) -> &mut Connection {
        &mut self.conn
    }

    pub fn setting(&self, key: &str) -> Result<Option<String>> {
        use rusqlite::OptionalExtension;
        Ok(self
            .conn
            .query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| {
                row.get(0)
            })
            .optional()?)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [key, value],
        )?;
        Ok(())
    }
}

pub fn delete_app_data(dir: &Path) -> Result<()> {
    match std::fs::remove_dir_all(dir) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_database_is_created_and_migrated_to_latest() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("rom.db");

        let db = Database::open(&path).unwrap();

        assert!(path.exists());
        assert_eq!(db.schema_version().unwrap(), 1);
    }

    #[test]
    fn reopening_an_up_to_date_database_keeps_data_and_version() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rom.db");
        {
            let db = Database::open(&path).unwrap();
            db.set_setting(setting::GAME_PATH, "/games/ron").unwrap();
        }

        let db = Database::open(&path).unwrap();

        assert_eq!(db.schema_version().unwrap(), 1);
        assert_eq!(
            db.setting(setting::GAME_PATH).unwrap().as_deref(),
            Some("/games/ron")
        );
    }

    #[test]
    fn unset_setting_is_none_and_set_overwrites() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(db.setting(setting::AUTO_ENABLE).unwrap(), None);

        db.set_setting(setting::AUTO_ENABLE, "true").unwrap();
        db.set_setting(setting::AUTO_ENABLE, "false").unwrap();

        assert_eq!(
            db.setting(setting::AUTO_ENABLE).unwrap().as_deref(),
            Some("false")
        );
    }

    #[test]
    fn deleting_a_mod_removes_its_profile_memberships() {
        let db = Database::open_in_memory().unwrap();
        let c = db.conn();
        c.execute_batch(
            "INSERT INTO mods (id, filename) VALUES (1, 'a.pak');
             INSERT INTO profiles (id, name) VALUES (1, 'Default');
             INSERT INTO profile_mods (profile_id, mod_id) VALUES (1, 1);
             DELETE FROM mods WHERE id = 1;",
        )
        .unwrap();

        let members: i64 = c
            .query_row("SELECT COUNT(*) FROM profile_mods", [], |r| r.get(0))
            .unwrap();
        assert_eq!(members, 0);
    }

    #[test]
    fn database_from_a_newer_app_version_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rom.db");
        Connection::open(&path)
            .unwrap()
            .pragma_update(None, "user_version", 99)
            .unwrap();

        let err = Database::open(&path).err().unwrap();

        assert!(matches!(
            err,
            Error::SchemaTooNew {
                found: 99,
                supported: 1
            }
        ));
    }

    #[test]
    fn deleting_app_data_removes_the_folder_and_tolerates_a_missing_one() {
        let parent = tempfile::tempdir().unwrap();
        let dir = parent.path().join("ReadyOrModManager");
        drop(Database::open(&dir.join("rom.db")).unwrap());

        delete_app_data(&dir).unwrap();
        assert!(!dir.exists());
        assert!(parent.path().exists());

        delete_app_data(&dir).unwrap();
    }
}
