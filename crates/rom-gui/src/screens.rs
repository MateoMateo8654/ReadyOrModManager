use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, Frame, Id, Layout, Margin, RichText, ScrollArea,
    Stroke, Ui, Vec2,
};
use rom_core::update::{self, UpdateCheck};
use rust_i18n::t;

use crate::app::{Action, Main, Tab};
use crate::view::View;
use crate::widgets;

impl eframe::App for Main {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        while let Some(finished) = self.worker.poll() {
            self.finish(&ctx, finished);
        }
        self.expire_messages(&ctx);

        let mut actions = Vec::new();
        self.take_drops(&ctx, &mut actions);
        let enabled = !self.busy();

        if self.game.is_some() {
            self.toolbar(ui, enabled, &mut actions);
        }
        self.bottom_bars(ui, &mut actions);
        egui::CentralPanel::default().show(ui, |ui| {
            ui.add_enabled_ui(enabled, |ui| match (&self.view, self.tab) {
                _ if self.game.is_none() => setup_screen(ui, &mut actions),
                (Some(view), Tab::Mods) => {
                    if let Some(review) = &mut self.review {
                        let vanilla = view.profiles[0].id;
                        if let Some(step) = review.show(ui, vanilla) {
                            actions.push(Action::Review(step));
                        }
                    } else {
                        mods_tab(ui, view, &mut actions);
                    }
                }
                (_, Tab::Settings) => self.settings_tab(ui, &mut actions),
                (None, Tab::Mods) => {}
            });
        });

        if let Some(dialog) = self.dialogs.front_mut()
            && let Some(choice) = dialog.show(&ctx)
        {
            self.answer(&ctx, choice);
        }
        for action in actions {
            self.act(&ctx, action);
        }
        if ctx.input(|i| !i.raw.hovered_files.is_empty()) && self.accepts_drops() {
            drop_overlay(&ctx);
        }
    }
}

impl Main {
    fn accepts_drops(&self) -> bool {
        self.game.is_some() && !self.busy() && self.review.is_none() && self.dialogs.is_empty()
    }

    fn take_drops(&self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        let paths: Vec<_> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_owned())
                .collect()
        });
        if paths.is_empty() || self.game.is_none() {
            return;
        }
        actions.push(if self.accepts_drops() {
            Action::Install(paths)
        } else {
            Action::DropIgnored
        });
    }

    fn toolbar(&mut self, ui: &mut Ui, enabled: bool, actions: &mut Vec<Action>) {
        egui::Panel::top("toolbar")
            .frame(Frame::side_top_panel(ui.style()).inner_margin(Margin::symmetric(12, 10)))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if let Some(view) = &self.view {
                        ui.add_enabled_ui(enabled, |ui| profile_controls(ui, view, actions));
                    }
                    if let Some(busy) = self.worker.busy() {
                        ui.spinner();
                        ui.weak(busy);
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let play = egui::Button::new(
                            RichText::new(t!("toolbar.play"))
                                .size(18.0)
                                .strong()
                                .color(Color32::WHITE),
                        )
                        .fill(Color32::from_rgb(40, 140, 60))
                        .min_size(Vec2::new(110.0, 34.0));
                        if ui
                            .add(play)
                            .on_hover_text(t!("toolbar.play_hint"))
                            .clicked()
                        {
                            actions.push(Action::Launch);
                        }
                    });
                });
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.tab, Tab::Mods, t!("tab.mods"));
                    ui.selectable_value(&mut self.tab, Tab::Settings, t!("tab.settings"));
                });
            });
    }

    fn bottom_bars(&self, ui: &mut Ui, actions: &mut Vec<Action>) {
        if let Some(found) = &self.update {
            egui::Panel::bottom("update_banner")
                .frame(
                    Frame::new()
                        .fill(Color32::from_rgb(30, 80, 140))
                        .inner_margin(Margin::symmetric(12, 6)),
                )
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.colored_label(
                            Color32::WHITE,
                            t!("update.available", version = &found.version),
                        );
                        ui.hyperlink_to(
                            RichText::new(t!("update.open_release"))
                                .color(Color32::WHITE)
                                .underline(),
                            &found.url,
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui
                                .small_button("✕")
                                .on_hover_text(t!("common.dismiss"))
                                .clicked()
                            {
                                actions.push(Action::DismissUpdate);
                            }
                        });
                    });
                });
        }
        if !self.messages.is_empty() {
            egui::Panel::bottom("messages").show(ui, |ui| {
                ui.add_space(4.0);
                for (i, msg) in self.messages.iter().enumerate() {
                    ui.horizontal(|ui| {
                        if ui
                            .small_button("✕")
                            .on_hover_text(t!("common.dismiss"))
                            .clicked()
                        {
                            actions.push(Action::DismissMessage(i));
                        }
                        if msg.error {
                            ui.colored_label(ui.visuals().error_fg_color, &msg.text);
                        } else {
                            ui.label(&msg.text);
                        }
                    });
                }
                ui.add_space(2.0);
            });
        }
    }

    fn settings_tab(&self, ui: &mut Ui, actions: &mut Vec<Action>) {
        ScrollArea::vertical().show(ui, |ui| {
            ui.add_space(8.0);
            ui.heading(t!("settings.game"));
            if let Some(game) = &self.game {
                ui.label(game.display().to_string());
            }
            ui.horizontal(|ui| {
                if ui.button(t!("settings.change_folder")).clicked() {
                    actions.push(Action::PickGameFolder);
                }
                if ui.button(t!("settings.detect")).clicked() {
                    actions.push(Action::DetectGame);
                }
            });
            if let Some(folder) = self.mod_folder() {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.weak(t!("settings.mod_folder", path = folder.display()));
                    if ui.small_button(t!("settings.open")).clicked() {
                        actions.push(Action::OpenModFolder);
                    }
                });
            }

            ui.add_space(16.0);
            ui.heading(t!("settings.mods"));
            let mut auto = self.auto_enable;
            if ui.checkbox(&mut auto, t!("settings.auto_enable")).changed() {
                actions.push(Action::SetAutoEnable(auto));
            }

            ui.add_space(16.0);
            ui.heading(t!("settings.export_import"));
            ui.label(t!("settings.export_import_hint"));
            ui.horizontal(|ui| {
                if ui.button(t!("settings.export_all")).clicked() {
                    actions.push(Action::ExportAll);
                }
                if ui.button(t!("settings.import")).clicked() {
                    actions.push(Action::Import);
                }
            });

            ui.add_space(16.0);
            ui.heading(t!("settings.updates"));
            let mut check = self.update_check == UpdateCheck::Enabled;
            if ui
                .checkbox(&mut check, t!("settings.update_check"))
                .changed()
            {
                actions.push(Action::SetUpdateCheck(check));
            }
            ui.weak(t!("settings.version", version = update::CURRENT_VERSION));

            ui.add_space(16.0);
            ui.heading(t!("settings.remove"));
            ui.horizontal(|ui| {
                if ui.button(t!("settings.delete_all_mods")).clicked() {
                    actions.push(Action::DeleteAllMods);
                }
                if ui.button(t!("settings.uninstall_app")).clicked() {
                    actions.push(Action::UninstallApp);
                }
            });
        });
    }
}

fn profile_controls(ui: &mut Ui, view: &View, actions: &mut Vec<Action>) {
    let active = view.active();
    ui.label(t!("toolbar.profile"));
    let mut pick = active.id;
    egui::ComboBox::from_id_salt("profile")
        .selected_text(&active.name)
        .width(200.0)
        .show_ui(ui, |ui| {
            for p in &view.profiles {
                ui.selectable_value(&mut pick, p.id, &p.name);
            }
        });
    if pick != active.id {
        actions.push(Action::Apply(pick));
    }
    ui.menu_button("⋯", |ui| {
        if ui.button(t!("profile.new")).clicked() {
            actions.push(Action::NewProfile);
        }
        let editable = !active.builtin;
        if ui
            .add_enabled(editable, egui::Button::new(t!("profile.rename")))
            .clicked()
        {
            actions.push(Action::RenameProfile);
        }
        let deletable: Vec<_> = view
            .profiles
            .iter()
            .filter(|p| !p.builtin && p.id != active.id)
            .collect();
        ui.add_enabled_ui(!deletable.is_empty(), |ui| {
            ui.menu_button(t!("profile.delete"), |ui| {
                for p in deletable {
                    if ui.button(&p.name).clicked() {
                        actions.push(Action::DeleteProfile(p.clone()));
                    }
                }
            })
            .response
            .on_disabled_hover_text(t!("profile.delete_hint"));
        });
        if ui
            .add_enabled(editable, egui::Button::new(t!("profile.export")))
            .clicked()
        {
            actions.push(Action::ExportProfile);
        }
    });
    if ui
        .button("⟳")
        .on_hover_text(t!("toolbar.refresh"))
        .clicked()
    {
        actions.push(Action::Refresh);
    }
}

fn mods_tab(ui: &mut Ui, view: &View, actions: &mut Vec<Action>) {
    let drop = Frame::new()
        .stroke(Stroke::new(1.5, ui.visuals().weak_text_color()))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::same(18))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.vertical_centered(|ui| {
                ui.label(RichText::new(t!("mods.drop")).size(16.0));
                ui.weak(t!("mods.browse"));
            });
        });
    if drop
        .response
        .interact(egui::Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
    {
        actions.push(Action::PickInstallFiles);
    }
    ui.add_space(8.0);

    let vanilla = view.active().builtin;
    if vanilla {
        ui.label(t!("mods.vanilla_active"));
    }
    if view.mods.is_empty() {
        ui.add_space(16.0);
        ui.vertical_centered(|ui| ui.weak(t!("mods.empty")));
        return;
    }
    let enabled = view.mods.iter().filter(|m| m.enabled).count();
    ui.weak(t!("mods.count", enabled = enabled, total = view.mods.len()));
    ui.separator();

    ScrollArea::vertical().show(ui, |ui| {
        for m in &view.mods {
            ui.horizontal(|ui| {
                let mut enabled = m.enabled;
                let switch = ui.add_enabled(!vanilla, widgets::toggle_switch(&mut enabled));
                if switch.changed() {
                    actions.push(Action::SetEnabled(m.filename.clone(), enabled));
                }
                let name = RichText::new(&m.filename);
                ui.label(if m.enabled {
                    name.strong()
                } else {
                    name.weak()
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.menu_button("⋯", |ui| {
                        if ui.button(t!("mods.uninstall")).clicked() {
                            actions.push(Action::Uninstall(m.filename.clone()));
                        }
                        if ui.button(t!("mods.show_folder")).clicked() {
                            actions.push(Action::OpenModFolder);
                        }
                    });
                    if let Some(size) = m.size {
                        ui.weak(widgets::size(size));
                    }
                });
            });
            ui.separator();
        }
    });
}

fn setup_screen(ui: &mut Ui, actions: &mut Vec<Action>) {
    ui.vertical_centered(|ui| {
        ui.add_space(ui.available_height() / 4.0);
        ui.heading(t!("setup.title"));
        ui.add_space(8.0);
        ui.label(t!("setup.body"));
        ui.add_space(12.0);
        if ui
            .button(RichText::new(t!("setup.browse")).size(16.0))
            .clicked()
        {
            actions.push(Action::PickGameFolder);
        }
        if ui.button(t!("setup.search_again")).clicked() {
            actions.push(Action::DetectGame);
        }
    });
}

fn drop_overlay(ctx: &egui::Context) {
    egui::Area::new(Id::unique("drop_overlay"))
        .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            Frame::new()
                .fill(Color32::from_black_alpha(220))
                .inner_margin(Margin::same(40))
                .corner_radius(CornerRadius::same(12))
                .stroke(Stroke::new(2.0, Color32::LIGHT_BLUE))
                .show(ui, |ui| {
                    ui.colored_label(
                        Color32::WHITE,
                        RichText::new(t!("mods.drop_overlay")).heading(),
                    )
                });
        });
}

pub struct Fatal(pub String);

impl eframe::App for Fatal {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(ui.available_height() / 3.0);
                ui.heading(t!("fatal.title"));
                ui.label(&self.0);
            });
        });
    }
}
