use std::path::{Path, PathBuf};

use crate::db::{Database, setting};
use crate::{Error, Result};

pub fn auto_enable(db: &Database) -> Result<bool> {
    Ok(db.setting(setting::AUTO_ENABLE)?.as_deref() != Some("false"))
}

pub fn set_auto_enable(db: &Database, on: bool) -> Result<()> {
    db.set_setting(setting::AUTO_ENABLE, if on { "true" } else { "false" })
}

pub fn game_path(db: &Database) -> Result<Option<PathBuf>> {
    Ok(db.setting(setting::GAME_PATH)?.map(PathBuf::from))
}

pub fn set_game_path(db: &Database, path: &Path) -> Result<()> {
    let text = path
        .to_str()
        .ok_or_else(|| Error::NonUtf8Path(path.to_owned()))?;
    db.set_setting(setting::GAME_PATH, text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use std::path::Path;

    #[test]
    fn auto_enable_is_on_until_turned_off() {
        let db = Database::open_in_memory().unwrap();
        assert!(auto_enable(&db).unwrap());

        set_auto_enable(&db, false).unwrap();
        assert!(!auto_enable(&db).unwrap());

        set_auto_enable(&db, true).unwrap();
        assert!(auto_enable(&db).unwrap());
    }

    #[test]
    fn the_chosen_game_path_is_unset_until_chosen_and_survives_a_relaunch() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("rom.db");
        let game = Path::new("/games/Ready Or Not");
        {
            let db = Database::open(&db_path).unwrap();
            assert_eq!(game_path(&db).unwrap(), None);
            set_game_path(&db, game).unwrap();
        }

        let db = Database::open(&db_path).unwrap();

        assert_eq!(game_path(&db).unwrap().as_deref(), Some(game));
    }
}
