//! Ordnung's visual component library.
//!
//! [`tokens`] holds the design tokens (colours, radii, spacing, type ramp) and
//! [`theme`] pushes them into egui's global style. The components sit beside
//! them: [`button`] is the one push button, [`field`] the one text box,
//! [`chip`] the avatar pill, [`cover`] the record's sleeve or its stand-in, [`menu`] the dropdown a button anchors,
//! [`window`] the one floating window, [`sidebar`] the window docked to an
//! edge, [`nav`] the docked panel that snaps between designed widths,
//! [`hover`] the hover note, [`choices`] the row of buttons a dialog
//! ends with, [`glass`] the surface they all sit on, [`control_row`] the
//! rule that a row of controls shares one height.

pub mod button;
pub mod chip;
pub mod choices;
pub mod cover;
pub mod escape;
pub mod fader;
pub mod field;
pub mod frost;
pub mod glass;
pub mod hover;
pub mod icon;
pub mod knob;
pub mod menu;
pub mod nav;
pub mod phosphor_icons;
pub mod sheet;
pub mod sidebar;
pub mod swatch;
pub mod text;
pub mod theme;
pub mod tokens;
pub mod window;

use eframe::egui;

/// The one height every control on a row shares: a single-line text field at
/// the body size, [`field::MARGIN`] included, which is 32 pt. The field is
/// the reference because it is the control whose height is least free to
/// move (its text must not clip); the buttons and pickers beside it are
/// brought to this through [`control_row`]. The text height is rounded the
/// way egui rounds a laid-out line, so the number is a whole point.
pub fn control_h(ui: &egui::Ui) -> f32 {
    ui.text_style_height(&egui::TextStyle::Body).round() + field::MARGIN.sum().y
}

/// A label that leads with an icon: the glyph from the app's icon face, a
/// gap, then the text in `font`, both centred on one line. For a button or
/// a menu item whose label carries a mark ("Analyze" behind its bolt), so
/// the mark is set in the icon face rather than left to the text chain,
/// where Inter's private-use glyphs would claim some of the codepoints.
/// Colours are left to the widget, so the label still answers hover.
pub fn icon_text(glyph: &str, text: &str, font: egui::FontId) -> egui::WidgetText {
    let mut job = egui::text::LayoutJob::default();
    job.append(
        glyph,
        0.0,
        egui::TextFormat {
            font_id: tokens::font::icon(font.size + 2.0),
            color: egui::Color32::PLACEHOLDER,
            valign: egui::Align::Center,
            ..Default::default()
        },
    );
    job.append(
        text,
        tokens::space::S3,
        egui::TextFormat {
            font_id: font,
            color: egui::Color32::PLACEHOLDER,
            valign: egui::Align::Center,
            ..Default::default()
        },
    );
    job.into()
}

/// A small info mark beside a header, with `text` as its hover note: the
/// short explanation for a setting whose label alone cannot carry it. A
/// setting whose label explains itself gets no note at all.
pub fn info_note(ui: &mut egui::Ui, text: impl Into<String>) {
    use hover::HoverNoteExt;
    ui.add(
        egui::Label::new(
            egui::RichText::new(phosphor_icons::named(phosphor_icons::app::INFO))
                .font(tokens::font::icon(tokens::font::body().size + 2.0))
                .color(icon::REST),
        )
        .selectable(false),
    )
    .on_hover_note(text);
}

/// The chip that floats at the pointer while something is dragged inside
/// the app ("3 track(s)", "1 song"), so it's clear something is being
/// carried toward a drop target. Painted in the tooltip layer, above all.
pub fn drag_chip(ctx: &egui::Context, text: String) {
    let Some(pos) = ctx.pointer_interact_pos() else {
        return;
    };
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Tooltip,
        egui::Id::new("drag-preview"),
    ));
    let at = pos + egui::vec2(14.0, 6.0);
    let galley = painter.layout_no_wrap(text, tokens::font::callout(), egui::Color32::WHITE);
    let pad = egui::vec2(6.0, 3.0);
    let rect = egui::Rect::from_min_size(at, galley.size() + pad * 2.0);
    painter.rect_filled(
        rect,
        egui::Rounding::same(4.0),
        egui::Color32::from_rgb(60, 110, 170),
    );
    painter.galley(at + pad, galley, egui::Color32::WHITE);
}

/// Lay out a row of controls at one height. Every `Button`, `ComboBox`,
/// glyph and single-line `TextEdit` added inside comes out [`control_h`]
/// tall, so a row never shows three neighbouring controls at three heights.
/// A push button on its own is shorter than a field; here its padding grows
/// so it meets the field. Use the plain `ui.button` inside, not
/// `small_button`: the small variant sets its own text size and would
/// break the line it sits on.
pub fn control_row<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let h = control_h(ui);
    // Rounded like the line egui lays out, or the padding would carry the
    // fraction and the button land a hair past `h`.
    let text_h = ui.text_style_height(&egui::TextStyle::Button).round();
    ui.scope(|ui| {
        let s = ui.spacing_mut();
        s.interact_size.y = h;
        s.button_padding.y = ((h - text_h) / 2.0).max(0.0);
        add(ui)
    })
    .inner
}

/// Carry a striped table's shading on under its last row: the even rows
/// the table would shade, continued through the blank space to the bottom
/// of the view, so a short list still reads as rows. Call it in the body
/// before `rows`, with the same row height and count; it paints nothing
/// once the rows reach the bottom.
pub fn blank_stripes(body: &mut egui_extras::TableBody<'_>, row_h: f32, rows: usize) {
    let spacing = body.ui_mut().spacing().item_spacing;
    // The width a row paints: every column at its width, the gaps between,
    // and half a gap past each edge, as egui_extras paints its stripes.
    let widths = body.widths();
    let width = widths.iter().sum::<f32>() + spacing.x * widths.len().saturating_sub(1) as f32;
    let ui = body.ui_mut();
    let rect = ui.available_rect_before_wrap();
    let fill = ui.visuals().faint_bg_color;
    let pitch = row_h + spacing.y;
    let x = egui::Rangef::new(rect.left() - 0.5 * spacing.x, rect.left() + width + 0.5 * spacing.x);
    let tops = (rows + rows % 2..).step_by(2).map(|i| rect.top() + i as f32 * pitch);
    for top in tops.take_while(|&top| top < rect.bottom()) {
        let y = egui::Rangef::new(top - 0.5 * spacing.y, top + row_h + 0.5 * spacing.y);
        ui.painter().rect_filled(egui::Rect::from_x_y_ranges(x, y), 0.0, fill);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Three rows in a view with room for many: the shading continues at
    /// the row pitch, on the even rows only, and stops at the view's
    /// bottom.
    #[test]
    fn blank_stripes_continue_the_rows_to_the_bottom() {
        use egui_extras::{Column, TableBuilder};
        const ROW_H: f32 = 10.0;
        const ROWS: usize = 3;
        let fill = egui::Color32::from_rgb(1, 2, 3);
        let ctx = egui::Context::default();
        ctx.style_mut(|s| s.visuals.faint_bg_color = fill);
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(200.0, 300.0))),
            ..Default::default()
        };
        let mut first_top = 0.0;
        let mut pitch = 0.0;
        let mut bottom = 0.0;
        let out = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                TableBuilder::new(ui)
                    .striped(true)
                    .column(Column::remainder())
                    .body(|mut body| {
                        let rect = body.ui_mut().available_rect_before_wrap();
                        first_top = rect.top();
                        bottom = rect.bottom();
                        pitch = ROW_H + body.ui_mut().spacing().item_spacing.y;
                        blank_stripes(&mut body, ROW_H, ROWS);
                        body.rows(ROW_H, ROWS, |mut row| {
                            // An empty cell: content taller than the row
                            // would stretch it.
                            row.col(|_| {});
                        });
                    });
            });
        });
        let mut tops: Vec<f32> = out
            .shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::Shape::Rect(r) if r.fill == fill => Some(r.rect.top()),
                _ => None,
            })
            .collect();
        tops.sort_by(f32::total_cmp);
        // Every stripe sits on an even row, the table's own two first,
        // then one per even row until the view ends.
        let expect: Vec<f32> = (0..)
            .step_by(2)
            .map(|i| first_top + i as f32 * pitch)
            .take_while(|&top| top < bottom)
            .collect();
        assert!(expect.len() > 2, "the view had room for more rows: {expect:?}");
        assert_eq!(tops.len(), expect.len(), "tops {tops:?} expected {expect:?}");
        for (got, want) in tops.iter().zip(&expect) {
            // A stripe reaches half a gap above its row.
            assert!((got - (want - 0.5 * (pitch - ROW_H))).abs() < 0.5, "tops {tops:?} expected {expect:?}");
        }
    }
}
