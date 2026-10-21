#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

rust_i18n::i18n!("locales", fallback = "en");

mod app;
mod dialogs;
mod errors;
mod review;
mod screens;
mod view;
mod widgets;
mod worker;

use rust_i18n::t;

fn main() -> eframe::Result {
    let title = t!("app.title").to_string();
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title(&title)
            .with_app_id("readyormodmanager")
            .with_inner_size([900.0, 640.0])
            .with_min_inner_size([560.0, 400.0])
            .with_drag_and_drop(true)
            .with_icon(
                eframe::icon_data::from_png_bytes(include_bytes!("../assets/logo.png"))
                    .expect("the bundled logo is a valid PNG"),
            ),
        ..Default::default()
    };
    let result = eframe::run_native(
        &title,
        options,
        Box::new(|cc| {
            Ok(match app::Main::new(&cc.egui_ctx) {
                Ok(main) => Box::new(main),
                Err(message) => Box::new(screens::Fatal(message)),
            })
        }),
    );
    if app::DELETE_DATA_ON_EXIT.load(std::sync::atomic::Ordering::SeqCst) {
        let deleted = rom_core::db::Database::default_dir()
            .and_then(|dir| rom_core::db::delete_app_data(&dir));
        if let Err(e) = deleted {
            eprintln!("{}", errors::message(&e));
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use rust_i18n::t;

    fn used_keys() -> Vec<(String, String)> {
        let mut keys = Vec::new();
        let mut dirs = vec![std::path::PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src"
        ))];
        while let Some(dir) = dirs.pop() {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    dirs.push(path);
                    continue;
                }
                let text = std::fs::read_to_string(&path).unwrap();
                for (at, needle) in text.match_indices("t\x21(") {
                    let before = text[..at].chars().next_back();
                    if before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                        continue;
                    }
                    let rest = &text[at + needle.len()..];
                    if let Some(lit) = rest.trim_start().strip_prefix('"') {
                        let key = lit.split('"').next().unwrap();
                        keys.push((key.to_owned(), path.display().to_string()));
                    }
                }
            }
        }
        keys
    }

    #[test]
    fn every_key_used_in_the_source_is_in_en_yml() {
        let keys = used_keys();
        assert!(
            keys.iter().any(|(k, _)| k == "app.title"),
            "scanner finds keys"
        );
        for (key, file) in keys {
            let text = t!(&key, locale = "en");
            assert_ne!(
                text, key,
                "{file}: key {key:?} is missing from locales/en.yml"
            );
        }
    }

    #[test]
    fn window_title_key_resolves_to_english_text() {
        assert_eq!(t!("app.title", locale = "en"), "Ready Or Mod Manager");
    }
}
