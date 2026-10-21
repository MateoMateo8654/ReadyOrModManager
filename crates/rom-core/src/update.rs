use std::time::Duration;

use semver::Version;

use crate::Result;
use crate::db::{Database, setting};

pub const GITHUB_REPO: &str = "MateoMateo8654/ReadyOrModManager";

pub const VERSION_URL: &str = "https://raw.githubusercontent.com/MateoMateo8654/ReadyOrModManager/refs/heads/main/version.txt";

pub const RELEASES_URL: &str =
    "https://github.com/MateoMateo8654/ReadyOrModManager/releases/latest";

pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateCheck {
    Unasked,
    Enabled,
    Disabled,
}

impl UpdateCheck {
    pub fn load(db: &Database) -> Result<Self> {
        Ok(match db.setting(setting::UPDATE_CHECK)?.as_deref() {
            Some("true") => Self::Enabled,
            Some("false") => Self::Disabled,
            _ => Self::Unasked,
        })
    }

    pub fn save(self, db: &Database) -> Result<()> {
        match self {
            Self::Unasked => Ok(()),
            Self::Enabled => db.set_setting(setting::UPDATE_CHECK, "true"),
            Self::Disabled => db.set_setting(setting::UPDATE_CHECK, "false"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AvailableUpdate {
    pub version: String,
    pub url: String,
}

pub trait VersionFetcher {
    fn fetch_latest_version(&self) -> Option<String>;
}

pub fn check_for_update(
    db: &Database,
    current: &str,
    fetcher: &impl VersionFetcher,
) -> Result<Option<AvailableUpdate>> {
    if UpdateCheck::load(db)? != UpdateCheck::Enabled {
        return Ok(None);
    }
    let Some(body) = fetcher.fetch_latest_version() else {
        return Ok(None);
    };
    let latest = body.trim();
    if !is_newer(current, latest) {
        return Ok(None);
    }
    Ok(Some(AvailableUpdate {
        version: latest.to_owned(),
        url: RELEASES_URL.to_owned(),
    }))
}

#[derive(Debug, Default, Clone, Copy)]
pub struct GithubFetcher;

impl VersionFetcher for GithubFetcher {
    fn fetch_latest_version(&self) -> Option<String> {
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(10)))
            .build();
        let agent: ureq::Agent = config.into();
        agent
            .get(VERSION_URL)
            .header(
                "User-Agent",
                concat!("ReadyOrModManager/", env!("CARGO_PKG_VERSION")),
            )
            .call()
            .ok()?
            .into_body()
            .read_to_string()
            .ok()
    }
}

pub fn is_newer(current: &str, latest: &str) -> bool {
    match (parse_version(current), parse_version(latest)) {
        (Some(current), Some(latest)) => latest.cmp_precedence(&current).is_gt(),
        _ => false,
    }
}

fn parse_version(s: &str) -> Option<Version> {
    let s = s.trim();
    let s = s.strip_prefix(['v', 'V']).unwrap_or(s);
    let split = s.find(['-', '+']).unwrap_or(s.len());
    let (core, rest) = s.split_at(split);
    let missing = 2usize.checked_sub(core.matches('.').count())?;
    Version::parse(&format!("{core}{}{rest}", ".0".repeat(missing))).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn newer_release_is_detected() {
        assert!(is_newer("0.1.0", "v0.2.0"));
        assert!(is_newer("0.1.0", "0.1.1"));
        assert!(!is_newer("0.2.0", "v0.1.9"));
        assert!(!is_newer("0.2.0", "v0.2.0"));
    }

    #[test]
    fn pre_releases_order_below_their_release() {
        assert!(is_newer("0.2.0-beta.1", "v0.2.0"));
        assert!(is_newer("0.2.0-alpha", "v0.2.0-beta"));
        assert!(is_newer("0.2.0-beta.2", "v0.2.0-beta.10"));
        assert!(!is_newer("0.2.0", "v0.2.0-rc.1"));
        assert!(is_newer("0.1.0", "v0.2.0-rc.1"));
    }

    #[test]
    fn opt_in_is_unasked_until_answered_and_then_remembered() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(UpdateCheck::load(&db).unwrap(), UpdateCheck::Unasked);

        UpdateCheck::Enabled.save(&db).unwrap();
        assert_eq!(UpdateCheck::load(&db).unwrap(), UpdateCheck::Enabled);

        UpdateCheck::Disabled.save(&db).unwrap();
        assert_eq!(UpdateCheck::load(&db).unwrap(), UpdateCheck::Disabled);
    }

    struct FakeFetcher {
        body: Option<&'static str>,
        calls: Cell<u32>,
    }

    impl FakeFetcher {
        fn new(body: Option<&'static str>) -> Self {
            Self {
                body,
                calls: Cell::new(0),
            }
        }
    }

    impl VersionFetcher for FakeFetcher {
        fn fetch_latest_version(&self) -> Option<String> {
            self.calls.set(self.calls.get() + 1);
            self.body.map(str::to_owned)
        }
    }

    #[test]
    fn no_request_before_first_launch_answer_or_when_disabled() {
        let db = Database::open_in_memory().unwrap();
        let fetcher = FakeFetcher::new(Some("2.1"));

        assert_eq!(check_for_update(&db, "2.0.0", &fetcher).unwrap(), None);
        UpdateCheck::Disabled.save(&db).unwrap();
        assert_eq!(check_for_update(&db, "2.0.0", &fetcher).unwrap(), None);

        assert_eq!(fetcher.calls.get(), 0);
    }

    #[test]
    fn enabled_check_reports_a_newer_version_file_with_the_releases_link() {
        let db = Database::open_in_memory().unwrap();
        UpdateCheck::Enabled.save(&db).unwrap();
        let fetcher = FakeFetcher::new(Some("2.1\n"));

        assert_eq!(
            check_for_update(&db, "2.0.0", &fetcher).unwrap(),
            Some(AvailableUpdate {
                version: "2.1".into(),
                url: "https://github.com/MateoMateo8654/ReadyOrModManager/releases/latest".into(),
            })
        );
        assert_eq!(fetcher.calls.get(), 1);
    }

    #[test]
    fn enabled_check_is_silent_when_up_to_date() {
        let db = Database::open_in_memory().unwrap();
        UpdateCheck::Enabled.save(&db).unwrap();

        let fetcher = FakeFetcher::new(Some("2.0"));
        assert_eq!(check_for_update(&db, "2.0.0", &fetcher).unwrap(), None);
    }

    #[test]
    fn network_and_parse_failures_are_silent() {
        let db = Database::open_in_memory().unwrap();
        UpdateCheck::Enabled.save(&db).unwrap();

        for body in [None, Some(""), Some("404: Not Found"), Some("<html>")] {
            let fetcher = FakeFetcher::new(body);
            assert_eq!(check_for_update(&db, "2.0.0", &fetcher).unwrap(), None);
            assert_eq!(fetcher.calls.get(), 1);
        }
    }

    #[test]
    fn two_part_versions_count_as_patch_zero() {
        assert!(is_newer("2.0.0", "2.1"));
        assert!(is_newer("2.0", "2.0.1"));
        assert!(!is_newer("2.0.0", "v2.0"));
        assert!(is_newer("2.0.0", "3"));
    }

    #[test]
    fn build_metadata_does_not_make_a_release_newer() {
        assert!(!is_newer("0.2.0", "v0.2.0+build.5"));
        assert!(!is_newer("0.2.0+a", "v0.2.0+b"));
    }

    #[test]
    fn unparseable_versions_are_never_newer() {
        assert!(!is_newer("0.1.0", "nightly"));
        assert!(!is_newer("garbage", "v9.9.9"));
    }
}
