use eframe::egui::{self, RichText, ScrollArea, Ui};
use rom_core::import::{ClashResolution, Detected, ImportedProfile};
use rom_core::install::{Candidate, Collision, InstallPlan, Resolution, Source};
use rom_core::mods::ModState;
use rom_core::profiles::ProfileId;
use rust_i18n::t;

use crate::widgets;

pub enum Then {
    Install,
    Import(Vec<ImportedProfile>),
}

pub enum Review {
    Install {
        source: String,
        candidates: Vec<(Candidate, bool)>,
        rejected: Vec<String>,
    },
    Import {
        source: String,
        mods: Vec<(Candidate, bool)>,
        profiles: Vec<ImportedProfile>,
        rejected: Vec<String>,
    },
    Collisions {
        plan: InstallPlan,
        choices: Vec<Option<Resolution>>,
        then: Then,
    },
}

pub enum Step {
    Cancel,
    Plan(Vec<Candidate>, Then),
    Settled,
}

impl Review {
    pub fn new(source: String, detected: Detected) -> Self {
        match detected {
            Detected::Install(inspection) => Review::Install {
                source,
                candidates: checked(inspection.candidates),
                rejected: inspection.rejected,
            },
            Detected::Export(contents) => Review::Import {
                source,
                mods: checked(contents.mods),
                profiles: contents.profiles,
                rejected: contents.rejected,
            },
        }
    }

    pub fn collisions(plan: InstallPlan, then: Then) -> Self {
        let choices = vec![None; plan.collisions().len()];
        Review::Collisions {
            plan,
            choices,
            then,
        }
    }

    pub fn show(&mut self, ui: &mut Ui, vanilla: ProfileId) -> Option<Step> {
        match self {
            Review::Install {
                source,
                candidates,
                rejected,
            } => {
                ui.heading(t!("review.install_title", source = source));
                ui.label(t!("review.install_hint"));
                ui.separator();
                let mut step = None;
                ScrollArea::vertical()
                    .max_height(ui.available_height() - 60.0)
                    .show(ui, |ui| {
                        file_checklist(ui, candidates);
                        rejected_list(ui, rejected);
                    });
                ui.separator();
                let n = candidates.iter().filter(|c| c.1).count();
                ui.horizontal(|ui| {
                    let install = egui::Button::new(t!("review.install_n", n = n));
                    if ui.add_enabled(n > 0, install).clicked() {
                        step = Some(Step::Plan(selected(candidates), Then::Install));
                    }
                    if ui.button(t!("common.cancel")).clicked() {
                        step = Some(Step::Cancel);
                    }
                });
                step
            }
            Review::Import {
                source,
                mods,
                profiles,
                rejected,
            } => {
                ui.heading(t!("review.import_title", source = source));
                ui.label(t!("review.import_hint"));
                ui.separator();
                ScrollArea::vertical()
                    .max_height(ui.available_height() - 60.0)
                    .show(ui, |ui| {
                        ui.label(RichText::new(t!("review.profiles")).strong());
                        for p in profiles.iter_mut() {
                            profile_entry(ui, p, vanilla);
                        }
                        ui.add_space(8.0);
                        ui.label(RichText::new(t!("review.files")).strong());
                        file_checklist(ui, mods);
                        rejected_list(ui, rejected);
                    });
                ui.separator();
                let settled = profiles
                    .iter()
                    .all(|p| p.clash.is_none() || p.on_clash.is_some());
                let mut step = None;
                ui.horizontal(|ui| {
                    let import = egui::Button::new(t!("review.import"));
                    if ui.add_enabled(settled, import).clicked() {
                        let profiles = std::mem::take(profiles);
                        step = Some(Step::Plan(selected(mods), Then::Import(profiles)));
                    }
                    if ui.button(t!("common.cancel")).clicked() {
                        step = Some(Step::Cancel);
                    }
                    if !settled {
                        ui.weak(t!("review.settle_clashes"));
                    }
                });
                step
            }
            Review::Collisions { plan, choices, .. } => {
                ui.heading(t!("review.collisions_title"));
                ui.label(t!("review.collisions_hint"));
                ui.separator();
                ScrollArea::vertical()
                    .max_height(ui.available_height() - 60.0)
                    .show(ui, |ui| {
                        for (i, collision) in plan.collisions().iter().enumerate() {
                            collision_entry(ui, i, collision, &mut choices[i]);
                            ui.separator();
                        }
                    });
                let settled = choices.iter().all(Option::is_some);
                let mut step = None;
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(settled, egui::Button::new(t!("review.continue")))
                        .clicked()
                    {
                        step = Some(Step::Settled);
                    }
                    if ui.button(t!("common.cancel")).clicked() {
                        step = Some(Step::Cancel);
                    }
                });
                step
            }
        }
    }

    pub fn into_plan(self) -> Option<(InstallPlan, Then)> {
        let Review::Collisions {
            mut plan,
            choices,
            then,
        } = self
        else {
            return None;
        };
        for (i, choice) in choices.into_iter().enumerate() {
            plan.resolve(i, choice?);
        }
        Some((plan, then))
    }
}

fn checked(candidates: Vec<Candidate>) -> Vec<(Candidate, bool)> {
    candidates
        .into_iter()
        .map(|c| {
            let on = c.preselected;
            (c, on)
        })
        .collect()
}

fn selected(candidates: &[(Candidate, bool)]) -> Vec<Candidate> {
    candidates
        .iter()
        .filter(|c| c.1)
        .map(|c| c.0.clone())
        .collect()
}

fn file_checklist(ui: &mut Ui, candidates: &mut [(Candidate, bool)]) {
    egui::Grid::new("review_files")
        .num_columns(2)
        .striped(true)
        .show(ui, |ui| {
            for (c, on) in candidates.iter_mut() {
                ui.checkbox(on, c.filename.as_str())
                    .on_hover_text(source_text(&c.source));
                ui.weak(widgets::size(c.size));
                ui.end_row();
            }
        });
}

fn rejected_list(ui: &mut Ui, rejected: &[String]) {
    if rejected.is_empty() {
        return;
    }
    ui.add_space(8.0);
    ui.collapsing(t!("review.rejected", n = rejected.len()), |ui| {
        for r in rejected {
            ui.weak(r);
        }
    });
}

fn profile_entry(ui: &mut Ui, p: &mut ImportedProfile, vanilla: ProfileId) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(&p.name).strong());
        ui.weak(t!("review.profile_mods", n = p.mods.len()));
    });
    ui.indent(("profile", &p.name), |ui| {
        if !p.missing.is_empty() {
            ui.label(t!("review.missing", list = p.missing.join(", ")));
        }
        if let Some(clash) = p.clash {
            ui.label(t!("review.clash", name = &p.name));
            ui.horizontal(|ui| {
                if clash != vanilla {
                    ui.radio_value(
                        &mut p.on_clash,
                        Some(ClashResolution::Replace),
                        t!("review.replace"),
                    );
                }
                ui.radio_value(
                    &mut p.on_clash,
                    Some(ClashResolution::KeepBoth),
                    t!("review.keep_both"),
                );
            });
        }
    });
}

fn collision_entry(ui: &mut Ui, i: usize, c: &Collision, choice: &mut Option<Resolution>) {
    ui.label(RichText::new(&c.filename).strong());
    match c.existing {
        Some(ModState::Enabled) => ui.weak(t!("review.existing_enabled")),
        Some(ModState::Disabled) => ui.weak(t!("review.existing_disabled")),
        None => ui.weak(t!("review.same_name", n = c.candidates.len())),
    };
    ui.push_id(i, |ui| {
        let keep = if c.existing.is_some() {
            t!("review.keep_installed")
        } else {
            t!("review.skip")
        };
        ui.radio_value(choice, Some(Resolution::Skip), keep);
        for (j, cand) in c.candidates.iter().enumerate() {
            let text = if c.existing.is_some() && c.candidates.len() == 1 {
                t!("review.replace_installed")
            } else {
                t!("review.use_from", source = source_text(&cand.source))
            };
            ui.radio_value(choice, Some(Resolution::Use(j)), text);
        }
    });
}

fn source_text(source: &Source) -> String {
    match source {
        Source::File(path) => path.display().to_string(),
        Source::Archive { archive, entry, .. } => {
            let name = archive.file_name().unwrap_or_default().to_string_lossy();
            format!("{name} › {entry}")
        }
    }
}
