//! The cover thumbnail: a record's art in a rounded square, or the blank
//! square with a note where no art has arrived. One drawing for every
//! row and card that leads with a sleeve, so a missing cover looks the
//! same in a crate, the liked songs and a tracklist.

use eframe::egui;

/// A cover thumb `side` pt square, or the blank square with a note where
/// there is none. Takes anything that converts to an image source, so a
/// cached texture handle or `&Tex` both fit.
pub fn thumb<'a>(ui: &mut egui::Ui, side: f32, tex: Option<impl Into<egui::ImageSource<'a>>>) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::hover());
    paint_at(ui, rect, tex, 4.0, Some(side * 0.45));
}

/// The art in `rect` the caller laid out, or the blank square where there
/// is none; `note` puts the ♪ on the blank at that font size.
pub fn paint_at<'a>(
    ui: &egui::Ui,
    rect: egui::Rect,
    tex: Option<impl Into<egui::ImageSource<'a>>>,
    rounding: f32,
    note: Option<f32>,
) {
    match tex {
        Some(h) => {
            egui::Image::new(h)
                .fit_to_exact_size(rect.size())
                .rounding(egui::Rounding::same(rounding))
                .paint_at(ui, rect);
        }
        None => {
            ui.painter().rect_filled(rect, egui::Rounding::same(rounding), egui::Color32::from_gray(34));
            if let Some(size) = note {
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "♪",
                    egui::FontId::proportional(size),
                    egui::Color32::from_gray(70),
                );
            }
        }
    }
}
