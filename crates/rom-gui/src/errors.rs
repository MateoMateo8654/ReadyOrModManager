use rom_core::Error;
use rom_core::import::ImportError;
use rom_core::install::InstallError;
use rom_core::mods::ModError;
use rom_core::profiles::ProfileError;
use rust_i18n::t;

pub fn message(err: &Error) -> String {
    match err {
        Error::Io(e) => t!("error.io", detail = e),
        Error::Database(e) => t!("error.database", detail = e),
        Error::SchemaTooNew { .. } => t!("error.schema_too_new"),
        Error::NoConfigDir => t!("error.no_config_dir"),
        Error::NonUtf8Path(path) => t!("error.non_utf8_path", path = path.display()),
        Error::ModConflict { filenames } => {
            t!("error.mod_conflict", filenames = filenames.join(", "))
        }
        Error::Mod(ModError::Missing { filename }) => t!("error.mod_missing", filename = filename),
        Error::Mod(ModError::Locked { filename, .. }) => {
            t!("error.mod_locked", filename = filename)
        }
        Error::Mod(ModError::PermissionDenied { filename, .. }) => {
            t!("error.mod_permission", filename = filename)
        }
        Error::Profile(e) => match e {
            ProfileError::VanillaImmutable => t!("error.vanilla_immutable"),
            ProfileError::NameTaken(name) => t!("error.name_taken", name = name),
            ProfileError::EmptyName => t!("error.empty_name"),
            ProfileError::NotFound => t!("error.profile_not_found"),
            ProfileError::DeleteActive => t!("error.delete_active"),
            ProfileError::NoActiveProfile => t!("error.no_active_profile"),
        },
        Error::Launch { source, .. } => t!("error.launch", detail = source),
        Error::Install(e) => match e {
            InstallError::Archive { archive, .. } => {
                t!("error.archive", archive = archive.display())
            }
            InstallError::MissingEntry { archive, entry } => {
                t!(
                    "error.missing_entry",
                    archive = archive.display(),
                    entry = entry
                )
            }
            InstallError::UnresolvedCollision(filename) => {
                t!("error.unresolved_collision", filename = filename)
            }
            InstallError::UnsafeFilename(filename) => {
                t!("error.unsafe_filename", filename = filename)
            }
        },
        Error::Import(ImportError::UnresolvedClash(name)) => {
            t!("error.unresolved_clash", name = name)
        }
    }
    .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_locked_mod_asks_to_close_the_game() {
        let err = Error::Mod(ModError::Locked {
            filename: "a.pak".into(),
            source: std::io::Error::other("sharing violation"),
        });

        assert_eq!(
            message(&err),
            "\u{201c}a.pak\u{201d} is in use. Close Ready or Not and try again."
        );
    }

    #[test]
    fn every_error_has_its_own_translated_message() {
        let io = || std::io::Error::other("boom");
        let errors = vec![
            Error::Io(io()),
            Error::Database(rusqlite::Error::QueryReturnedNoRows),
            Error::SchemaTooNew {
                found: 9,
                supported: 1,
            },
            Error::NoConfigDir,
            Error::NonUtf8Path("x".into()),
            Error::ModConflict {
                filenames: vec!["a.pak".into()],
            },
            Error::Mod(ModError::Missing {
                filename: "a.pak".into(),
            }),
            Error::Mod(ModError::Locked {
                filename: "a.pak".into(),
                source: io(),
            }),
            Error::Mod(ModError::PermissionDenied {
                filename: "a.pak".into(),
                source: io(),
            }),
            Error::Profile(ProfileError::VanillaImmutable),
            Error::Profile(ProfileError::NameTaken("P".into())),
            Error::Profile(ProfileError::EmptyName),
            Error::Profile(ProfileError::NotFound),
            Error::Profile(ProfileError::DeleteActive),
            Error::Profile(ProfileError::NoActiveProfile),
            Error::Launch {
                url: "steam://x".into(),
                source: io(),
            },
            Error::Install(InstallError::Archive {
                archive: "a.zip".into(),
                message: "bad".into(),
            }),
            Error::Install(InstallError::MissingEntry {
                archive: "a.zip".into(),
                entry: "e".into(),
            }),
            Error::Install(InstallError::UnresolvedCollision("a.pak".into())),
            Error::Install(InstallError::UnsafeFilename("CON".into())),
            Error::Import(ImportError::UnresolvedClash("P".into())),
        ];

        for err in &errors {
            let msg = message(err);
            assert!(
                !msg.starts_with("Something went wrong"),
                "{err:?} has no message"
            );
            assert!(
                !msg.starts_with("error."),
                "{err:?} uses a missing key: {msg}"
            );
        }
    }
}
