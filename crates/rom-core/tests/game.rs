use std::fs;
use std::path::{Path, PathBuf};

use rom_core::game::{find_game_in_steam_roots, is_game_root, linux_steam_roots, mod_folder};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/steam");

fn fixture(name: &str) -> String {
    fs::read_to_string(Path::new(FIXTURES).join(name)).unwrap()
}

fn vdf_escape(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "\\\\")
}

fn make_game_root(root: &Path) {
    fs::create_dir_all(root.join("ReadyOrNot/Content/Paks")).unwrap();
}

struct FakeSteam {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    lib2: PathBuf,
}

impl FakeSteam {
    fn new(vdf_name: &str) -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("steam");
        let lib2 = tmp.path().join("lib2");
        fs::create_dir_all(root.join("steamapps")).unwrap();
        fs::create_dir_all(lib2.join("steamapps")).unwrap();
        let vdf = fixture(vdf_name)
            .replace("@ROOT@", &vdf_escape(&root))
            .replace("@LIB2@", &vdf_escape(&lib2));
        fs::write(root.join("steamapps/libraryfolders.vdf"), vdf).unwrap();
        FakeSteam {
            _tmp: tmp,
            root,
            lib2,
        }
    }

    fn install_game(&self, library: &Path) -> PathBuf {
        fs::write(
            library.join("steamapps/appmanifest_1144200.acf"),
            fixture("appmanifest_1144200.acf"),
        )
        .unwrap();
        let game = library.join("steamapps/common/Ready Or Not");
        make_game_root(&game);
        game
    }
}

#[test]
fn finds_game_in_a_secondary_steam_library() {
    let steam = FakeSteam::new("libraryfolders.vdf");
    let game = steam.install_game(&steam.lib2);

    assert_eq!(
        find_game_in_steam_roots(std::slice::from_ref(&steam.root)),
        Some(game)
    );
}

#[test]
fn finds_game_in_the_steam_root_library() {
    let steam = FakeSteam::new("libraryfolders.vdf");
    let game = steam.install_game(&steam.root);

    assert_eq!(
        find_game_in_steam_roots(std::slice::from_ref(&steam.root)),
        Some(game)
    );
}

#[test]
fn finds_game_via_legacy_libraryfolders_format() {
    let steam = FakeSteam::new("libraryfolders_legacy.vdf");
    let game = steam.install_game(&steam.lib2);

    assert_eq!(
        find_game_in_steam_roots(std::slice::from_ref(&steam.root)),
        Some(game)
    );
}

#[test]
fn finds_game_in_steam_root_even_without_libraryfolders_file() {
    let steam = FakeSteam::new("libraryfolders.vdf");
    fs::remove_file(steam.root.join("steamapps/libraryfolders.vdf")).unwrap();
    let game = steam.install_game(&steam.root);

    assert_eq!(
        find_game_in_steam_roots(std::slice::from_ref(&steam.root)),
        Some(game)
    );
}

#[test]
fn searches_every_steam_root_in_order() {
    let empty = FakeSteam::new("libraryfolders.vdf");
    let steam = FakeSteam::new("libraryfolders.vdf");
    let game = steam.install_game(&steam.lib2);

    let roots = [empty.root.clone(), steam.root.clone()];
    assert_eq!(find_game_in_steam_roots(&roots), Some(game));
}

fn install_steam_under_home(home: &Path, relative: &str) -> PathBuf {
    let root = home.join(relative);
    fs::create_dir_all(root.join("steamapps")).unwrap();
    fs::write(
        root.join("steamapps/appmanifest_1144200.acf"),
        fixture("appmanifest_1144200.acf"),
    )
    .unwrap();
    let game = root.join("steamapps/common/Ready Or Not");
    make_game_root(&game);
    game
}

#[test]
fn linux_roots_cover_native_steam() {
    let home = tempfile::tempdir().unwrap();
    let game = install_steam_under_home(home.path(), ".local/share/Steam");

    assert_eq!(
        find_game_in_steam_roots(&linux_steam_roots(home.path())),
        Some(game)
    );
}

#[test]
fn linux_roots_cover_the_dot_steam_symlink_location() {
    let home = tempfile::tempdir().unwrap();
    let game = install_steam_under_home(home.path(), ".steam/steam");

    assert_eq!(
        find_game_in_steam_roots(&linux_steam_roots(home.path())),
        Some(game)
    );
}

#[test]
fn linux_roots_cover_flatpak_steam() {
    let home = tempfile::tempdir().unwrap();
    let game = install_steam_under_home(
        home.path(),
        ".var/app/com.valvesoftware.Steam/.local/share/Steam",
    );

    assert_eq!(
        find_game_in_steam_roots(&linux_steam_roots(home.path())),
        Some(game)
    );
}

#[test]
fn linux_roots_find_nothing_in_an_empty_home() {
    let home = tempfile::tempdir().unwrap();
    assert_eq!(
        find_game_in_steam_roots(&linux_steam_roots(home.path())),
        None
    );
}

#[test]
fn manual_path_validation_accepts_the_game_root() {
    let tmp = tempfile::tempdir().unwrap();
    let game = tmp.path().join("Ready Or Not");
    make_game_root(&game);

    assert!(is_game_root(&game));
}

#[test]
fn manual_path_validation_rejects_unrelated_folders() {
    let tmp = tempfile::tempdir().unwrap();
    let game = tmp.path().join("Ready Or Not");
    make_game_root(&game);

    assert!(!is_game_root(tmp.path()), "parent of the game root");
    assert!(
        !is_game_root(&game.join("ReadyOrNot")),
        "inside the game root"
    );
    assert!(
        !is_game_root(&tmp.path().join("missing")),
        "nonexistent path"
    );

    let half = tmp.path().join("half");
    fs::create_dir_all(half.join("ReadyOrNot")).unwrap();
    assert!(!is_game_root(&half), "ReadyOrNot without Content/Paks");
}

#[test]
fn mod_folder_is_rom_inside_the_games_paks_folder() {
    let root = Path::new("/games/Ready Or Not");
    assert_eq!(
        mod_folder(root),
        Path::new("/games/Ready Or Not/ReadyOrNot/Content/Paks/RoM")
    );
}

#[test]
fn returns_none_when_no_steam_root_exists() {
    let tmp = tempfile::tempdir().unwrap();
    assert_eq!(find_game_in_steam_roots(&[tmp.path().join("nope")]), None);
    assert_eq!(find_game_in_steam_roots(&[]), None);
}

#[test]
fn returns_none_when_game_is_not_installed_in_any_library() {
    let steam = FakeSteam::new("libraryfolders.vdf");
    assert_eq!(
        find_game_in_steam_roots(std::slice::from_ref(&steam.root)),
        None
    );
}

#[test]
fn ignores_a_stale_manifest_whose_install_folder_is_gone() {
    let steam = FakeSteam::new("libraryfolders.vdf");
    let game = steam.install_game(&steam.lib2);
    fs::remove_dir_all(&game).unwrap();
    assert_eq!(
        find_game_in_steam_roots(std::slice::from_ref(&steam.root)),
        None
    );
}

#[test]
fn returns_none_for_a_corrupt_libraryfolders_file() {
    let steam = FakeSteam::new("libraryfolders.vdf");
    fs::write(
        steam.root.join("steamapps/libraryfolders.vdf"),
        "\"libraryfolders\" { \"0\" {",
    )
    .unwrap();
    assert_eq!(
        find_game_in_steam_roots(std::slice::from_ref(&steam.root)),
        None
    );
}
