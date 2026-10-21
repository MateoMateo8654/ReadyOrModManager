#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("database schema version {found} is newer than supported version {supported}")]
    SchemaTooNew { found: u32, supported: u32 },
    #[error("could not determine the OS config directory")]
    NoConfigDir,
    #[error("path {} is not valid UTF-8", .0.display())]
    NonUtf8Path(std::path::PathBuf),
    #[error("Mods present both Enabled and Disabled (X and X.disabled): {}", filenames.join(", "))]
    ModConflict { filenames: Vec<String> },

    #[error(transparent)]
    Mod(#[from] crate::mods::ModError),

    #[error(transparent)]
    Profile(#[from] crate::profiles::ProfileError),

    #[error("could not open {url}: {source}")]
    Launch {
        url: String,
        #[source]
        source: std::io::Error,
    },

    #[error(transparent)]
    Install(#[from] crate::install::InstallError),

    #[error(transparent)]
    Import(#[from] crate::import::ImportError),
}

pub type Result<T> = std::result::Result<T, Error>;
