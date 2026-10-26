//! The colour swatch: a dot a user picks a colour by, with a ring on the
//! current pick. One for the playlist look editor and the accent choice in
//! Settings → Appearance, so a colour is chosen the same way everywhere.

use eframe::egui;

use super::hover::HoverNoteExt;
use super::tokens::color;

/// The swatch's cell; the dot fills a little over a third of it.
const D: f32 = 22.0;

/// One swatch: `fill` as the dot, `None` as an empty ring with a slash
/// ("no colour"). `name` is the hover note. Returns whether it was clicked.
pub fn swatch(ui: &mut egui::Ui, fill: Option<egui::Color32>, name: &str, chosen: bool) -> bool {
    let d = D;
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(d, d), egui::Sense::click());
    let c = rect.center();
    match fill {
        Some(f) => {
            ui.painter().circle_filled(c, d * 0.36, f);
        }
        None => {
            // "No colour": an empty ring with a slash.
            ui.painter().circle_stroke(c, d * 0.36, egui::Stroke::new(1.5, color::LABEL_3));
            let k = d * 0.26;
            ui.painter().line_segment(
                [c + egui::vec2(-k, k), c + egui::vec2(k, -k)],
                egui::Stroke::new(1.5, color::LABEL_3),
            );
        }
    }
    if chosen || resp.hovered() {
        ui.painter().circle_stroke(
            c,
            d * 0.5,
            egui::Stroke::new(
                if chosen { 2.0 } else { 1.0 },
                if chosen { color::LABEL } else { color::LABEL_3 },
            ),
        );
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_note(name)
        .clicked()
}
