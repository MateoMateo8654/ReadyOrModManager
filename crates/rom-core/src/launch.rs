use crate::{Error, Result};

pub const STEAM_LAUNCH_URL: &str = "steam://rungameid/1144200";

pub trait UrlOpener {
    fn open(&self, url: &str) -> std::io::Result<()>;
}

impl<F: Fn(&str) -> std::io::Result<()>> UrlOpener for F {
    fn open(&self, url: &str) -> std::io::Result<()> {
        self(url)
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SystemOpener;

impl UrlOpener for SystemOpener {
    fn open(&self, url: &str) -> std::io::Result<()> {
        open::that(url)
    }
}

pub fn launch_game() -> Result<()> {
    launch_game_with(&SystemOpener)
}

pub fn launch_game_with(opener: &impl UrlOpener) -> Result<()> {
    opener
        .open(STEAM_LAUNCH_URL)
        .map_err(|source| Error::Launch {
            url: STEAM_LAUNCH_URL.to_owned(),
            source,
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn launch_opens_steam_rungameid_url() {
        let opened = RefCell::new(Vec::new());
        let opener = |url: &str| {
            opened.borrow_mut().push(url.to_owned());
            Ok(())
        };
        launch_game_with(&opener).unwrap();
        assert_eq!(
            *opened.borrow(),
            vec!["steam://rungameid/1144200".to_owned()]
        );
    }

    #[test]
    fn opener_failure_returns_launch_error() {
        let opener = |_: &str| {
            Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "no handler",
            ))
        };
        let err = launch_game_with(&opener).unwrap_err();
        assert!(
            matches!(&err, Error::Launch { url, source }
                if url == "steam://rungameid/1144200" && source.kind() == std::io::ErrorKind::NotFound),
            "unexpected error: {err:?}"
        );
    }
}
