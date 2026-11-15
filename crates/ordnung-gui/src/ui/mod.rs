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
/// the body size, [`field::margin()`] included, which is 32 pt. The field is
/// the reference because it is the control whose height is least free to
/// move (its text must not clip); the buttons and pickers beside it are
/// brought to this through [`control_row`]. The text height is rounded the
/// way egui rounds a laid-out line, so the number is a whole point.
pub fn control_h(ui: &egui::Ui) -> f32 {
    control_h_in(ui.ctx())
}

/// [`control_h`] from the context alone, for layout that sizes a panel
/// before it has a `Ui` (the player reserves the cue bar's height first).
pub fn control_h_in(ctx: &egui::Context) -> f32 {
    let body = egui::TextStyle::Body.resolve(&ctx.style());
    ctx.fonts(|f| f.row_height(&body)).round() + field::margin().sum().y
}

/// A label that leads with an icon: the glyph from the app's icon face, a
/// gap, then the text in `font`, both centred on one line. For a button or
/// a menu item whose label carries a mark ("Analyze" behind its bolt), so
/// the mark is set in the icon face rather than left to the text chain,
/// where Inter's private-use glyphs would claim some of the codepoints.
/// Colours are left to the widget, so the label still answers hover.
pub fn icon_text(glyph: &str, text: &str, font: egui::FontId) -> egui::WidgetText {
    icon_text_in(glyph, text, font, egui::Color32::PLACEHOLDER)
}

/// [`icon_text`] for the common case: a button or menu label at body
/// size, the icon named as in [`phosphor_icons::app`].
pub fn icon_label(name: &str, text: &str) -> egui::WidgetText {
    icon_text(phosphor_icons::named(name), text, tokens::font::body())
}

/// [`icon_text`] in one fixed colour, for a label whose ink is its meaning
/// (a warning, a price) rather than the widget's.
pub fn icon_text_in(glyph: &str, text: &str, font: egui::FontId, color: egui::Color32) -> egui::WidgetText {
    let mut job = egui::text::LayoutJob::default();
    job.append(
        glyph,
        0.0,
        egui::TextFormat {
            font_id: tokens::font::icon(font.size + 2.0),
            color,
            valign: egui::Align::Center,
            ..Default::default()
        },
    );
    job.append(
        text,
        tokens::space::S3,
        egui::TextFormat {
            font_id: font,
            color,
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

/// A level as a filled line: `ink` over a quiet track, `frac` of `line`'s
/// width. How full a stick is, how many people have a record.
pub fn gauge(p: &egui::Painter, line: egui::Rect, frac: f32, ink: egui::Color32) {
    p.rect_filled(line, tokens::radius::XS, tokens::color::separator_opaque());
    let mut fill = line;
    fill.set_width(line.width() * frac.clamp(0.0, 1.0));
    p.rect_filled(fill, tokens::radius::XS, ink);
}

/// The ink every ghost shares: the separator grey on one slow pulse read off
/// the clock, so all the ghosts on screen breathe in phase and a wait of
/// several seconds reads as work in progress, not a hang.
pub fn ghost_ink(ui: &egui::Ui) -> egui::Color32 {
    let t = ui.input(|i| i.time);
    ui.ctx().request_repaint_after(std::time::Duration::from_millis(33));
    // 1.0 down to 0.55 and back, 1.2 s each way.
    let a = 0.775 + 0.225 * (t * std::f64::consts::PI / 1.2).cos() as f32;
    tokens::color::separator_opaque().gamma_multiply(a)
}

/// The width of ghost bar `i` in a run of them: `base` plus a spread up to
/// `span` that depends on the index alone, so the bars look like text of
/// different lengths and hold still from frame to frame.
pub fn ghost_w(i: usize, base: f32, span: usize) -> f32 {
    base + (i * 37 % span) as f32
}

/// A ghost bar `w` wide centred on `line`'s left edge and middle.
fn ghost_bar(ui: &egui::Ui, left_mid: egui::Pos2, w: f32) {
    let h = 2.0 * tokens::radius::XS;
    let bar = egui::Rect::from_min_size(left_mid - egui::vec2(0.0, h / 2.0), egui::vec2(w, h));
    ui.painter().rect_filled(bar, tokens::radius::XS, ghost_ink(ui));
}

/// A line of text that hasn't arrived: a quiet bar `w` wide at the height a
/// line of `style` takes, where the words will land. The zone keeps its
/// place while the data loads and the text replaces the bar without moving
/// anything around it.
pub fn ghost_line(ui: &mut egui::Ui, w: f32, style: egui::TextStyle) {
    let h = ui.text_style_height(&style);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
    ghost_bar(ui, rect.left_center(), w);
}

/// A list row that hasn't arrived: the blank sleeve `thumb` square and two
/// bars of text beside it, in a slot `h` tall across the width. `i` is the
/// row's place in the list and only varies the bars.
pub fn ghost_row(ui: &mut egui::Ui, h: f32, thumb: f32, i: usize) {
    let (slot, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), h), egui::Sense::hover());
    let pad = (h - thumb) / 2.0;
    let art = egui::Rect::from_min_size(slot.min + egui::vec2(pad, pad), egui::vec2(thumb, thumb));
    cover::paint_at(ui, art, None::<&egui::TextureHandle>, tokens::radius::XS, None);
    let x = art.right() + tokens::space::S3;
    ghost_bar(ui, egui::pos2(x, slot.center().y - 9.0), ghost_w(i, 150.0, 90));
    ghost_bar(ui, egui::pos2(x, slot.center().y + 8.0), ghost_w(i + 2, 90.0, 70));
}

/// A wall of covers that hasn't arrived: `rows` rows of blank sleeves at the
/// wall's own fit, each over `lines` caption bars in `caption_h`.
pub fn ghost_wall(ui: &mut egui::Ui, caption_h: f32, lines: usize, rows: usize) {
    let (cols, side) = cover::wall_fit(ui.available_width());
    ui.spacing_mut().item_spacing = egui::vec2(cover::WALL_GAP, cover::WALL_GAP);
    for row in 0..rows {
        ui.horizontal_top(|ui| {
            for col in 0..cols {
                let (cell, _) =
                    ui.allocate_exact_size(egui::vec2(side, side + caption_h), egui::Sense::hover());
                let art = egui::Rect::from_min_size(cell.min, egui::vec2(side, side));
                cover::paint_at(ui, art, None::<&egui::TextureHandle>, 6.0, None);
                for l in 0..lines {
                    let at = egui::pos2(cell.left(), art.bottom() + 12.0 + 14.0 * l as f32);
                    ghost_bar(ui, at, ghost_w(row * cols + col + 2 * l, 60.0, 60));
                }
            }
        });
    }
}

/// The chip that floats at the pointer while something is dragged inside
/// the app ("3 tracks", "1 song"), so it's clear something is being
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
        tokens::color::nav_accent(),
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

/// A line of text that carries a link or an inline control, laid out at the
/// line's height. `ui.horizontal` assumes something interactive is coming and
/// makes the row `interact_size` tall, which sets the words 3 pt low and
/// spaces such a line apart from the plain labels around it.
pub fn text_line<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let h = ui.text_style_height(&egui::TextStyle::Body);
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), h),
        egui::Layout::left_to_right(egui::Align::Center),
        add,
    )
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
