use std::path::PathBuf;

use eframe::egui::{self, Context, Id, Modal, RichText, Ui};
use rom_core::apply::{ApplyFailure, ApplyInProgress};
use rom_core::migration::V1Install;
use rom_core::profiles::Profile;
use rust_i18n::t;

use crate::errors;

pub enum Dialog {
    AskUpdateCheck,
    OfferMigration(V1Install),
    DeleteV1 {
        dir: PathBuf,
    },
    ApplyFailed {
        in_progress: ApplyInProgress,
        switch: Switch,
    },
    ConfirmUninstall(String),
    NewProfile {
        name: String,
        copy: bool,
        active: String,
        error: Option<String>,
    },
    RenameProfile {
        profile: Profile,
        name: String,
        error: Option<String>,
    },
    DeleteProfile(Profile),
    DeleteAllMods {
        count: usize,
        profiles_too: bool,
    },
    UninstallApp {
        mods_too: bool,
        program: Option<PathBuf>,
        data: Option<PathBuf>,
    },
}

pub struct Switch {
    pub target: String,
    pub back_to: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    Yes,
    No,
}

impl Dialog {
    pub fn show(&mut self, ctx: &Context) -> Option<Choice> {
        let mut choice = None;
        Modal::new(Id::unique("rom_dialog")).show(ctx, |ui| {
            ui.set_max_width(440.0);
            choice = self.contents(ui);
        });
        choice
    }

    fn contents(&mut self, ui: &mut Ui) -> Option<Choice> {
        match self {
            Dialog::AskUpdateCheck => {
                ui.heading(t!("dialog.update_check_title"));
                ui.label(t!("dialog.update_check_body"));
                buttons(
                    ui,
                    t!("dialog.update_check_yes"),
                    t!("dialog.update_check_no"),
                )
            }
            Dialog::OfferMigration(v1) => {
                ui.heading(t!("dialog.migration_title"));
                ui.label(t!(
                    "dialog.migration_body",
                    dir = v1.dir.display(),
                    mods = v1.mods.len(),
                    profiles = v1.profiles.len()
                ));
                buttons(ui, t!("dialog.migration_yes"), t!("dialog.migration_no"))
            }
            Dialog::DeleteV1 { dir } => {
                ui.heading(t!("dialog.delete_v1_title"));
                ui.label(t!("dialog.delete_v1_body", dir = dir.display()));
                buttons(ui, t!("dialog.delete_v1_yes"), t!("common.keep"))
            }
            Dialog::ApplyFailed {
                in_progress,
                switch,
            } => {
                ui.heading(t!("dialog.apply_failed_title"));
                let failure = in_progress.failure();
                match failure {
                    ApplyFailure::Rename(r) => {
                        ui.label(t!("dialog.apply_failed_rename", filename = &r.filename));
                    }
                    ApplyFailure::Activate(_) => {
                        ui.label(t!("dialog.apply_failed_activate"));
                    }
                }
                ui.weak(errors::message(failure.error()));
                ui.add_space(4.0);
                if in_progress.target().is_none() {
                    ui.label(t!("dialog.undo_failed"));
                    return retry_only(ui);
                }
                match &switch.back_to {
                    Some(previous) => ui.label(t!(
                        "dialog.apply_failed_choice",
                        target = &switch.target,
                        previous = previous
                    )),
                    None => ui.label(t!("dialog.reapply_failed_choice", target = &switch.target)),
                };
                buttons(ui, t!("common.retry"), t!("common.cancel"))
            }
            Dialog::ConfirmUninstall(filename) => {
                ui.heading(t!("dialog.uninstall_title"));
                ui.label(t!("dialog.uninstall_body", filename = filename));
                buttons(ui, t!("dialog.uninstall_yes"), t!("common.keep"))
            }
            Dialog::NewProfile {
                name,
                copy,
                active,
                error,
            } => {
                ui.heading(t!("dialog.new_profile_title"));
                let submitted = name_field(ui, name, error);
                ui.checkbox(copy, t!("dialog.new_profile_copy", active = active));
                ui.weak(t!("dialog.new_profile_hint"));
                submitted.or_else(|| buttons(ui, t!("dialog.create"), t!("common.cancel")))
            }
            Dialog::RenameProfile {
                profile,
                name,
                error,
            } => {
                ui.heading(t!("dialog.rename_profile_title", name = &profile.name));
                let submitted = name_field(ui, name, error);
                submitted.or_else(|| buttons(ui, t!("dialog.rename"), t!("common.cancel")))
            }
            Dialog::DeleteAllMods {
                count,
                profiles_too,
            } => {
                ui.heading(t!("dialog.delete_all_mods_title"));
                ui.label(t!("dialog.delete_all_mods_body", n = count));
                ui.checkbox(profiles_too, t!("dialog.delete_all_mods_profiles"));
                buttons(ui, t!("dialog.delete_all_mods_yes"), t!("common.keep"))
            }
            Dialog::UninstallApp {
                mods_too,
                program,
                data,
            } => {
                ui.heading(t!("dialog.uninstall_app_title"));
                if let Some(data) = data {
                    ui.label(t!("dialog.uninstall_app_data", path = data.display()));
                }
                ui.checkbox(mods_too, t!("dialog.uninstall_app_mods"));
                if let Some(program) = program {
                    ui.label(t!("dialog.uninstall_app_program", path = program.display()));
                }
                buttons(ui, t!("dialog.uninstall_app_yes"), t!("common.cancel"))
            }
            Dialog::DeleteProfile(profile) => {
                ui.heading(t!("dialog.delete_profile_title"));
                ui.label(t!("dialog.delete_profile_body", name = &profile.name));
                buttons(ui, t!("dialog.delete"), t!("common.keep"))
            }
        }
    }
}

fn name_field(ui: &mut Ui, name: &mut String, error: &Option<String>) -> Option<Choice> {
    let field = ui.add(egui::TextEdit::singleline(name).hint_text(t!("dialog.profile_name")));
    if !field.has_focus() && !field.lost_focus() && name.is_empty() {
        field.request_focus();
    }
    if let Some(error) = error {
        ui.colored_label(ui.visuals().error_fg_color, error);
    }
    let entered = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
    entered.then_some(Choice::Yes)
}

fn retry_only(ui: &mut Ui) -> Option<Choice> {
    ui.add_space(8.0);
    ui.button(RichText::new(t!("common.retry")).strong())
        .clicked()
        .then_some(Choice::Yes)
}

fn buttons(ui: &mut Ui, yes: impl Into<String>, no: impl Into<String>) -> Option<Choice> {
    ui.add_space(8.0);
    let mut choice = None;
    ui.horizontal(|ui| {
        if ui.button(RichText::new(yes.into()).strong()).clicked() {
            choice = Some(Choice::Yes);
        }
        if ui.button(no.into()).clicked() {
            choice = Some(Choice::No);
        }
    });
    choice
}
