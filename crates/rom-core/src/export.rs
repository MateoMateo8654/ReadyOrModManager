use std::collections::HashSet;
use std::fs::File;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use zip::ZipWriter;
use zip::write::SimpleFileOptions;

use crate::Result;
use crate::db::Database;
use crate::mods;
use crate::profiles::{self, Profile, ProfileError, ProfileId};

pub fn export_all(db: &Database, mod_folder: &Path, dest: &Path) -> Result<()> {
    let mods = mods::filenames(db)?;
    let mut stems = HashSet::new();
    let profiles = profiles::list(db)?
        .into_iter()
        .filter(|p| !p.builtin)
        .map(|p| {
            let stem = profiles::unique_name(&file_stem(&p.name), &mut stems);
            Ok((stem, profile_json(db, &p)?))
        })
        .collect::<Result<Vec<_>>>()?;

    write_zip(dest, |zw| {
        for filename in &mods {
            add_mod(zw, mod_folder, filename, &format!("mods/{filename}"))?;
        }
        for (stem, json) in &profiles {
            add_bytes(zw, &format!("profiles/{stem}.json"), json)?;
        }
        Ok(())
    })
}

pub fn export_profile(
    db: &Database,
    mod_folder: &Path,
    profile: ProfileId,
    dest: &Path,
) -> Result<()> {
    let profile = profiles::get(db, profile)?.ok_or(ProfileError::NotFound)?;
    let mods = profiles::mod_filenames(db, profile.id)?;
    let json = profile_json(db, &profile)?;

    write_zip(dest, |zw| {
        for filename in &mods {
            add_mod(zw, mod_folder, filename, filename)?;
        }
        add_bytes(zw, "profile.json", &json)
    })
}

fn profile_json(db: &Database, profile: &Profile) -> Result<Vec<u8>> {
    let json = serde_json::json!({
        "name": profile.name,
        "mods": profiles::mod_filenames(db, profile.id)?,
    });
    Ok(serde_json::to_vec_pretty(&json).map_err(io::Error::from)?)
}

fn file_stem(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect()
}

fn mod_path(mod_folder: &Path, filename: &str) -> PathBuf {
    let enabled = mods::enabled_path(mod_folder, filename);
    if enabled.is_file() {
        enabled
    } else {
        mods::disabled_path(mod_folder, filename)
    }
}

fn write_zip(dest: &Path, fill: impl FnOnce(&mut ZipWriter<File>) -> io::Result<()>) -> Result<()> {
    let result = (|| -> io::Result<()> {
        let mut zw = ZipWriter::new(File::create(dest)?);
        fill(&mut zw)?;
        zw.finish()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(dest);
    }
    Ok(result?)
}

fn add_mod(
    zw: &mut ZipWriter<File>,
    mod_folder: &Path,
    filename: &str,
    entry: &str,
) -> io::Result<()> {
    let mut src = File::open(mod_path(mod_folder, filename))?;
    let opts = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Stored)
        .large_file(true);
    zw.start_file(entry, opts)?;
    io::copy(&mut src, zw)?;
    Ok(())
}

fn add_bytes(zw: &mut ZipWriter<File>, entry: &str, bytes: &[u8]) -> io::Result<()> {
    let opts = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .large_file(true);
    zw.start_file(entry, opts)?;
    zw.write_all(bytes)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::io::Read;
    use std::path::PathBuf;

    use serde_json::{Value, json};

    use super::*;
    use crate::profiles;

    struct Setup {
        dir: tempfile::TempDir,
        db: Database,
        mod_folder: PathBuf,
    }

    impl Setup {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let mod_folder = dir.path().join("RoM");
            std::fs::create_dir_all(&mod_folder).unwrap();
            let mut db = Database::open_in_memory().unwrap();
            profiles::init(&mut db).unwrap();
            Self {
                dir,
                db,
                mod_folder,
            }
        }

        fn mod_file(&mut self, on_disk: &str, contents: &[u8]) {
            std::fs::write(self.mod_folder.join(on_disk), contents).unwrap();
            let filename = on_disk
                .strip_suffix(mods::DISABLED_SUFFIX)
                .unwrap_or(on_disk);
            mods::insert(&self.db, filename).unwrap();
        }

        fn mod_id(&self, filename: &str) -> mods::ModId {
            mods::id_of(&self.db, filename).unwrap().unwrap()
        }

        fn profile(&self, name: &str) -> ProfileId {
            profiles::list(&self.db)
                .unwrap()
                .into_iter()
                .find(|p| p.name == name)
                .unwrap()
                .id
        }

        fn dest(&self) -> PathBuf {
            self.dir.path().join("export.zip")
        }
    }

    fn read_zip(path: &Path) -> BTreeMap<String, Vec<u8>> {
        let mut za = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
        let mut out = BTreeMap::new();
        for i in 0..za.len() {
            let mut f = za.by_index(i).unwrap();
            if f.is_dir() {
                continue;
            }
            let name = f.name().unwrap().into_owned();
            let mut buf = Vec::new();
            f.read_to_end(&mut buf).unwrap();
            out.insert(name, buf);
        }
        out
    }

    fn json_of(bytes: &[u8]) -> Value {
        serde_json::from_slice(bytes).unwrap()
    }

    #[test]
    fn export_all_writes_every_mod_and_every_profile_but_vanilla() {
        let mut s = Setup::new();
        s.mod_file("a.pak", b"AAA");
        s.mod_file("b.pak.disabled", b"BBB");
        let default = s.profile("Default");
        profiles::add_mod(&s.db, default, s.mod_id("a.pak")).unwrap();
        let coop = profiles::create(&s.db, "Co-op").unwrap();
        profiles::add_mod(&s.db, coop, s.mod_id("b.pak")).unwrap();
        profiles::add_mod(&s.db, coop, s.mod_id("a.pak")).unwrap();

        export_all(&s.db, &s.mod_folder, &s.dest()).unwrap();

        let zip = read_zip(&s.dest());
        assert_eq!(
            zip.keys().collect::<Vec<_>>(),
            [
                "mods/a.pak",
                "mods/b.pak",
                "profiles/Co-op.json",
                "profiles/Default.json"
            ]
        );
        assert_eq!(zip["mods/a.pak"], b"AAA");
        assert_eq!(zip["mods/b.pak"], b"BBB");
        assert_eq!(
            json_of(&zip["profiles/Default.json"]),
            json!({"name": "Default", "mods": ["a.pak"]})
        );
        assert_eq!(
            json_of(&zip["profiles/Co-op.json"]),
            json!({"name": "Co-op", "mods": ["a.pak", "b.pak"]})
        );
    }

    #[test]
    fn export_profile_writes_only_its_mods_flat_with_profile_json() {
        let mut s = Setup::new();
        s.mod_file("a.pak", b"AAA");
        s.mod_file("b.pak.disabled", b"BBB");
        s.mod_file("c.utoc", b"CCC");
        let coop = profiles::create(&s.db, "Co-op").unwrap();
        profiles::add_mod(&s.db, coop, s.mod_id("c.utoc")).unwrap();
        profiles::add_mod(&s.db, coop, s.mod_id("b.pak")).unwrap();

        export_profile(&s.db, &s.mod_folder, coop, &s.dest()).unwrap();

        let zip = read_zip(&s.dest());
        assert_eq!(
            zip.keys().collect::<Vec<_>>(),
            ["b.pak", "c.utoc", "profile.json"]
        );
        assert_eq!(zip["b.pak"], b"BBB");
        assert_eq!(zip["c.utoc"], b"CCC");
        assert_eq!(
            json_of(&zip["profile.json"]),
            json!({"name": "Co-op", "mods": ["b.pak", "c.utoc"]})
        );
    }

    #[test]
    fn profile_names_that_are_not_valid_file_names_keep_their_name_in_the_json() {
        let s = Setup::new();
        profiles::create(&s.db, "PvP: 1/2").unwrap();

        export_all(&s.db, &s.mod_folder, &s.dest()).unwrap();

        let zip = read_zip(&s.dest());
        assert_eq!(
            zip.keys().collect::<Vec<_>>(),
            ["profiles/Default.json", "profiles/PvP_ 1_2.json"]
        );
        assert_eq!(
            json_of(&zip["profiles/PvP_ 1_2.json"]),
            json!({"name": "PvP: 1/2", "mods": []})
        );
    }

    #[test]
    fn profiles_whose_file_names_clash_are_all_exported() {
        let s = Setup::new();
        profiles::create(&s.db, "a/b").unwrap();
        profiles::create(&s.db, "a:b").unwrap();
        profiles::create(&s.db, "A_B").unwrap();

        export_all(&s.db, &s.mod_folder, &s.dest()).unwrap();

        let zip = read_zip(&s.dest());
        let mut names: Vec<String> = zip
            .values()
            .map(|json| json_of(json)["name"].as_str().unwrap().to_owned())
            .collect();
        names.sort();
        assert_eq!(names, ["A_B", "Default", "a/b", "a:b"]);
    }

    #[test]
    fn exporting_an_unknown_profile_is_refused() {
        let s = Setup::new();

        let err = export_profile(&s.db, &s.mod_folder, ProfileId(999), &s.dest()).unwrap_err();

        assert!(
            matches!(err, crate::Error::Profile(ProfileError::NotFound)),
            "{err:?}"
        );
        assert!(!s.dest().exists());
    }

    #[test]
    fn a_missing_mod_file_fails_the_export_and_leaves_no_zip() {
        let mut s = Setup::new();
        s.mod_file("a.pak", b"AAA");
        std::fs::remove_file(s.mod_folder.join("a.pak")).unwrap();

        let err = export_all(&s.db, &s.mod_folder, &s.dest()).unwrap_err();

        assert!(
            matches!(&err, crate::Error::Io(e) if e.kind() == std::io::ErrorKind::NotFound),
            "{err:?}"
        );
        assert!(!s.dest().exists());
    }
}
