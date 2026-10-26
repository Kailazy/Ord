//! The colour swatch: a dot a user picks a colour by, with a ring on the
//! current pick. One for the playlist look editor and the accent choice in
//! Settings → Appearance, so a colour is chosen the same way everywhere.

use eframe::egui;

use super::hover::HoverNoteExt;
use super::tokens::color;

/// The swatch's cell; the dot fills a little over a third of it.
const D: f32 = 22.0;

/// One swatch: `fill` as the dot, `None` as an empty ring with a slash
/// ("no colour"). `name` is the hover note.
pub fn swatch(ui: &mut egui::Ui, fill: Option<egui::Color32>, name: &str, chosen: bool) -> egui::Response {
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
    ring(ui, c, chosen, resp.hovered());
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_note(name)
}

/// The swatch that keeps a colour: an empty ring with a plus, at the end
/// of a row of presets. `name` is the hover note.
pub fn swatch_add(ui: &mut egui::Ui, name: &str) -> egui::Response {
    let d = D;
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(d, d), egui::Sense::click());
    let c = rect.center();
    ui.painter().circle_stroke(c, d * 0.36, egui::Stroke::new(1.5, color::LABEL_3));
    let k = d * 0.18;
    let stroke = egui::Stroke::new(1.5, color::LABEL_3);
    ui.painter().line_segment([c - egui::vec2(k, 0.0), c + egui::vec2(k, 0.0)], stroke);
    ui.painter().line_segment([c - egui::vec2(0.0, k), c + egui::vec2(0.0, k)], stroke);
    ring(ui, c, false, resp.hovered());
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_note(name)
}

/// The ring round the current pick, or a fainter one under the pointer.
fn ring(ui: &egui::Ui, c: egui::Pos2, chosen: bool, hovered: bool) {
    if chosen || hovered {
        ui.painter().circle_stroke(
            c,
            D * 0.5,
            egui::Stroke::new(
                if chosen { 2.0 } else { 1.0 },
                if chosen { color::LABEL } else { color::LABEL_3 },
            ),
        );
    }
}
