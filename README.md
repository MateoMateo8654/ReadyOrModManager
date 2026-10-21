# Ready or Mod Manager

A lightweight desktop mod manager for *Ready or Not*, on Windows and Linux.

Drop in your mods, switch them on and off, and save sets of mods as Profiles you can swap between with one click. Then launch the game straight from the app.

Join the Discord for bug reports, update notifications and feature requests: https://discord.gg/P9Zm4rsqCX

Download the latest version from the [releases page](https://github.com/MateoMateo8654/ReadyOrModManager/releases/latest).

---

## Features

### Finds the game for you

On first start the app looks for *Ready or Not* in your Steam libraries. If it can't find it, pick the game folder by hand. You can change it later in Settings.

> **Epic Games Store:** not yet supported for automatic detection or the Play button. You can still use the app by choosing the game folder by hand; start the game from the Epic Games Launcher.

### Installing Mods

- Drag and drop files onto the window, or click the drop area to browse.
- `.zip`, `.7z` and `.tar` archives are opened for you: pick which files to install. Folders inside archives are flattened.
- Loose `.pak` files (or any other file) can be dropped directly.
- If a file with the same name is already installed, you choose whether to keep it, replace it, or skip it.
- Optionally, newly installed Mods are Enabled in the Active Profile right away (Settings).

### Enabling and Disabling Mods

Tick a Mod to Enable it, untick to Disable it. Disabled Mods stay installed and are just ignored by the game. Switching is instant, even for large Mods.

From the Mods list you can also uninstall a Mod or show it in its folder. Press **Refresh** if you added or removed files in the Mod Folder yourself.

### Profiles

A Profile is a named set of Mods that are Enabled together. Every other Mod is Disabled while that Profile is Active.

- **Switch** Profiles from the dropdown. The Mods are switched immediately.
- **Create** a new Profile, empty or starting from the Mods of the current one.
- **Rename** or **Delete** a Profile. Deleting a Profile keeps its Mods installed.
- **Export** a single Profile to a `.zip` together with its Mods.
- **Vanilla** is a built-in Profile with every Mod Disabled, for playing without mods.

### Play

Press **▶ PLAY** to start *Ready or Not* through Steam (Steam only for now; Epic Games Store is not yet supported).

### Export and Import

- **Export everything** writes all your Mods and Profiles into one `.zip`, for backups, moving to another PC, or sharing.
- **Import** an Export by dropping it onto the window or via Settings. Its Mods and Profiles are added; if a Profile with the same name already exists, you choose to replace it or keep both.
- Exports are compatible with v1 of Ready or Mod Manager in both directions.

### Coming from v1

If you used the old version (v1), the app finds its data in `Documents/RoM` on first start and offers to move your Mods and Profiles over. Afterwards it can delete the old folder for you.

### Updates

If you allow it, the app checks GitHub for a newer version on startup and links you to the release page. Nothing is downloaded or installed automatically. You can turn this off in Settings.

> **Strongly recommended:** keep the update check turned on, so you don't miss important updates and fixes.

### Settings

- **Game**: change the game folder or detect it from Steam again, and open the Mod Folder.
- **Mods**: Enable newly installed Mods automatically.
- **Export / Import**: export everything, or import an Export.
- **Updates**: check for a new version on startup.
- **Remove**:
  - **Delete all Mods** deletes every Mod file, optionally resetting all Profiles too.
  - **Uninstall Ready or Mod Manager** deletes the app's data (Profiles and settings), optionally all Mods too. Delete the program file yourself afterwards.

---

## Where things are stored

| What | Where |
| --- | --- |
| Mods | `ReadyOrNot/Content/Paks/RoM` inside the game folder. Disabled Mods end in `.disabled`. |
| Profiles and settings | Windows: `%APPDATA%\ReadyOrModManager`<br>Linux: `~/.config/ReadyOrModManager` |

---

## Usage of AI
Parts of this Program where developed by or with artificial intelligence, due to some time criticality, these parts of the code will be replaced by human written code whenever possible.

---

## Disclaimer

I am a single developer that trys to maintain this project in my free time in between my job and University, while i will try to respond to bug reports or feature Request, i can not guaranty that i will always have time.

---

## License

Ready or Mod Manager is licensed under [CC BY-NC 4.0](https://creativecommons.org/licenses/by-nc/4.0/), see [LICENSE](LICENSE). Release builds include third-party libraries and fonts under their own licenses; see [THIRD_PARTY_LICENSES.html](THIRD_PARTY_LICENSES.html). The same texts are built into the app under **Settings → Licenses**, so the program file alone is enough to distribute.

Ready or Mod Manager is not affiliated with or endorsed by VOID Interactive. *Ready or Not* is a trademark of its respective owner.

---

## Building from source

Requires a recent stable [Rust](https://rustup.rs) toolchain (edition 2024).

```sh
cargo build --release -p rom-gui
```

The binary is written to `target/release/rom-gui` (`rom-gui.exe` on Windows). Run the tests with `cargo test`.

After changing dependencies, regenerate the third-party license lists. The `.txt` is built into the app, and `cargo test` checks it covers the main dependencies:

```sh
cargo install cargo-about --locked --features cli
cargo about generate --all-features about.hbs -o THIRD_PARTY_LICENSES.html
cargo about generate --all-features about-txt.hbs -o crates/rom-gui/assets/THIRD_PARTY_LICENSES.txt
```

The workspace has two crates:

- `crates/rom-core`: Mods, Profiles, install/import/export, game detection and v1 Migration.
- `crates/rom-gui`: the [egui](https://github.com/emilk/egui) desktop app.

On Linux, install `crates/rom-gui/assets/readyormodmanager.desktop` and `logo.png` (as the `readyormodmanager` icon) so Wayland desktops show the app's icon.
