//! One line of text cut to a width with an ellipsis, for painting where a
//! label can't go: a chip's name, a caption, a menu row, a tracklist's bar.

use eframe::egui;
use std::sync::Arc;

/// `text` on one line in `font` and `ink`, cut to `max_w` with an ellipsis.
/// `break_anywhere` cuts inside a word; otherwise the cut falls at the last
/// word boundary that fits.
pub fn truncated(ui: &egui::Ui, text: impl Into<String>, font: egui::FontId, ink: egui::Color32, max_w: f32, break_anywhere: bool) -> Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple_singleline(text.into(), font, ink);
    job.wrap.max_width = max_w.max(0.0);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = break_anywhere;
    ui.fonts(|f| f.layout_job(job))
}
