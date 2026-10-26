//! Settings → Appearance: the look presets, the table row shade and the
//! accent. Split out of `modals.rs`; part of the GUI `App`.
use super::*;
use crate::ui::hover::HoverNoteExt;
use crate::ui::swatch::{swatch, swatch_add};
use crate::ui::tokens::{color, font, radius, space};

impl App {
    pub(crate) fn draw_appearance_tab(&mut self, ui: &mut egui::Ui) {
        let mut dirty = false;

        heading(
            ui,
            "Look",
            "The app background and the accent that suits it, as a set. Pick \
             one, then tune the accent below.",
        );
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = space::S3;
            let (bg, accent) = (color::bg(), color::accent());
            for look in color::LOOKS {
                let chosen = look.background == bg && look.accent == accent;
                if look_tile(ui, look, self.config.row_shade, chosen).clicked() {
                    self.config.background = stored(look.background, color::BG_STOCK);
                    self.config.accent = stored(look.accent, color::ACCENT_STOCK);
                    dirty = true;
                }
            }
        });

        divider(ui);
        heading(
            ui,
            "Table rows",
            "Every other row of a song or record table is a touch darker, so a \
             wide row can be followed across its columns.",
        );
        dirty |= ui
            .checkbox(&mut self.config.zebra_rows, "Shade alternating rows")
            .changed();
        ui.add_enabled_ui(self.config.zebra_rows, |ui| {
            egui::Grid::new("settings_appearance_grid")
                .num_columns(2)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    ui.label("Shade:");
                    dirty |= ui
                        .add(egui::Slider::new(&mut self.config.row_shade, 0..=120).show_value(false))
                        .on_hover_note("How much darker the shaded rows are")
                        .changed();
                    ui.end_row();
                });
        });

        divider(ui);
        heading(
            ui,
            "Accent",
            "The colour of selections, links and the primary button. The \
             presets first; the picker sets any colour, and + keeps it as a \
             preset.",
        );
        let current = color::accent();
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = space::S2;
            if swatch(ui, Some(color::ACCENT_STOCK), "Blue (default)", current == color::ACCENT_STOCK)
                .clicked()
            {
                self.config.accent.clear();
                dirty = true;
            }
            for (name, c) in color::TAG_PALETTE {
                if swatch(ui, Some(*c), name, current == *c).clicked() {
                    self.config.accent = color::to_hex(*c);
                    dirty = true;
                }
            }
            let mut remove = None;
            for (i, hex) in self.config.accent_presets.iter().enumerate() {
                let Some(c) = color::from_hex(hex) else { continue };
                let resp = swatch(ui, Some(c), "Saved colour. Right-click removes it", current == c);
                if resp.clicked() {
                    self.config.accent = hex.clone();
                    dirty = true;
                }
                if resp.secondary_clicked() {
                    remove = Some(i);
                }
            }
            if let Some(i) = remove {
                self.config.accent_presets.remove(i);
                dirty = true;
            }
            let kept = self.config.accent_presets.iter().any(|h| color::from_hex(h) == Some(current));
            if !kept && swatch_add(ui, "Keep this colour as a preset").clicked() {
                self.config.accent_presets.push(color::to_hex(current));
                dirty = true;
            }
        });
        ui.add_space(space::S3);
        // egui's own picker: hue strip and a saturation/value square. Wider
        // than its stock 100 pt, so the square is a real target.
        let mut picked = current;
        ui.scope(|ui| {
            ui.spacing_mut().slider_width = 220.0;
            if egui::color_picker::color_picker_color32(ui, &mut picked, egui::color_picker::Alpha::Opaque) {
                self.config.accent = stored(picked, color::ACCENT_STOCK);
                dirty = true;
            }
        });

        if dirty {
            self.config.apply_appearance(ui.ctx());
            self.save_config();
        }
    }
}

/// How the config stores a colour: empty for the stock one, else `#rrggbb`.
fn stored(c: egui::Color32, stock: egui::Color32) -> String {
    if c == stock {
        String::new()
    } else {
        color::to_hex(c)
    }
}

/// A settings section's title and the line under it, as the other tabs
/// set them.
fn heading(ui: &mut egui::Ui, title: &str, note: &str) {
    ui.label(egui::RichText::new(title).strong());
    ui.label(egui::RichText::new(note).small().weak());
    ui.add_space(4.0);
}

fn divider(ui: &mut egui::Ui) {
    ui.add_space(14.0);
    ui.separator();
    ui.add_space(10.0);
}

/// A look's preview, the name under it: the sidebar on the base tone with
/// its selected tile, the content area with its shaded rows, the primary
/// button in the accent. A ring marks the current look.
fn look_tile(ui: &mut egui::Ui, look: &color::Look, row_shade: u8, chosen: bool) -> egui::Response {
    const W: f32 = 88.0;
    const H: f32 = 56.0;
    const SIDE: f32 = 24.0;
    let resp = ui
        .vertical(|ui| {
            ui.set_width(W);
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(W, H), egui::Sense::click());
            let p = ui.painter().clone();
            let r = rect.shrink(2.0);
            let round = radius::SM;
            let side = egui::Rect::from_min_size(r.min, egui::vec2(SIDE, r.height()));
            let content = egui::Rect::from_min_max(egui::pos2(side.right(), r.top()), r.max);
            p.rect_filled(side, egui::Rounding { nw: round, sw: round, ne: 0.0, se: 0.0 }, look.background);
            p.rect_filled(
                egui::Rect::from_min_size(side.min + egui::vec2(4.0, 6.0), egui::vec2(SIDE - 8.0, 6.0)),
                2.0,
                color::accent_soft_of(look.accent, look.background),
            );
            p.rect_filled(
                egui::Rect::from_min_size(side.min + egui::vec2(4.0, 16.0), egui::vec2(SIDE - 8.0, 6.0)),
                2.0,
                color::surface_of(look.background),
            );
            p.rect_filled(content, egui::Rounding { ne: round, se: round, nw: 0.0, sw: 0.0 }, color::content_bg_of(look.background));
            let row_h = content.height() / 5.0;
            for i in [1, 3] {
                let row = egui::Rect::from_min_size(
                    content.min + egui::vec2(0.0, i as f32 * row_h),
                    egui::vec2(content.width(), row_h),
                );
                p.rect_filled(row, 0.0, egui::Color32::from_black_alpha(row_shade));
            }
            p.rect_filled(
                egui::Rect::from_min_size(content.min + egui::vec2(6.0, 3.0), egui::vec2(20.0, row_h - 6.0)),
                2.0,
                color::nav_accent_of(look.accent, look.background),
            );
            if chosen || resp.hovered() {
                p.rect_stroke(
                    rect,
                    round + 2.0,
                    egui::Stroke::new(
                        if chosen { 2.0 } else { 1.0 },
                        if chosen { color::LABEL } else { color::LABEL_3 },
                    ),
                );
            }
            ui.vertical_centered(|ui| {
                ui.label(
                    egui::RichText::new(look.name)
                        .font(font::caption())
                        .color(if chosen { color::LABEL } else { color::LABEL_2 }),
                );
            });
            resp
        })
        .inner;
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}
