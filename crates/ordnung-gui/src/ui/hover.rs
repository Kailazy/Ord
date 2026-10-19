//! Tooltip / hover-text styling. egui has no per-tooltip text style, so we route
//! hover copy through a small helper so every note reads consistently. Tooltips
//! use the same proportional UI font as the body, just one shade darker than the
//! primary label so they read as quiet, supplementary chrome. Call sites use
//! [`HoverNoteExt::on_hover_note`] in place of egui's `Response::on_hover_text`;
//! [`note`] styles ad-hoc labels inside `on_hover_note_ui` closures the same
//! way. Every note is shown through [`show`], on glass; nothing calls egui's
//! `on_hover_*` directly.
//!
//! egui's tooltip is a popup frame filled from the style, an opaque grey
//! card beside every glass surface in the app. [`show`] puts the note on the
//! same material as a window or a menu: the app blurred under it (a live
//! `frost` backdrop) and one tint over that, with the window's hairline as
//! its edge. Everything else about the tooltip stays egui's: when it shows (the
//! pointer resting for the delay, the grace between neighbours), where it
//! goes, and that one with controls in it stays while the pointer is on it.
//!
//! The frame is egui's own, made transparent for the call: its fill, stroke
//! and shadow are switched off in the style while the tooltip is shown and
//! put back after, and the glass is painted in their place under the note.
//! The swap is only made when the note can show at all (the pointer is on
//! the widget, or the note was up last pass), so the hundreds of notes a
//! frame declares cost nothing.
//!
//! One frost serves every tooltip. Hovering from one widget to the next
//! keeps the surface up, so the snapshot is taken once per hover spell
//! rather than per widget; the first note of a spell sits a pass or two
//! out while it is taken, as a window does.

use super::glass;
use super::tokens::color;
use eframe::egui::{self, Color32, RichText};

/// Hover-text point size — matches the 13pt UI body.
const SIZE: f32 = 13.0;

/// Tooltip text colour: a shade darker than the primary `LABEL` (235) so notes
/// recede slightly against the body text without dropping to a faint gray.
const COLOR: Color32 = Color32::from_rgb(205, 205, 211);

/// Wrap a hover string in the tooltip style — the main UI font, one shade darker.
/// Use inside `on_hover_ui` closures: `ui.label(hover::note("…"))`.
pub fn note(text: impl Into<String>) -> RichText {
    RichText::new(text.into()).size(SIZE).color(COLOR)
}

/// Extension giving every widget a tooltip in the note style, on glass.
/// Drop-ins for egui's `on_hover_text`, `on_disabled_hover_text`,
/// `on_hover_ui` and `on_hover_ui_at_pointer`.
pub trait HoverNoteExt {
    /// `text`, styled as a note, beside the widget while it is enabled.
    fn on_hover_note(self, text: impl Into<String>) -> Self;
    /// `text`, styled as a note, beside the widget while it is disabled.
    fn on_disabled_hover_note(self, text: impl Into<String>) -> Self;
    /// A note with its own layout; style its labels with [`note`].
    fn on_hover_note_ui(self, add: impl FnOnce(&mut egui::Ui)) -> Self;
    /// [`Self::on_hover_note_ui`] at the pointer, for a widget too large to
    /// sit a note beside.
    fn on_hover_note_at_pointer(self, add: impl FnOnce(&mut egui::Ui)) -> Self;
}

impl HoverNoteExt for egui::Response {
    fn on_hover_note(self, text: impl Into<String>) -> Self {
        let text = note(text);
        self.on_hover_note_ui(|ui| {
            ui.label(text);
        })
    }

    fn on_disabled_hover_note(self, text: impl Into<String>) -> Self {
        let text = note(text);
        show(self, |resp| {
            resp.on_disabled_hover_ui(|ui| {
                on_glass(ui, |ui| {
                    ui.label(text);
                })
            })
        })
    }

    fn on_hover_note_ui(self, add: impl FnOnce(&mut egui::Ui)) -> Self {
        show(self, |resp| resp.on_hover_ui(|ui| on_glass(ui, add)))
    }

    fn on_hover_note_at_pointer(self, add: impl FnOnce(&mut egui::Ui)) -> Self {
        show(self, |resp| resp.on_hover_ui_at_pointer(|ui| on_glass(ui, add)))
    }
}

/// The one glass entry every tooltip shares (see the module docs).
fn glass_id() -> egui::Id {
    egui::Id::new("ordnung-tooltip")
}

/// The tooltip's edge: the window's hairline.
fn stroke() -> egui::Stroke {
    egui::Stroke::new(1.0, color::SEPARATOR)
}

/// `attach` adds the note to `resp` with one of egui's `on_hover_*` calls,
/// its body wrapped in [`on_glass`]; this makes egui's frame transparent
/// around that call.
fn show(resp: egui::Response, attach: impl FnOnce(egui::Response) -> egui::Response) -> egui::Response {
    let ctx = resp.ctx.clone();
    // egui decides whether the note shows; this only asks whether it could,
    // so the style swap below is made for the note or two a frame that can.
    let could = resp.contains_pointer()
        || resp.is_tooltip_open()
        || ctx.memory(|m| m.everything_is_visible());
    if !could {
        return resp;
    }
    let style = ctx.style();
    ctx.style_mut(|s| {
        s.visuals.window_fill = egui::Color32::TRANSPARENT;
        s.visuals.window_stroke = egui::Stroke::NONE;
        s.visuals.popup_shadow = egui::epaint::Shadow::NONE;
    });
    let resp = attach(resp);
    ctx.set_style(style);
    resp
}

/// The note's content with the glass under it, inside egui's (now
/// transparent) popup frame.
fn on_glass(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    let ctx = ui.ctx().clone();
    let id = glass_id();
    let slot = glass::begin(ui);
    add(ui);
    // The frame wraps the content's `min_rect` in the popup margin; the
    // glass covers the same.
    let rect = ui.min_rect() + ui.style().spacing.menu_margin;
    let rounding = ui.style().visuals.menu_rounding;
    glass::end(&ctx, slot, id, rect, rounding, stroke());
    glass::drawn(&ctx, id);
}
