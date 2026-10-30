//! Animated dropdown menu — Ordnung's popover for pickers and small action
//! menus, replacing egui's stock `menu_button` where the chrome deserves the
//! app's own motion language.
//!
//! The motion mirrors the search suggestion popup so every transient surface in
//! the app moves the same way: the panel rises its last few points into place
//! while fading in (cubic-out, reversed on close), and rows cascade top-down on
//! a short stagger instead of appearing all at once. Hover highlights ease in
//! and out rather than snapping. All of it is driven by egui's animation clock,
//! so a dismissed menu fades away from wherever its open animation had reached.
//!
//! Usage: draw any anchor widget, then attach the menu to its response.
//!
//! ```ignore
//! let btn = ui.button("Sort");
//! menu::dropdown(&btn, 170.0, |m| {
//!     if m.selectable(current == Sort::Artist, "Artist") {
//!         current = Sort::Artist;
//!         m.close();
//!     }
//!     m.separator();
//!     if m.item("Reset") { /* … */ }
//! });
//! ```
//!
//! Clicking the anchor toggles the menu; Esc or a click anywhere else dismisses
//! it. Rows that should dismiss on selection call [`MenuUi::close`] — rows that
//! toggle (multi-select filters) simply don't, and the menu stays up.

use eframe::egui;

use super::tokens::{color, font, radius, space};

/// Open/close transition length, matched to the search popup's `POPUP_ANIM`.
const ANIM: f32 = 0.14;
/// How far the panel rises into place while fading in.
const RISE: f32 = 6.0;
/// Gap between the anchor's bottom edge and the panel.
const GAP: f32 = 4.0;
/// One row's entrance fade/slide length.
const ROW_ANIM: f32 = 0.16;
/// Delay between successive rows' entrances — the cascade.
const ROW_STAGGER: f32 = 0.02;
/// Hover highlight ease, matched to the app's row highlights.
const HOVER_ANIM: f32 = 0.11;
/// Height of one menu row.
const ROW_H: f32 = 26.0;
/// Width reserved for the ✓ column in selectable rows, so labels align whether
/// checked or not (the Apple menu convention).
const CHECK_W: f32 = 18.0;
/// Horizontal text inset inside a row.
const ROW_PAD: f32 = space::S3;
/// Gap between the columns of a [`MenuUi::columns`] run.
const COL_GAP: f32 = space::S2;
/// The panel's drop shadow, shared with egui's own menus (see [`context`]).
const SHADOW: egui::epaint::Shadow = egui::epaint::Shadow {
    offset: egui::vec2(0.0, 6.0),
    blur: 22.0,
    spread: 0.0,
    color: egui::Color32::from_black_alpha(90),
};

/// Attach an animated dropdown to `anchor`. Clicking the anchor toggles it;
/// `width` is the panel's fixed content width. Fixed, not a minimum: inside an
/// `Area` the "available" width is the rest of the screen, so any row that
/// filled it would drag the panel out to the window edge. A menu also shouldn't
/// resize as rows come and go (a filter count appearing, a section toggling).
pub fn dropdown(anchor: &egui::Response, width: f32, add: impl FnOnce(&mut MenuUi)) {
    dropdown_from(anchor, width, false, add);
}

/// A [`dropdown`] hung from the anchor's right edge instead of its left: for
/// an anchor at the right edge of its window, where a panel hanging from the
/// left edge would run past the window.
pub fn dropdown_right(anchor: &egui::Response, width: f32, add: impl FnOnce(&mut MenuUi)) {
    dropdown_from(anchor, width, true, add);
}

fn dropdown_from(
    anchor: &egui::Response,
    width: f32,
    right: bool,
    add: impl FnOnce(&mut MenuUi),
) {
    let ctx = anchor.ctx.clone();
    let id = anchor.id.with("ord_dropdown");
    let mut open: bool = ctx.data(|d| d.get_temp(id).unwrap_or(false));
    let now = ctx.input(|i| i.time);
    if anchor.clicked() {
        open = !open;
        if open {
            // Stamp the open moment: every row times its entrance off this.
            ctx.data_mut(|d| d.insert_temp(id.with("at"), now));
        }
    }
    // Dismissal context, sampled before the content draws so this frame's own
    // interactions can't muddy it: a focused text field means Esc is aimed at
    // the field (unfocus), not the menu; an open popup (a combo box inside the
    // menu) means a click on one of its options — which lands outside this
    // Area — must not read as click-away.
    let field_focused = ctx.memory(|m| m.focused().is_some());
    let nested_popup_open = ctx.memory(|m| m.any_popup_open());
    let Some(panel) = draw_panel(&ctx, anchor, id, open, width, right, add) else {
        ctx.data_mut(|d| d.insert_temp(id, open));
        return;
    };
    if panel.want_close {
        open = false;
    }
    if open {
        // The menu is a surface Escape closes, above the window it opened
        // in (see `escape`); a field being typed in keeps the press.
        if super::escape::take(&ctx, panel.area.layer_id, field_focused) {
            open = false;
        } else if !anchor.clicked() && !nested_popup_open && panel.area.clicked_elsewhere() {
            // The anchor guard matters on the frame the menu opens: that click
            // is outside the area too, and without it the menu would dismiss
            // itself the moment it was summoned.
            open = false;
        }
    }
    if open && panel.rows_entering() {
        ctx.request_repaint();
    }
    ctx.data_mut(|d| d.insert_temp(id, open));
}

/// A dropdown the caller holds open instead of the anchor's click: `open` is
/// decided every frame (a drag hovering the anchor, say), and the panel
/// rises, stays and fades on that alone — no click-away, no Escape. Returns
/// the panel's rect while it is on screen, so the caller can count the
/// pointer on the panel as still on the anchor.
pub fn held(
    anchor: &egui::Response,
    width: f32,
    open: bool,
    add: impl FnOnce(&mut MenuUi),
) -> Option<egui::Rect> {
    let ctx = anchor.ctx.clone();
    let id = anchor.id.with("ord_dropdown");
    let was: bool = ctx.data(|d| d.get_temp(id).unwrap_or(false));
    if open && !was {
        let now = ctx.input(|i| i.time);
        ctx.data_mut(|d| d.insert_temp(id.with("at"), now));
    }
    ctx.data_mut(|d| d.insert_temp(id, open));
    let panel = draw_panel(&ctx, anchor, id, open, width, false, add)?;
    if open && panel.rows_entering() {
        ctx.request_repaint();
    }
    Some(panel.area.rect)
}

/// What [`draw_panel`] reports back for the dismissal and repaint rules.
struct Panel {
    area: egui::Response,
    rows: usize,
    want_close: bool,
    since_open: f32,
}

impl Panel {
    /// The box's own open animation drives repaints while it runs; the row
    /// cascade outlives it slightly, so frames must keep coming until the
    /// last row has fully entered.
    fn rows_entering(&self) -> bool {
        self.since_open < ROW_ANIM + ROW_STAGGER * self.rows as f32
    }
}

/// The panel itself: the rise-and-fade on `open`, the glass, the rows.
/// `None` once a fade-out has settled, when nothing is drawn.
fn draw_panel(
    ctx: &egui::Context,
    anchor: &egui::Response,
    id: egui::Id,
    open: bool,
    width: f32,
    right: bool,
    add: impl FnOnce(&mut MenuUi),
) -> Option<Panel> {
    let now = ctx.input(|i| i.time);
    // How far up the panel is, 0 → 1. Drawing continues past a `false` in
    // `open` until the fade-out settles, so dismissing is as smooth as opening.
    let open_t = ctx.animate_bool_with_time_and_easing(
        id.with("t"),
        open,
        ANIM,
        egui::emath::easing::cubic_out,
    );
    if open_t <= 0.0 {
        return None;
    }
    let opened_at: f64 = ctx.data(|d| d.get_temp(id.with("at")).unwrap_or(now));
    let since_open = (now - opened_at) as f32;
    // Rise and fade share the one `open_t` so the two halves of the motion can
    // never drift apart.
    let rise = RISE * (1.0 - open_t);

    // The panel hangs from the anchor's left edge, but that edge is pinned
    // for as long as the menu is up: an anchor whose label grows while the
    // menu is open (the Filters button taking on its count) grows leftwards
    // when it is laid right-aligned, and the panel must not walk with it.
    // The anchor's right edge tells the two cases apart: it holds while a
    // right-aligned label grows, and moves when the window itself relaid
    // the anchor, which is when the pin is dropped and the panel follows.
    // Hung from the right edge, the panel ends where the anchor ends, and
    // that edge is the one that holds, so there is nothing to pin.
    let left = if right {
        anchor.rect.right() - (width + 2.0 * space::S2)
    } else {
        let pin_id = id.with("pin");
        let pin: Option<(f32, f32)> = ctx.data(|d| d.get_temp(pin_id));
        let left = match pin {
            Some((left, right)) if right == anchor.rect.right() => left,
            _ => anchor.rect.left(),
        };
        ctx.data_mut(|d| d.insert_temp(pin_id, (left, anchor.rect.right())));
        left
    };

    let mut rows = 0usize;
    let mut want_close = false;
    let mut slot = None;
    let area = egui::Area::new(id.with("area"))
        .order(egui::Order::Foreground)
        // Non-interactive until fully materialised, so a click during the fade
        // lands on whatever the user aimed at rather than on a ghost row.
        .interactable(open && open_t > 0.99)
        .fixed_pos(egui::pos2(left, anchor.rect.bottom() + GAP - rise))
        .constrain(true)
        .show(ctx, |ui| {
            ui.multiply_opacity(open_t);
            // The glass is the fill, painted under the rows once the panel's
            // rect is known (see `glass`); the frame keeps the shadow.
            slot = Some(super::glass::begin(ui));
            egui::Frame::none()
                .fill(egui::Color32::TRANSPARENT)
                .rounding(egui::Rounding::same(radius::MD))
                .inner_margin(egui::Margin::symmetric(space::S2, space::S2))
                .shadow(SHADOW)
                .show(ui, |ui| {
                    ui.set_min_width(width);
                    ui.set_max_width(width);
                    // Menu rows sit closer than the global spacing: the panel
                    // should read as one composed surface, not a stack of
                    // widgets with air between them.
                    ui.spacing_mut().item_spacing.y = 2.0;
                    let mut m = MenuUi {
                        ui,
                        since_open,
                        row: 0,
                        close: false,
                        grid: None,
                    };
                    add(&mut m);
                    rows = m.row;
                    want_close = m.close;
                });
        });
    if let Some(slot) = slot {
        super::glass::end(
            ctx,
            slot,
            id,
            area.response.rect,
            egui::Rounding::same(radius::MD),
            super::window::edge(),
        );
    }
    Some(Panel {
        area: area.response,
        rows,
        want_close,
        since_open,
    })
}

/// A right-click menu on `resp`, in the dropdown's chrome. egui's context
/// menu does the work (its placement at the pointer, its submenus, its
/// dismissal); this only dresses it: the panel on glass with the dropdown's
/// inset, rounding and shadow, and every plain `ui.button` in it at the
/// dropdown's row height and text inset, so a right-click menu and a
/// dropdown read as one component. Like the tooltip, it swaps egui's menu
/// frame to transparent in the style for the call and puts it back after.
/// Nested menus inside go through [`menu_button`].
pub fn context(resp: &egui::Response, add: impl FnOnce(&mut egui::Ui)) {
    let ctx = resp.ctx.clone();
    let style = ctx.style();
    ctx.style_mut(native_style);
    resp.context_menu(|ui| panel(ui, add));
    ctx.set_style(style);
}

/// egui's `menu_button` in the same chrome: a submenu inside a [`context`]
/// menu (egui makes it one when the ui is a menu's), or a button that opens
/// one. Drop-in for `ui.menu_button`.
pub fn menu_button<R>(
    ui: &mut egui::Ui,
    label: impl Into<egui::WidgetText>,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<Option<R>> {
    styled(ui, |ui| ui.menu_button(label, |ui| panel(ui, add)))
}

/// A top-level menu opened by a button of the caller's own styling, in the
/// same chrome. Drop-in for `egui::menu::menu_custom_button`.
pub fn custom_menu_button<R>(
    ui: &mut egui::Ui,
    button: egui::Button<'_>,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<Option<R>> {
    styled(ui, |ui| egui::menu::menu_custom_button(ui, button, |ui| panel(ui, add)))
}

/// Run `show` with egui's menu frame styled by [`native_style`], the global
/// style put back after.
fn styled<R>(ui: &mut egui::Ui, show: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let ctx = ui.ctx().clone();
    let style = ctx.style();
    ctx.style_mut(native_style);
    let r = show(ui);
    ctx.set_style(style);
    r
}

/// A line of text in a native menu (a title over the rows, a "nothing
/// here" note), aligned with the rows' labels.
pub fn text(ui: &mut egui::Ui, text: impl Into<egui::WidgetText>) {
    ui.horizontal(|ui| {
        ui.add_space(ROW_PAD);
        ui.label(text);
    });
}

/// egui's menu frame made transparent and inset like the dropdown's: the
/// glass painted in [`panel`] is the fill.
fn native_style(s: &mut egui::Style) {
    s.visuals.window_fill = egui::Color32::TRANSPARENT;
    s.visuals.window_stroke = egui::Stroke::NONE;
    s.visuals.popup_shadow = SHADOW;
    s.spacing.menu_margin = egui::Margin::same(space::S2);
}

/// A native menu's content on glass, its rows at the dropdown's metrics.
fn panel<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let ctx = ui.ctx().clone();
    let slot = super::glass::begin(ui);
    rows(ui);
    let r = add(ui);
    // The frame wraps the content's `min_rect` in the menu margin; the glass
    // covers the same. One glass entry per open menu: the Area's id is one.
    let rect = ui.min_rect() + ui.style().spacing.menu_margin;
    super::glass::end(
        &ctx,
        slot,
        ui.layer_id().id,
        rect,
        egui::Rounding::same(radius::MD),
        super::window::edge(),
    );
    r
}

/// Size egui's own widgets in a menu like this module's rows: a `ui.button`
/// comes out `ROW_H` tall with its label `ROW_PAD` in, rows sit 2pt apart,
/// and a disabled row paints no slab on the glass.
fn rows(ui: &mut egui::Ui) {
    let s = ui.spacing_mut();
    s.button_padding = egui::vec2(ROW_PAD, 0.0);
    s.interact_size.y = ROW_H;
    s.item_spacing.y = 2.0;
    // As good as transparent, but not `TRANSPARENT`: a disabled widget fades
    // its colours towards this fill, and egui reads a fade to exactly
    // transparent as "invisible" and drops the widget's shapes.
    ui.visuals_mut().widgets.noninteractive.weak_bg_fill = egui::Color32::from_black_alpha(1);
}

/// The same rows inside a native egui menu (a context menu's submenu),
/// where the panel is egui's and only the rows are ours: a `menu_button`
/// submenu that offers a multi-select (the Tags submenu of a song's menu)
/// draws its ✓ rows through this so they look and toggle the way a
/// [`dropdown`]'s do. No entrance cascade: the native menu has none, and
/// the rows should not unroll after the panel has popped. A `close()`
/// from within closes the native menu.
pub fn embedded(ui: &mut egui::Ui, add: impl FnOnce(&mut MenuUi)) {
    rows(ui);
    let mut m = MenuUi {
        ui,
        since_open: f32::MAX,
        row: 0,
        close: false,
        grid: None,
    };
    add(&mut m);
    if m.close {
        ui.close_menu();
    }
}

/// The builder handed to a [`dropdown`]'s content closure. Rows are drawn in
/// call order and each times its entrance off its position in that order.
pub struct MenuUi<'u> {
    ui: &'u mut egui::Ui,
    since_open: f32,
    row: usize,
    close: bool,
    /// Set while a [`MenuUi::columns`] run lays its rows.
    grid: Option<Grid>,
}

/// Where a [`MenuUi::columns`] run puts its rows: the `per_col` rows from
/// `first` fill a column down from `origin`, the next `per_col` the column
/// to its right, and so on.
#[derive(Clone, Copy)]
struct Grid {
    origin: egui::Pos2,
    col_w: f32,
    per_col: usize,
    first: usize,
}

impl MenuUi<'_> {
    /// A plain action row. Returns true on click.
    pub fn item(&mut self, label: impl Into<String>) -> bool {
        self.row(None, label.into(), None, color::LABEL, |_| false)
            .clicked()
    }

    /// A plain action row with a quiet right-aligned detail — a price's
    /// shipping, a count. Returns true on click.
    pub fn item_detail(&mut self, label: impl Into<String>, detail: impl Into<String>) -> bool {
        let detail = detail.into();
        self.row(
            None,
            label.into(),
            (!detail.is_empty()).then_some(detail),
            color::LABEL,
            |_| false,
        )
        .clicked()
    }

    /// A drop target for a drag carrying a `T` payload: lit like a hovered
    /// row while such a drag is over it (egui hovers nothing during a drag),
    /// in the reading column with a quiet detail. Returns the response, for
    /// the caller's release check.
    pub fn drop_row<T: std::any::Any + Send + Sync>(
        &mut self,
        label: impl Into<String>,
        detail: Option<String>,
    ) -> egui::Response {
        self.row(None, label.into(), detail, color::LABEL, |r| {
            r.dnd_hover_payload::<T>().is_some()
        })
    }

    /// An action row in the destructive red, for deletes and their kin.
    pub fn item_danger(&mut self, label: impl Into<String>) -> bool {
        self.row(None, label.into(), None, color::RED, |_| false)
            .clicked()
    }

    /// A row with a ✓ column — for single- or multi-select lists. All
    /// selectable rows share the column so their labels align. Returns true on
    /// click; the caller decides whether that selects, toggles, or closes.
    pub fn selectable(&mut self, selected: bool, label: impl Into<String>) -> bool {
        self.row(Some(selected), label.into(), None, color::LABEL, |_| false)
            .clicked()
    }

    /// A selectable row with a quiet right-aligned detail — a count, a
    /// shortcut, a unit. Kept out of the label so the reading column stays
    /// clean and the numbers line up against the panel edge.
    pub fn selectable_detail(
        &mut self,
        selected: bool,
        label: impl Into<String>,
        detail: impl Into<String>,
    ) -> bool {
        self.row(
            Some(selected),
            label.into(),
            Some(detail.into()),
            color::LABEL,
            |_| false,
        )
        .clicked()
    }

    /// A section caption, small, semibold and quiet, for a menu that
    /// groups rows under headings (facet pickers). Aligned to the row text
    /// inset.
    pub fn header(&mut self, text: impl Into<String>) {
        // The gap above is a group gap: it parts this caption from the rows
        // before it. A caption that opens the panel has nothing to part
        // from, and on the panel's own margin it sits as far from the top
        // edge as the last row does from the bottom.
        if self.ui.cursor().top() > self.ui.max_rect().top() {
            self.ui.add_space(space::S3);
        }
        self.ui.horizontal(|ui| {
            ui.add_space(ROW_PAD);
            ui.label(
                egui::RichText::new(text)
                    .font(font::strong(font::footnote().size))
                    .color(color::LABEL_3),
            );
        });
        self.ui.add_space(space::S1);
    }

    /// A capped-height scrollable run of rows for long lists (style tags).
    /// Rows inside keep their place in the entrance cascade, and a `close()`
    /// from within still dismisses the menu.
    pub fn scroll(&mut self, max_height: f32, add: impl FnOnce(&mut MenuUi)) {
        let since_open = self.since_open;
        let mut row = self.row;
        let mut close = false;
        // The panel's Area hands its content last frame's height, so a run
        // that starts short could never grow to its rows (a ratchet, with a
        // scrollbar over a cut-off list). The minimum scrolled height sizes
        // the run from the content instead, up to the cap.
        egui::ScrollArea::vertical()
            .max_height(max_height)
            .min_scrolled_height(max_height)
            .show(self.ui, |ui| {
                let mut m = MenuUi {
                    ui,
                    since_open,
                    row,
                    close: false,
                    grid: None,
                };
                add(&mut m);
                row = m.row;
                close = m.close;
            });
        self.row = row;
        self.close |= close;
    }

    /// The rows inside laid in columns of `per_col`, each `col_w` wide: a
    /// list longer than that runs on to the right instead of down the
    /// screen (the Tags menu), and the panel widens to hold the columns.
    /// Rows after the run span the whole width. The width is given, not
    /// read from the panel: egui's own menus size the panel from last
    /// frame's content, so a column as wide as the panel would double it
    /// every frame.
    pub fn columns(&mut self, per_col: usize, col_w: f32, add: impl FnOnce(&mut MenuUi)) {
        self.ui.set_max_width(col_w);
        let grid = Grid {
            origin: self.ui.cursor().min,
            col_w,
            per_col,
            first: self.row,
        };
        self.grid = Some(grid);
        add(self);
        self.grid = None;
        let n = self.row - grid.first;
        if n == 0 {
            return;
        }
        let cols = n.div_ceil(grid.per_col);
        let gap = self.ui.spacing().item_spacing.y;
        let size = egui::vec2(
            cols as f32 * (grid.col_w + COL_GAP) - COL_GAP,
            n.min(grid.per_col) as f32 * (ROW_H + gap) - gap,
        );
        // The rows placed themselves; this widens the panel to the last
        // column and puts the cursor under the tallest one.
        self.ui.set_max_width(size.x);
        self.ui
            .advance_cursor_after_rect(egui::Rect::from_min_size(grid.origin, size));
    }

    /// A hairline between row groups, inset from the panel edges.
    pub fn separator(&mut self) {
        let w = self.ui.available_width();
        let (rect, _) = self
            .ui
            .allocate_exact_size(egui::vec2(w, space::S3), egui::Sense::hover());
        let y = rect.center().y;
        self.ui.painter().hline(
            (rect.left() + space::S2)..=(rect.right() - space::S2),
            y,
            egui::Stroke::new(1.0, color::separator_opaque()),
        );
    }

    /// A quiet caption row — coverage notes, hints. Not interactive.
    pub fn note(&mut self, text: impl Into<String>) {
        self.ui.add_space(space::S1);
        self.ui.horizontal(|ui| {
            ui.add_space(ROW_PAD);
            ui.label(
                egui::RichText::new(text.into())
                    .font(font::footnote())
                    .color(color::LABEL_3),
            );
        });
        self.ui.add_space(space::S1);
    }

    /// Dismiss the menu after this frame — call on the click that should end
    /// the interaction. Toggling rows simply don't, and the menu stays up.
    pub fn close(&mut self) {
        self.close = true;
    }

    /// Escape hatch to the raw [`egui::Ui`] for bespoke content (scroll areas,
    /// buttons). Such content takes no part in the row cascade.
    pub fn ui(&mut self) -> &mut egui::Ui {
        self.ui
    }

    /// One row: animated hover fill, staggered fade/slide entrance, optional
    /// ✓ column and right-aligned detail, label in `text_color`. `lit` adds
    /// to the hover (a drag over a drop row); the response is returned.
    fn row(
        &mut self,
        selected: Option<bool>,
        label: String,
        detail: Option<String>,
        text_color: egui::Color32,
        lit: impl FnOnce(&egui::Response) -> bool,
    ) -> egui::Response {
        let i = self.row;
        self.row += 1;
        // This row's entrance, 0 → 1: rows trail each other by `ROW_STAGGER`
        // so the list unrolls top-down instead of appearing at once.
        let t = ((self.since_open - ROW_STAGGER * i as f32) / ROW_ANIM).clamp(0.0, 1.0);
        let enter = egui::emath::easing::cubic_out(t);

        let (rect, resp) = match self.grid {
            Some(g) => {
                let k = i - g.first;
                let (col, r) = (k / g.per_col, k % g.per_col);
                let gap = self.ui.spacing().item_spacing.y;
                let rect = egui::Rect::from_min_size(
                    g.origin
                        + egui::vec2(col as f32 * (g.col_w + COL_GAP), r as f32 * (ROW_H + gap)),
                    egui::vec2(g.col_w, ROW_H),
                );
                (rect, self.ui.allocate_rect(rect, egui::Sense::click()))
            }
            None => {
                let w = self.ui.available_width();
                self.ui
                    .allocate_exact_size(egui::vec2(w, ROW_H), egui::Sense::click())
            }
        };
        if !self.ui.is_rect_visible(rect) {
            return resp;
        }

        let lit = resp.hovered() || lit(&resp);
        let hot = self
            .ui
            .ctx()
            .animate_bool_with_time(resp.id.with("hot"), lit, HOVER_ANIM);
        let painter = self.ui.painter();
        // A picked row wears a soft accent wash under everything else, so an
        // active filter reads at a glance without shouting; the ✓ carries the
        // saturated accent.
        if selected == Some(true) {
            painter.rect_filled(
                rect,
                egui::Rounding::same(radius::SM),
                color::accent_soft().gamma_multiply(0.45 * enter),
            );
        }
        if hot > 0.0 {
            let fill = if resp.is_pointer_button_down_on() {
                color::surface_active()
            } else {
                color::surface_hover()
            };
            painter.rect_filled(
                rect,
                egui::Rounding::same(radius::SM),
                fill.gamma_multiply(hot),
            );
        }

        // Entrance: fade the row in while it slides its last few points left
        // into place. Subtle by design — the slide is 5pt over 160 ms.
        let slide = (1.0 - enter) * 5.0;
        let mut x = rect.left() + ROW_PAD + slide;
        if selected == Some(true) {
            painter.text(
                egui::pos2(x, rect.center().y),
                egui::Align2::LEFT_CENTER,
                "✓",
                font::body(),
                color::accent().gamma_multiply(enter),
            );
        }
        if selected.is_some() {
            x += CHECK_W;
        }
        // The detail claims its width first; the label gets the rest and
        // truncates with an ellipsis rather than running under the detail.
        let detail_galley = detail.map(|d| {
            painter.layout_no_wrap(d, font::callout(), color::LABEL_3.gamma_multiply(enter))
        });
        let label_max = rect.right()
            - ROW_PAD
            - x
            - detail_galley
                .as_ref()
                .map_or(0.0, |g| g.size().x + ROW_PAD);
        let label_galley = super::text::truncated(self.ui, label, font::body(), text_color.gamma_multiply(enter), label_max, true);
        painter.galley(
            egui::pos2(x, rect.center().y - label_galley.size().y / 2.0),
            label_galley,
            text_color,
        );
        if let Some(g) = detail_galley {
            painter.galley(
                egui::pos2(rect.right() - ROW_PAD - g.size().x, rect.center().y - g.size().y / 2.0),
                g,
                color::LABEL_3,
            );
        }
        resp
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An open dropdown must draw without touching the context's lock from
    /// inside one of its own write closures: that deadlocks the UI thread
    /// forever. Run the passes on a worker so a regression fails instead of
    /// hanging the test run.
    #[test]
    fn open_dropdown_draws_a_second_pass_without_deadlock() {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let ctx = egui::Context::default();
            let id = egui::Id::new("anchor").with("ord_dropdown");
            // Open it the way a click on the anchor would: the flag is the
            // dropdown's own state.
            ctx.data_mut(|d| d.insert_temp(id, true));
            for _ in 0..3 {
                let _ = ctx.run(Default::default(), |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let anchor = ui.push_id("anchor", |ui| ui.label("Sort")).response;
                        let anchor = egui::Response { id: egui::Id::new("anchor"), ..anchor };
                        dropdown(&anchor, 120.0, |m| {
                            m.selectable(true, "Artist");
                            m.item("Reset");
                        });
                    });
                });
            }
            // And an open dropdown stands in Escape's stack, above the
            // window it may have opened in.
            assert!(super::super::escape::pending(&ctx));
            let _ = tx.send(());
        });
        assert!(
            rx.recv_timeout(std::time::Duration::from_secs(10)).is_ok(),
            "drawing an open dropdown deadlocked"
        );
    }

    /// An anchor laid right-aligned grows leftwards when its label grows
    /// (the Filters button taking on its count); the open panel stays put.
    /// When the anchor's right edge moves too, the window relaid it and the
    /// panel follows.
    #[test]
    fn open_dropdown_keeps_its_left_edge_while_a_right_aligned_anchor_grows() {
        let ctx = egui::Context::default();
        let id = egui::Id::new("anchor").with("ord_dropdown");
        ctx.data_mut(|d| d.insert_temp(id, true));
        let area_left = |anchor: egui::Rect| {
            let mut left = None;
            for _ in 0..2 {
                let _ = ctx.run(Default::default(), |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let resp = ui.interact(anchor, egui::Id::new("anchor"), egui::Sense::click());
                        dropdown(&resp, 120.0, |m| {
                            m.item("Row");
                        });
                        left = ctx
                            .memory(|m| m.area_rect(id.with("area")))
                            .map(|r| r.left());
                    });
                });
            }
            left.expect("the open dropdown has an area")
        };
        let at = |l: f32, r: f32| egui::Rect::from_min_max(egui::pos2(l, 10.0), egui::pos2(r, 30.0));
        let first = area_left(at(300.0, 360.0));
        // Label grows leftwards, right edge holds: pinned.
        assert_eq!(area_left(at(280.0, 360.0)), first);
        // The whole anchor moved: the panel follows.
        assert_ne!(area_left(at(380.0, 460.0)), first);
    }

    /// A run of rows past its column cap continues in a column to the
    /// right, the panel widens to it, and the rows after span that width.
    #[test]
    fn columns_run_a_long_list_rightwards() {
        let ctx = egui::Context::default();
        let mut size = egui::Vec2::ZERO;
        let mut after = 0.0;
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let menu = ui.scope(|ui| {
                    embedded(ui, |m| {
                        m.columns(12, 100.0, |m| {
                            for i in 0..13 {
                                m.item(i.to_string());
                            }
                        });
                        after = m.ui().available_width();
                    });
                });
                size = menu.response.rect.size();
            });
        });
        assert!(size.x >= 200.0, "{size:?}");
        assert!(size.y <= 12.0 * (ROW_H + 2.0), "{size:?}");
        assert!(after >= 200.0, "{after}");
    }
}
