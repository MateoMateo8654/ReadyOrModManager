use eframe::egui::{self, Ui, Vec2};
use rust_i18n::t;

pub fn toggle_switch(on: &mut bool) -> impl egui::Widget + '_ {
    move |ui: &mut Ui| {
        let size = ui.spacing().interact_size.y * Vec2::new(2.0, 1.0);
        let (rect, mut response) = ui.allocate_exact_size(size, egui::Sense::click());
        if response.clicked() {
            *on = !*on;
            response.mark_changed();
        }
        if ui.is_rect_visible(rect) {
            let t = ui.ctx().animate_bool_responsive(response.id, *on);
            let visuals = ui.style().interact_selectable(&response, *on);
            let radius = 0.5 * rect.height();
            ui.painter().rect(
                rect,
                radius,
                visuals.bg_fill,
                visuals.bg_stroke,
                egui::StrokeKind::Inside,
            );
            let x = egui::lerp((rect.left() + radius)..=(rect.right() - radius), t);
            let center = egui::pos2(x, rect.center().y);
            ui.painter().circle(
                center,
                0.75 * radius,
                visuals.fg_stroke.color,
                visuals.fg_stroke,
            );
        }
        response
    }
}

pub fn size(bytes: u64) -> String {
    let b = bytes as f64;
    match bytes {
        1_000_000_000.. => t!("size.gb", n = format!("{:.1}", b / 1e9)),
        1_000_000.. => t!("size.mb", n = format!("{:.1}", b / 1e6)),
        1_000.. => t!("size.kb", n = format!("{:.0}", b / 1e3)),
        _ => t!("size.b", n = bytes),
    }
    .into_owned()
}
