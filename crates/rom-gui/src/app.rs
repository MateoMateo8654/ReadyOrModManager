use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use eframe::egui::{Context, ViewportCommand};
use rom_core::apply::{self, ApplyOutcome};
use rom_core::db::Database;
use rom_core::import::{self, ImportedProfile};
use rom_core::install::{self, InstallPlan};
use rom_core::migration::{self, V1Install};
use rom_core::profiles::{self, Profile, ProfileId};
use rom_core::update::{self, AvailableUpdate, UpdateCheck, VersionFetcher};
use rom_core::{Error, export, game, launch, mods, settings, uninstall};
use rust_i18n::t;

use crate::dialogs::{Choice, Dialog, Switch};
use crate::errors;
use crate::review::{Review, Step, Then};
use crate::view::View;
use crate::worker::Worker;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Mods,
    Settings,
}

pub struct Message {
    pub text: String,
    pub error: bool,
    expires: Option<Instant>,
}

const INFO_LIFETIME: Duration = Duration::from_secs(5);

pub enum Action {
    Apply(ProfileId),
    SetEnabled(String, bool),
    Uninstall(String),
    Refresh,
    Launch,
    PickInstallFiles,
    Install(Vec<PathBuf>),
    NewProfile,
    RenameProfile,
    DeleteProfile(Profile),
    ExportProfile,
    ExportAll,
    Import,
    PickGameFolder,
    DetectGame,
    OpenModFolder,
    SetAutoEnable(bool),
    SetUpdateCheck(bool),
    DismissUpdate,
    DismissMessage(usize),
    Review(Step),
    DeleteAllMods,
    UninstallApp,
    ShowLicenses,
    DropIgnored,
}

pub static DELETE_DATA_ON_EXIT: AtomicBool = AtomicBool::new(false);

pub enum Finished {
    Reviewed(String, rom_core::Result<import::Detected>),
    Installed(rom_core::Result<usize>),
    Imported(rom_core::Result<ImportSummary>),
    Applied(rom_core::Result<ApplyOutcome>, Switch),
    ExportWithOtherFiles(String),
    Exported(rom_core::Result<PathBuf>),
    Migrated(rom_core::Result<MigrationSummary>),
    Launched(rom_core::Result<()>),
    UpdateChecked(Option<AvailableUpdate>),
    ModsDeleted(rom_core::Result<()>),
    ReadyToUninstall(rom_core::Result<()>),
    Picked(Option<Picked>),
}

pub enum Picked {
    InstallFiles(Vec<PathBuf>),
    ImportFile(PathBuf),
    GameFolder(PathBuf),
    ExportProfile(ProfileId, PathBuf),
    ExportAll(PathBuf),
}

pub struct ImportSummary {
    mods: usize,
    profiles: usize,
    left_out: usize,
    apply: Option<rom_core::Result<ApplyOutcome>>,
}

pub struct MigrationSummary {
    left_out: usize,
    apply: ApplyOutcome,
    dir: PathBuf,
}

pub struct Main {
    pub worker: Worker<Finished>,
    home: Option<PathBuf>,
    pub game: Option<PathBuf>,
    pub view: Option<View>,
    pub tab: Tab,
    pub review: Option<Review>,
    pub dialogs: VecDeque<Dialog>,
    pub messages: Vec<Message>,
    pub update: Option<AvailableUpdate>,
    pub update_check: UpdateCheck,
    pub auto_enable: bool,
    picking: bool,
    after_apply: Vec<Dialog>,
}

impl Main {
    pub fn new(ctx: &Context) -> Result<Self, String> {
        let fatal = |e: Error| errors::message(&e);
        let mut db = Database::open_default().map_err(fatal)?;
        profiles::init(&mut db).map_err(fatal)?;
        let update_check = UpdateCheck::load(&db).map_err(fatal)?;
        let auto_enable = settings::auto_enable(&db).map_err(fatal)?;
        let stored = settings::game_path(&db).map_err(fatal)?;

        let mut app = Self {
            worker: Worker::new(db),
            home: dirs::home_dir(),
            game: None,
            view: None,
            tab: Tab::Mods,
            review: None,
            dialogs: VecDeque::new(),
            messages: Vec::new(),
            update: None,
            update_check,
            auto_enable,
            picking: false,
            after_apply: Vec::new(),
        };
        match update_check {
            UpdateCheck::Unasked => app.dialogs.push_back(Dialog::AskUpdateCheck),
            UpdateCheck::Enabled => app.check_for_update(ctx),
            UpdateCheck::Disabled => {}
        }
        match stored.filter(|p| game::is_game_root(p)) {
            Some(path) => app.use_game(path),
            None => {
                if let Some(path) = game::detect_game() {
                    app.set_game(path);
                }
            }
        }
        Ok(app)
    }

    pub fn mod_folder(&self) -> Option<PathBuf> {
        self.game.as_deref().map(game::mod_folder)
    }

    pub fn busy(&self) -> bool {
        self.worker.busy().is_some() || self.picking
    }

    fn error(&mut self, err: &Error) {
        self.messages.push(Message {
            text: errors::message(err),
            error: true,
            expires: None,
        });
    }

    fn error_text(&mut self, text: impl Into<String>) {
        self.messages.push(Message {
            text: text.into(),
            error: true,
            expires: None,
        });
    }

    fn info(&mut self, text: impl Into<String>) {
        self.messages.push(Message {
            text: text.into(),
            error: false,
            expires: Some(Instant::now() + INFO_LIFETIME),
        });
    }

    pub fn expire_messages(&mut self, ctx: &Context) {
        let now = Instant::now();
        self.messages.retain(|m| m.expires.is_none_or(|t| t > now));
        if let Some(next) = self.messages.iter().filter_map(|m| m.expires).min() {
            ctx.request_repaint_after(next - now);
        }
    }

    fn set_game(&mut self, path: PathBuf) {
        if !game::is_game_root(&path) {
            self.error_text(t!("error.not_game_folder", path = path.display()));
            return;
        }
        let saved = settings::set_game_path(&self.worker.db(), &path);
        if let Err(e) = saved {
            self.error(&e);
            return;
        }
        self.use_game(path);
    }

    fn use_game(&mut self, path: PathBuf) {
        self.game = Some(path);
        self.refresh();
        let offer = match &self.home {
            Some(home) => migration::should_offer(&self.worker.db(), home),
            None => Ok(None),
        };
        match offer {
            Ok(Some(v1)) => self.dialogs.push_back(Dialog::OfferMigration(v1)),
            Ok(None) => {}
            Err(e) => self.error(&e),
        }
    }

    fn refresh(&mut self) {
        let Some(folder) = self.mod_folder() else {
            return;
        };
        let result = mods::refresh(&mut self.worker.db(), &folder);
        if let Err(e) = result {
            self.error(&e);
        }
        self.reload();
    }

    fn reload(&mut self) {
        let Some(folder) = self.mod_folder() else {
            return;
        };
        let loaded = View::load(&self.worker.db(), &folder);
        match loaded {
            Ok(view) => self.view = Some(view),
            Err(e) => self.error(&e),
        }
    }

    fn active_name(&self) -> String {
        self.view
            .as_ref()
            .map(|v| v.active().name.clone())
            .unwrap_or_default()
    }

    fn profile_name(&self, id: ProfileId) -> String {
        self.view
            .as_ref()
            .and_then(|v| v.profile_name(id))
            .unwrap_or_default()
            .to_owned()
    }

    pub fn act(&mut self, ctx: &Context, action: Action) {
        match action {
            Action::Apply(target) => self.start_apply(ctx, target),
            Action::SetEnabled(filename, enabled) => self.set_enabled(&filename, enabled),
            Action::DropIgnored => self.info(t!("info.drop_ignored")),
            Action::Uninstall(filename) => {
                self.dialogs.push_back(Dialog::ConfirmUninstall(filename))
            }
            Action::Refresh => self.refresh(),
            Action::Launch => self.launch(ctx),
            Action::PickInstallFiles => {
                let picker = rfd::FileDialog::new()
                    .set_title(t!("picker.install"))
                    .add_filter(
                        t!("picker.mods_and_archives"),
                        &["zip", "7z", "tar", "pak", "utoc", "ucas"],
                    )
                    .add_filter(t!("picker.all_files"), &["*"]);
                self.pick(ctx, move || picker.pick_files().map(Picked::InstallFiles));
            }
            Action::Install(paths) => self.start_review(ctx, paths),
            Action::NewProfile => {
                let active = self.active_name();
                self.dialogs.push_back(Dialog::NewProfile {
                    name: String::new(),
                    copy: true,
                    active,
                    error: None,
                });
            }
            Action::RenameProfile => {
                if let Some(view) = &self.view {
                    let profile = view.active().clone();
                    self.dialogs.push_back(Dialog::RenameProfile {
                        name: profile.name.clone(),
                        profile,
                        error: None,
                    });
                }
            }
            Action::DeleteProfile(profile) => {
                self.dialogs.push_back(Dialog::DeleteProfile(profile))
            }
            Action::DeleteAllMods => {
                let count = self.view.as_ref().map_or(0, |v| v.mods.len());
                self.dialogs.push_back(Dialog::DeleteAllMods {
                    count,
                    profiles_too: false,
                });
            }
            Action::UninstallApp => self.dialogs.push_back(Dialog::UninstallApp {
                mods_too: false,
                program: program_file(),
                data: rom_core::db::Database::default_dir().ok(),
            }),
            Action::ShowLicenses => self.dialogs.push_back(Dialog::Licenses),
            Action::ExportProfile => self.export_profile(ctx),
            Action::ExportAll => self.export_all(ctx),
            Action::Import => {
                let picker = rfd::FileDialog::new()
                    .set_title(t!("picker.import"))
                    .add_filter(t!("picker.exports"), &["zip"]);
                self.pick(ctx, move || picker.pick_file().map(Picked::ImportFile));
            }
            Action::PickGameFolder => {
                let mut picker = rfd::FileDialog::new().set_title(t!("picker.game_folder"));
                if let Some(game) = &self.game {
                    picker = picker.set_directory(game);
                }
                self.pick(ctx, move || picker.pick_folder().map(Picked::GameFolder));
            }
            Action::DetectGame => match game::detect_game() {
                Some(path) => self.set_game(path),
                None => self.error_text(t!("error.game_not_detected")),
            },
            Action::OpenModFolder => {
                if let Some(folder) = self.mod_folder()
                    && let Err(e) = open::that_detached(&folder)
                {
                    self.error(&Error::Io(e));
                }
            }
            Action::SetAutoEnable(on) => {
                let saved = settings::set_auto_enable(&self.worker.db(), on);
                match saved {
                    Ok(()) => self.auto_enable = on,
                    Err(e) => self.error(&e),
                }
            }
            Action::SetUpdateCheck(on) => self.set_update_check(ctx, on),
            Action::DismissUpdate => self.update = None,
            Action::DismissMessage(i) => {
                if i < self.messages.len() {
                    self.messages.remove(i);
                }
            }
            Action::Review(step) => self.review_step(ctx, step),
        }
    }

    fn set_enabled(&mut self, filename: &str, enabled: bool) {
        let Some(folder) = self.mod_folder() else {
            return;
        };
        let result =
            mods::set_enabled_in_active_profile(&self.worker.db(), &folder, filename, enabled);
        if let Err(e) = result {
            self.error(&e);
        }
        self.reload();
    }

    fn start_apply(&mut self, ctx: &Context, target: ProfileId) {
        let Some(folder) = self.mod_folder() else {
            return;
        };
        let switch = Switch {
            target: self.profile_name(target),
            back_to: Some(self.active_name()),
        };
        self.worker.run(
            ctx,
            t!("busy.applying", name = &switch.target).into_owned(),
            move |db| Finished::Applied(apply::apply(db, &folder, target), switch),
        );
    }

    fn applied(&mut self, outcome: ApplyOutcome, switch: Switch) {
        match outcome {
            ApplyOutcome::Done => self.dialogs.extend(self.after_apply.drain(..)),
            ApplyOutcome::Failed(in_progress) => {
                self.dialogs.push_front(Dialog::ApplyFailed {
                    in_progress,
                    switch,
                });
            }
        }
    }

    fn start_review(&mut self, ctx: &Context, paths: Vec<PathBuf>) {
        if paths.is_empty() || self.mod_folder().is_none() {
            return;
        }
        let source = match paths.as_slice() {
            [one] => one
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            many => t!("review.n_files", n = many.len()).into_owned(),
        };
        self.tab = Tab::Mods;
        self.worker.run(
            ctx,
            t!("busy.reading", source = &source).into_owned(),
            move |db| {
                if let [one] = paths.as_slice() {
                    return Finished::Reviewed(source, import::detect(db, one));
                }
                for path in &paths {
                    if let Ok(import::Detected::Export(_)) = import::detect(db, path) {
                        let name = path.file_name().unwrap_or_default();
                        return Finished::ExportWithOtherFiles(name.to_string_lossy().into_owned());
                    }
                }
                Finished::Reviewed(
                    source,
                    install::inspect(&paths).map(import::Detected::Install),
                )
            },
        );
    }

    fn review_step(&mut self, ctx: &Context, step: Step) {
        let Some(folder) = self.mod_folder() else {
            return;
        };
        match step {
            Step::Cancel => self.review = None,
            Step::Plan(selection, then) => match install::plan(&folder, selection) {
                Ok(plan) if plan.collisions().is_empty() => {
                    self.review = None;
                    self.run_plan(ctx, folder, plan, then);
                }
                Ok(plan) => self.review = Some(Review::collisions(plan, then)),
                Err(e) => self.error(&e),
            },
            Step::Settled => {
                if let Some((plan, then)) = self.review.take().and_then(Review::into_plan) {
                    self.run_plan(ctx, folder, plan, then);
                }
            }
        }
    }

    fn run_plan(&mut self, ctx: &Context, folder: PathBuf, plan: InstallPlan, then: Then) {
        match then {
            Then::Install => self
                .worker
                .run(ctx, t!("busy.installing").into_owned(), move |db| {
                    Finished::Installed(install::install(db, &folder, plan).map(|m| m.len()))
                }),
            Then::Import(profiles) => {
                self.worker
                    .run(ctx, t!("busy.importing").into_owned(), move |db| {
                        Finished::Imported(run_import(db, &folder, plan, profiles))
                    })
            }
        }
    }

    fn pick(&mut self, ctx: &Context, picker: impl FnOnce() -> Option<Picked> + Send + 'static) {
        if self.picking {
            return;
        }
        self.picking = true;
        self.worker
            .run_quiet(ctx, move |_| Finished::Picked(picker()));
    }

    fn picked(&mut self, ctx: &Context, picked: Picked) {
        match picked {
            Picked::InstallFiles(files) => self.start_review(ctx, files),
            Picked::ImportFile(file) => {
                self.tab = Tab::Mods;
                self.start_review(ctx, vec![file]);
            }
            Picked::GameFolder(path) => self.set_game(path),
            Picked::ExportProfile(profile, dest) => self.export_profile_to(ctx, profile, dest),
            Picked::ExportAll(dest) => self.export_all_to(ctx, dest),
        }
    }

    fn export_profile(&mut self, ctx: &Context) {
        let Some(view) = &self.view else { return };
        let profile = view.active().id;
        let picker = zip_picker(&format!("{}.zip", view.active().name));
        self.pick(ctx, move || {
            save_zip(picker).map(|dest| Picked::ExportProfile(profile, dest))
        });
    }

    fn export_profile_to(&mut self, ctx: &Context, profile: ProfileId, dest: PathBuf) {
        let Some(folder) = self.mod_folder() else {
            return;
        };
        self.worker
            .run(ctx, t!("busy.exporting").into_owned(), move |db| {
                Finished::Exported(
                    export::export_profile(db, &folder, profile, &dest).map(|()| dest),
                )
            });
    }

    fn export_all(&mut self, ctx: &Context) {
        let picker = zip_picker(&t!("picker.export_all_name"));
        self.pick(ctx, move || save_zip(picker).map(Picked::ExportAll));
    }

    fn export_all_to(&mut self, ctx: &Context, dest: PathBuf) {
        let Some(folder) = self.mod_folder() else {
            return;
        };
        self.worker
            .run(ctx, t!("busy.exporting").into_owned(), move |db| {
                Finished::Exported(export::export_all(db, &folder, &dest).map(|()| dest))
            });
    }

    fn launch(&mut self, ctx: &Context) {
        self.worker
            .run_quiet(ctx, |_| Finished::Launched(launch::launch_game()));
        self.info(t!("info.launching"));
    }

    fn set_update_check(&mut self, ctx: &Context, on: bool) {
        let check = if on {
            UpdateCheck::Enabled
        } else {
            UpdateCheck::Disabled
        };
        let saved = check.save(&self.worker.db());
        if let Err(e) = saved {
            self.error(&e);
            return;
        }
        self.update_check = check;
        if on {
            self.check_for_update(ctx);
        } else {
            self.update = None;
        }
    }

    fn check_for_update(&self, ctx: &Context) {
        self.worker.run_quiet(ctx, |db| {
            let body = update::GithubFetcher.fetch_latest_version();
            let db = db.lock().unwrap_or_else(|e| e.into_inner());
            let found = update::check_for_update(&db, update::CURRENT_VERSION, &Fetched(body));
            Finished::UpdateChecked(found.ok().flatten())
        });
    }

    pub fn finish(&mut self, ctx: &Context, finished: Finished) {
        match finished {
            Finished::Reviewed(source, Ok(detected)) => {
                self.review = Some(Review::new(source, detected));
            }
            Finished::Installed(Ok(n)) => {
                self.info(t!("info.installed", n = n));
                self.reload();
            }
            Finished::Imported(Ok(summary)) => {
                self.info(t!(
                    "info.imported",
                    mods = summary.mods,
                    profiles = summary.profiles
                ));
                if summary.left_out > 0 {
                    self.info(t!("info.left_out", n = summary.left_out));
                }
                self.reload();
                match summary.apply {
                    Some(Ok(outcome)) => {
                        let switch = Switch {
                            target: self.active_name(),
                            back_to: None,
                        };
                        self.applied(outcome, switch);
                    }
                    Some(Err(e)) => self.error(&e),
                    None => {}
                }
            }
            Finished::Applied(Ok(outcome), switch) => {
                self.applied(outcome, switch);
                self.reload();
            }
            Finished::ExportWithOtherFiles(name) => {
                self.error_text(t!("error.export_with_others", name = name));
            }
            Finished::Exported(Ok(dest)) => {
                self.info(t!("info.exported", path = dest.display()));
            }
            Finished::Migrated(Ok(summary)) => {
                self.info(t!("info.migrated"));
                if summary.left_out > 0 {
                    self.info(t!("info.left_out", n = summary.left_out));
                }
                self.reload();
                let target = match &summary.apply {
                    ApplyOutcome::Failed(p) => p.target(),
                    ApplyOutcome::Done => None,
                };
                let switch = Switch {
                    target: target.map_or_else(|| self.active_name(), |id| self.profile_name(id)),
                    back_to: None,
                };
                self.after_apply.push(Dialog::DeleteV1 { dir: summary.dir });
                self.applied(summary.apply, switch);
            }
            Finished::Launched(Ok(())) => {}
            Finished::Picked(picked) => {
                self.picking = false;
                if let Some(picked) = picked {
                    self.picked(ctx, picked);
                }
            }
            Finished::ModsDeleted(Ok(())) => {
                self.info(t!("info.mods_deleted"));
                self.reload();
            }
            Finished::ReadyToUninstall(Ok(())) => {
                DELETE_DATA_ON_EXIT.store(true, Ordering::SeqCst);
                ctx.send_viewport_cmd(ViewportCommand::Close);
            }
            Finished::UpdateChecked(found) => self.update = found,
            Finished::Reviewed(_, Err(e))
            | Finished::Installed(Err(e))
            | Finished::Imported(Err(e))
            | Finished::Applied(Err(e), _)
            | Finished::Exported(Err(e))
            | Finished::Migrated(Err(e))
            | Finished::Launched(Err(e))
            | Finished::ModsDeleted(Err(e))
            | Finished::ReadyToUninstall(Err(e)) => {
                self.error(&e);
                self.reload();
            }
        }
    }

    pub fn answer(&mut self, ctx: &Context, choice: Choice) {
        let Some(dialog) = self.dialogs.pop_front() else {
            return;
        };
        let yes = choice == Choice::Yes;
        match dialog {
            Dialog::AskUpdateCheck => self.set_update_check(ctx, yes),
            Dialog::OfferMigration(v1) if yes => self.start_migration(ctx, v1),
            Dialog::OfferMigration(_) => {
                let declined = migration::decline(&self.worker.db());
                if let Err(e) = declined {
                    self.error(&e);
                }
            }
            Dialog::DeleteV1 { .. } if yes => {
                if let Some(home) = &self.home
                    && let Err(e) = migration::delete_v1(home)
                {
                    self.error(&e);
                }
            }
            Dialog::DeleteV1 { .. } => {}
            Dialog::ApplyFailed {
                in_progress,
                switch,
            } => {
                let label = if yes && in_progress.target().is_some() {
                    t!("busy.applying", name = &switch.target)
                } else {
                    t!("busy.undoing")
                }
                .into_owned();
                self.worker.run(ctx, label, move |db| {
                    let outcome = if yes {
                        in_progress.retry(db)
                    } else {
                        in_progress.cancel()
                    };
                    Finished::Applied(Ok(outcome), switch)
                });
            }
            Dialog::ConfirmUninstall(filename) if yes => {
                if let Some(folder) = self.mod_folder() {
                    let result = uninstall::uninstall(&mut self.worker.db(), &folder, &filename);
                    match result {
                        Ok(()) => self.info(t!("info.uninstalled", filename = &filename)),
                        Err(e) => self.error(&e),
                    }
                    self.reload();
                }
            }
            Dialog::NewProfile {
                name, copy, active, ..
            } if yes => match self.create_profile(&name, copy) {
                Ok(id) => self.start_apply(ctx, id),
                Err(e) => self.dialogs.push_front(Dialog::NewProfile {
                    name,
                    copy,
                    active,
                    error: Some(errors::message(&e)),
                }),
            },
            Dialog::RenameProfile { profile, name, .. } if yes => {
                let renamed = profiles::rename(&self.worker.db(), profile.id, &name);
                match renamed {
                    Ok(()) => self.reload(),
                    Err(e) => self.dialogs.push_front(Dialog::RenameProfile {
                        profile,
                        name,
                        error: Some(errors::message(&e)),
                    }),
                }
            }
            Dialog::DeleteProfile(profile) if yes => {
                let deleted = profiles::delete(&self.worker.db(), profile.id);
                match deleted {
                    Ok(()) => self.reload(),
                    Err(e) => self.error(&e),
                }
            }
            Dialog::DeleteAllMods { profiles_too, .. } if yes => {
                let Some(folder) = self.mod_folder() else {
                    return;
                };
                self.worker
                    .run(ctx, t!("busy.deleting_mods").into_owned(), move |db| {
                        Finished::ModsDeleted(delete_all_mods(db, &folder, profiles_too))
                    });
            }
            Dialog::UninstallApp { mods_too, .. } if yes => {
                let folder = self.mod_folder().filter(|_| mods_too);
                self.worker
                    .run(ctx, t!("busy.uninstalling").into_owned(), move |db| {
                        let deleted = match folder {
                            Some(folder) => delete_all_mods(db, &folder, false),
                            None => Ok(()),
                        };
                        Finished::ReadyToUninstall(deleted)
                    });
            }
            Dialog::ConfirmUninstall(_)
            | Dialog::NewProfile { .. }
            | Dialog::RenameProfile { .. }
            | Dialog::DeleteProfile(_)
            | Dialog::DeleteAllMods { .. }
            | Dialog::UninstallApp { .. }
            | Dialog::Licenses => {}
        }
    }

    fn create_profile(&mut self, name: &str, copy: bool) -> rom_core::Result<ProfileId> {
        let db = self.worker.db();
        if copy {
            profiles::duplicate(&db, profiles::active(&db)?, name)
        } else {
            profiles::create(&db, name)
        }
    }

    fn start_migration(&mut self, ctx: &Context, v1: V1Install) {
        let Some(folder) = self.mod_folder() else {
            return;
        };
        self.worker
            .run(ctx, t!("busy.migrating").into_owned(), move |db| {
                Finished::Migrated(run_migration(db, &folder, &v1))
            });
    }
}

fn delete_all_mods(db: &mut Database, folder: &Path, profiles_too: bool) -> rom_core::Result<()> {
    match mods::refresh(db, folder) {
        Ok(_) | Err(Error::ModConflict { .. }) => {}
        Err(e) => return Err(e),
    }
    uninstall::uninstall_all(db, folder)?;
    if profiles_too {
        profiles::reset(db)?;
    }
    Ok(())
}

fn program_file() -> Option<PathBuf> {
    std::env::var_os("APPIMAGE")
        .map(PathBuf::from)
        .or_else(|| std::env::current_exe().ok())
}

fn run_import(
    db: &mut Database,
    folder: &Path,
    plan: InstallPlan,
    profiles: Vec<ImportedProfile>,
) -> rom_core::Result<ImportSummary> {
    let done = import::import_and_apply(db, folder, plan, profiles)?;
    Ok(ImportSummary {
        mods: done.imported.mods.len(),
        profiles: done.imported.profiles.len(),
        left_out: done.imported.left_out.len(),
        apply: done.apply,
    })
}

fn run_migration(
    db: &mut Database,
    folder: &Path,
    v1: &V1Install,
) -> rom_core::Result<MigrationSummary> {
    let migrated = migration::migrate(db, folder, v1)?;
    Ok(MigrationSummary {
        left_out: migrated.left_out.len(),
        apply: migrated.apply,
        dir: v1.dir.clone(),
    })
}

fn zip_picker(name: &str) -> rfd::FileDialog {
    rfd::FileDialog::new()
        .set_title(t!("picker.export"))
        .set_file_name(name)
        .add_filter(t!("picker.exports"), &["zip"])
}

fn save_zip(picker: rfd::FileDialog) -> Option<PathBuf> {
    let path = picker.save_file()?;
    if path.extension().is_none() {
        return Some(path.with_extension("zip"));
    }
    Some(path)
}

struct Fetched(Option<String>);

impl VersionFetcher for Fetched {
    fn fetch_latest_version(&self) -> Option<String> {
        self.0.clone()
    }
}
