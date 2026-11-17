//! The player's tempo control: the BPM itself, CDJ style.
//!
//! No slider: the playing tempo is the control. It reads out large, the
//! pitch percent under it, and the hand changes it right there: scroll or
//! drag up and down over the number, tap the arrows beside it for a 0.1%
//! nudge, double-click for the track's own tempo. A track without a tempo
//! shows the percent large instead.
//!
//! The control only reports; how the rate is applied (turntable or key lock)
//! is the engine's business (see `audio::PitchState`).

use super::hover::HoverNoteExt;
use super::tokens::{color, font, radius};

/// Footprint: as tall as the transport's disc beside it.
pub const W: f32 = 120.0;
const H: f32 = 40.0;
/// The nudge arrows' column on the right.
const ARROWS_W: f32 = 18.0;
/// Step applied per scroll-wheel notch or arrow tap, in percent.
const STEP_PCT: f32 = 0.1;
/// Percent per point of vertical drag: a full ±8% is a 320 pt sweep.
const DRAG_PCT_PER_PT: f32 = 0.05;

/// Draw the tempo control at `value` percent over `±range` and return the
/// new value when the user changed it, or `None`. `bpm` is the track's
/// tempo at zero; `None` shows the percent alone.
pub fn pitch(ui: &mut egui::Ui, value: f32, range: f32, bpm: Option<f32>) -> Option<f32> {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(W, H), egui::Sense::click_and_drag());
    let range = range.max(0.1);
    let value = value.clamp(-range, range);
    let clamp = |v: f32| ((v * 10.0).round() / 10.0).clamp(-range, range);

    let arrows = egui::Rect::from_min_max(egui::pos2(rect.right() - ARROWS_W, rect.top()), rect.max);
    let up = egui::Rect::from_min_max(arrows.min, egui::pos2(arrows.right(), arrows.center().y));
    let down = egui::Rect::from_min_max(egui::pos2(arrows.left(), arrows.center().y), arrows.max);
    let up_r = ui.interact(up.shrink(1.0), resp.id.with("up"), egui::Sense::click());
    let down_r = ui.interact(down.shrink(1.0), resp.id.with("down"), egui::Sense::click());

    let mut changed = None;
    if up_r.clicked() {
        changed = Some(clamp(value + STEP_PCT));
    } else if down_r.clicked() {
        changed = Some(clamp(value - STEP_PCT));
    }
    if resp.dragged() {
        let dy = resp.drag_delta().y;
        if dy != 0.0 {
            // Up is faster, like pushing a record along.
            changed = Some((value - dy * DRAG_PCT_PER_PT).clamp(-range, range));
        }
    }
    if resp.hovered() || up_r.hovered() || down_r.hovered() {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll != 0.0 {
            changed = Some(clamp(changed.unwrap_or(value) + scroll.signum() * STEP_PCT));
        }
    }
    if resp.double_clicked() {
        changed = Some(0.0);
    }

    let shown = changed.unwrap_or(value);
    let active = resp.hovered() || resp.dragged();
    let p = ui.painter();
    p.rect_filled(
        rect,
        egui::Rounding::same(radius::SM),
        if active { color::surface_hover() } else { color::field() },
    );
    for (r, h, pointing_up) in [(up, &up_r, true), (down, &down_r, false)] {
        if h.hovered() {
            p.rect_filled(r.shrink(1.0), egui::Rounding::same(radius::XS), color::surface_active());
        }
        let c = r.center();
        let d = if pointing_up { -2.5 } else { 2.5 };
        p.add(egui::Shape::convex_polygon(
            vec![
                egui::pos2(c.x - 3.5, c.y - d * 0.6),
                egui::pos2(c.x, c.y + d),
                egui::pos2(c.x + 3.5, c.y - d * 0.6),
            ],
            if h.hovered() { color::LABEL } else { color::LABEL_3 },
            egui::Stroke::NONE,
        ));
    }

    let x = arrows.left() - 4.0;
    let pct = format!("{shown:+.1}%");
    let pct_ink = if shown == 0.0 { color::LABEL_3 } else { color::ORANGE };
    match bpm.filter(|b| *b > 0.0) {
        Some(bpm) => {
            p.text(
                egui::pos2(x, rect.top() + 3.0),
                egui::Align2::RIGHT_TOP,
                format!("{:.2}", bpm * (1.0 + shown / 100.0)),
                font::strong(font::title().size),
                color::LABEL,
            );
            p.text(
                egui::pos2(x, rect.bottom() - 3.0),
                egui::Align2::RIGHT_BOTTOM,
                pct,
                font::mono_small(),
                pct_ink,
            );
        }
        None => {
            p.text(
                egui::pos2(x, rect.center().y),
                egui::Align2::RIGHT_CENTER,
                pct,
                font::strong(font::headline().size),
                pct_ink,
            );
        }
    }

    if active {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
    }
    let _ = resp.on_hover_note("Scroll or drag to change tempo. Double-click resets.");
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The control paints and reports nothing when untouched, at any value.
    #[test]
    fn idle_tempo_reports_no_change() {
        let ctx = egui::Context::default();
        crate::ui::theme::install(&ctx);
        for v in [-8.0, -0.1, 0.0, 3.3, 8.0] {
            let _ = ctx.run(Default::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    assert_eq!(pitch(ui, v, 8.0, Some(120.0)), None);
                });
            });
        }
    }
}
