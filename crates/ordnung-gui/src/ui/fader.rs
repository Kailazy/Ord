//! The player's tempo control: the BPM itself, CDJ style.
//!
//! No slider: the playing tempo is the control. It reads out large, the
//! pitch percent under it, and the hand changes it right there: scroll or
//! drag up and down over the number, tap the arrows beside it for a 0.1%
//! nudge, double-click for the track's own tempo, or click it and type the
//! tempo wanted (Enter or a click away sets it, Escape keeps the old one).
//! A track without a tempo shows the percent large instead, and takes a
//! typed percent.
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

    // A click on the number opens it for typing; the text lives in egui's
    // memory while the edit is open.
    let edit_id = resp.id.with("edit");
    let bpm = bpm.filter(|b| *b > 0.0);
    let untouched = match bpm {
        Some(bpm) => format!("{:.2}", bpm * (1.0 + value / 100.0)),
        None => format!("{value:.1}"),
    };
    let mut typing = ui.data(|d| d.get_temp::<String>(edit_id));
    let opened = typing.is_none() && resp.clicked();
    if opened {
        typing = Some(untouched.clone());
    }

    let mut changed = None;
    if up_r.clicked() {
        changed = Some(clamp(value + STEP_PCT));
    } else if down_r.clicked() {
        changed = Some(clamp(value - STEP_PCT));
    }
    if typing.is_none() && resp.dragged() {
        let dy = resp.drag_delta().y;
        if dy != 0.0 {
            // Up is faster, like pushing a record along.
            changed = Some((value - dy * DRAG_PCT_PER_PT).clamp(-range, range));
        }
    }
    if typing.is_none() && (resp.hovered() || up_r.hovered() || down_r.hovered()) {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll != 0.0 {
            changed = Some(clamp(changed.unwrap_or(value) + scroll.signum() * STEP_PCT));
        }
    }
    if resp.double_clicked() {
        changed = Some(0.0);
    }

    let shown = changed.unwrap_or(value);
    let active = typing.is_none() && (resp.hovered() || resp.dragged());
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

    if let Some(mut text) = typing {
        // The field takes the number's place, in the number's own type, and
        // a faint accent outline marks the control as open.
        let type_font = font::strong(font::title().size);
        let pad_y = ((H - ui.fonts(|f| f.row_height(&type_font))) / 2.0).floor();
        let area = egui::Rect::from_min_max(rect.min, egui::pos2(arrows.left(), rect.bottom()));
        // A child, not `ui.put`: the control's space is already taken, and
        // `put` would pull the row's cursor back to the field's right edge.
        let field = ui.new_child(egui::UiBuilder::new().max_rect(area)).add(
            super::field::Field::singleline(&mut text)
                .quiet()
                .id(edit_id)
                .font(type_font)
                .align(egui::Align::RIGHT)
                .margin(egui::Margin::symmetric(4.0, pad_y))
                .width(area.width() - 8.0),
        );
        ui.painter().rect_stroke(
            rect,
            egui::Rounding::same(radius::SM),
            egui::Stroke::new(1.0, color::accent().gamma_multiply(0.5)),
        );
        if opened {
            super::field::focus_select_all(ui.ctx(), &field, &text);
        }
        // The second click of a double-click lands on the field just opened:
        // still a reset, as long as nothing was typed.
        let reset = field.double_clicked() && text == untouched;
        if reset {
            changed = Some(0.0);
        } else if field.lost_focus() && !ui.input(|i| i.key_pressed(egui::Key::Escape)) && text != untouched {
            if let Some(v) = typed_percent(&text, bpm) {
                changed = Some(v.clamp(-range, range));
            }
        }
        if reset || field.lost_focus() {
            ui.data_mut(|d| d.remove::<String>(edit_id));
        } else {
            ui.data_mut(|d| d.insert_temp(edit_id, text));
        }
        return changed;
    }

    let x = arrows.left() - 4.0;
    let pct = format!("{shown:+.1}%");
    let pct_ink = if shown == 0.0 { color::LABEL_3 } else { color::ORANGE };
    match bpm {
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
    let _ = resp.on_hover_note("Click to type a tempo. Scroll or drag to change it. Double-click resets.");
    changed
}

/// The percent a typed number asks for: a tempo against the track's `bpm`,
/// or the percent itself when the track has none. Takes a decimal comma
/// and a trailing `%`.
fn typed_percent(text: &str, bpm: Option<f32>) -> Option<f32> {
    let n: f32 = text.trim().trim_end_matches('%').trim().replace(',', ".").parse().ok()?;
    let pct = match bpm {
        Some(bpm) => (n - bpm) / bpm * 100.0,
        None => n,
    };
    pct.is_finite().then_some(pct)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_number_becomes_a_percent() {
        assert_eq!(typed_percent("126", Some(120.0)), Some(5.0));
        assert_eq!(typed_percent(" 120,0 ", Some(120.0)), Some(0.0));
        assert_eq!(typed_percent("-2.5%", None), Some(-2.5));
        assert_eq!(typed_percent("fast", Some(120.0)), None);
        assert_eq!(typed_percent("", None), None);
    }

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

    /// One pass over the control at 0% on a 120 BPM track.
    fn pass(ctx: &egui::Context, events: Vec<egui::Event>) -> Option<f32> {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 200.0))),
            events,
            focused: true,
            ..Default::default()
        };
        let mut out = None;
        let _ = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| out = pitch(ui, 0.0, 8.0, Some(120.0)));
        });
        out
    }

    fn click() -> [Vec<egui::Event>; 2] {
        let pos = egui::pos2(40.0, 25.0);
        let button = |pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        [vec![egui::Event::PointerMoved(pos), button(true)], vec![button(false)]]
    }

    fn key(key: egui::Key) -> Vec<egui::Event> {
        vec![egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }]
    }

    /// A click opens the number with its text selected, so typing replaces
    /// it; Enter sets the typed tempo, Escape keeps the old one.
    #[test]
    fn click_type_enter_sets_the_tempo() {
        for (end, want) in [(egui::Key::Enter, Some(5.0)), (egui::Key::Escape, None)] {
            let ctx = egui::Context::default();
            crate::ui::theme::install(&ctx);
            pass(&ctx, vec![]);
            for events in click() {
                assert_eq!(pass(&ctx, events), None);
            }
            pass(&ctx, vec![]);
            assert_eq!(pass(&ctx, vec![egui::Event::Text("126".into())]), None);
            assert_eq!(pass(&ctx, key(end)), want);
            // Closed again: the next pass is the plain readout.
            assert_eq!(pass(&ctx, vec![]), None);
            assert!(!ctx.wants_keyboard_input());
        }
    }
}
