//! Design tokens — the single source of truth for Ordnung's visual language.
//!
//! Apple-inspired and deliberately small: layered dark-gray surfaces (never pure
//! black), hairline separators, an 8-pt spacing grid, a concentric corner-radius
//! scale, an SF-like type ramp, and a semantic colour palette. Everything visual
//! should reference a token here rather than an inline literal, so the whole app
//! re-skins from one place.
//!
//! Pass 1 wires these into egui's global [`Style`](eframe::egui::Style) (see
//! [`super::theme`]); bespoke components consume them directly in a later pass.

use eframe::egui::{Color32, FontId};

/// Corner radii. Apple keeps nested corners *concentric*: an inset child's radius
/// equals its parent's radius minus the padding between them, so the two curves
/// stay parallel.
pub mod radius {
    pub const XS: f32 = 4.0; // chips, tiny pills
    pub const SM: f32 = 6.0; // buttons, inputs
    pub const MD: f32 = 10.0; // menus, popovers
    pub const LG: f32 = 14.0; // cards, windows
}

/// 8-pt spacing grid. Reach for these instead of ad-hoc pixel gaps; consistent
/// rhythm is most of what makes a layout read as "designed".
/// Stroke widths.
pub mod stroke {
    /// The edge of a control at rest: a touch over a hairline, so at Retina it
    /// comes out as a clean solid line rather than a feathered one.
    pub const OUTLINE: f32 = 1.5;
}

pub mod space {
    pub const S1: f32 = 2.0;
    pub const S2: f32 = 4.0;
    pub const S3: f32 = 8.0;
    pub const S4: f32 = 12.0;
    pub const S5: f32 = 16.0;
    pub const S6: f32 = 24.0;
    pub const S7: f32 = 32.0;
}

/// Semantic colours. Surfaces are layered grays that read as elevation; label
/// colours descend in contrast for hierarchy; separators are hairline; the system
/// hues follow Apple's dark-mode HIG values for status accents.
pub mod color {
    use super::Color32;
    use std::sync::atomic::{AtomicU32, Ordering};

    // --- Surfaces, low → high elevation ---
    // Every surface is a fixed step from one base tone, the app background,
    // which the user can change (Settings → Appearance, a look). Surfaces
    // are read through these functions, never a const, so the whole app
    // re-skins at once; the stock base comes out as the hand-tuned grays.
    /// The stock app background: egui's prior default dark fill
    /// (`from_gray(27)`), so the window brightness is what it always was.
    pub const BG_STOCK: Color32 = Color32::from_rgb(27, 27, 27);
    static BG_RT: AtomicU32 = AtomicU32::new(0x1B1B1B);

    /// Set the app background, `None` for the stock gray. The style is
    /// pushed separately (`theme::restyle`), so call that after.
    pub fn set_background(c: Option<Color32>) {
        BG_RT.store(to_packed(c.unwrap_or(BG_STOCK)), Ordering::Relaxed);
    }
    /// App background behind panels.
    pub fn bg() -> Color32 {
        from_packed(BG_RT.load(Ordering::Relaxed))
    }
    /// `base` moved `d` per channel, pinned to the byte range.
    fn step(base: Color32, d: [i16; 3]) -> Color32 {
        let c = |v: u8, d: i16| (i16::from(v) + d).clamp(0, 255) as u8;
        Color32::from_rgb(c(base.r(), d[0]), c(base.g(), d[1]), c(base.b(), d[2]))
    }
    /// Content background for the main songs/table area over `base` — a
    /// touch lighter, so the central list reads as raised above the nav
    /// sidebar and the top/bottom bars (which stay on the base).
    pub fn content_bg_of(base: Color32) -> Color32 {
        step(base, [8, 8, 10])
    }
    pub fn content_bg() -> Color32 {
        content_bg_of(bg())
    }
    /// Default panel / resting surface over `base`.
    pub fn surface_of(base: Color32) -> Color32 {
        step(base, [5, 5, 8])
    }
    pub fn surface() -> Color32 {
        surface_of(bg())
    }
    /// Raised surface — cards, hovered rows, menus.
    pub fn surface_hi() -> Color32 {
        step(bg(), [17, 17, 21])
    }
    /// The see-through surface every floating layer shares: the record sheet
    /// over its frost, and the dropdowns and popups over whatever they open
    /// on. One alpha, so a menu opened over the sheet reads as the same
    /// glass: `surface` at 90%.
    pub fn surface_glass() -> Color32 {
        surface().gamma_multiply(0.9)
    }
    /// Sunken surface — text fields and other inputs.
    pub fn field() -> Color32 {
        step(bg(), [-7, -7, -5])
    }
    /// Hovered interactive surface — one step above `surface_hi`.
    pub fn surface_hover() -> Color32 {
        step(bg(), [27, 27, 31])
    }
    /// Pressed interactive surface — one step above `surface_hover`.
    pub fn surface_active() -> Color32 {
        step(bg(), [37, 37, 41])
    }
    /// The pull tab's hovered fill — one elevation step up from the drawer, so
    /// the handle lifts under the pointer instead of flashing to a light grey.
    pub fn drawer_hover() -> Color32 {
        step(bg(), [19, 19, 23])
    }

    // --- Hairlines ---
    /// Translucent separator that adapts to whatever sits behind it (white α20,
    /// premultiplied — equivalent to `from_white_alpha(20)` but const-constructible).
    pub const SEPARATOR: Color32 = Color32::from_rgba_premultiplied(20, 20, 20, 20);
    /// Opaque separator for use over a known dark surface.
    pub fn separator_opaque() -> Color32 {
        step(bg(), [29, 29, 33])
    }
    /// The outline on a control at rest: a solid grey edge that lifts a
    /// button off the surface without hardening into a border. Opaque, not
    /// translucent, so it reads as one flat line rather than a tint of what
    /// sits behind it. The way a macOS toolbar button sits on its bar.
    pub const OUTLINE: Color32 = Color32::from_rgb(78, 78, 82);
    /// The same outline while the pointer is on the control.
    pub const OUTLINE_HOVER: Color32 = Color32::from_rgb(98, 98, 102);

    // --- Labels, primary → faint ---
    pub const LABEL: Color32 = Color32::from_rgb(235, 235, 240);
    pub const LABEL_2: Color32 = Color32::from_rgb(168, 168, 176);
    pub const LABEL_3: Color32 = Color32::from_rgb(120, 120, 128);
    pub const LABEL_4: Color32 = Color32::from_rgb(88, 88, 96);

    // --- Accent ---
    // The one hue the user picks freely (Settings → Appearance). Read
    // through the functions below, never a const, so every selection, link
    // and primary button follows the choice at once.
    /// The stock accent, for showing the default choice. Draw with [`accent`].
    pub const ACCENT_STOCK: Color32 = Color32::from_rgb(74, 134, 232);
    static ACCENT_RT: AtomicU32 = AtomicU32::new(0x4A86E8);

    /// Set the accent, `None` for the stock blue. The style is pushed
    /// separately (`theme::restyle`), so call that after.
    pub fn set_accent(c: Option<Color32>) {
        ACCENT_RT.store(to_packed(c.unwrap_or(ACCENT_STOCK)), Ordering::Relaxed);
    }
    /// The accent: selections, links, the active control.
    pub fn accent() -> Color32 {
        from_packed(ACCENT_RT.load(Ordering::Relaxed))
    }
    /// The accent under the pointer, a step lighter.
    pub fn accent_hover() -> Color32 {
        accent().lerp_to_gamma(Color32::WHITE, 0.15)
    }
    /// Translucent-looking accent for selection fills: `accent` blended
    /// into the `base` it sits on.
    pub fn accent_soft_of(accent: Color32, base: Color32) -> Color32 {
        base.lerp_to_gamma(accent, 0.42)
    }
    pub fn accent_soft() -> Color32 {
        accent_soft_of(accent(), bg())
    }
    /// The primary-action fill over `base`: the "Add songs…" button, the
    /// onboarding and token buttons. The one saturated element on screen,
    /// a shade under the accent so white text sits on it.
    pub fn nav_accent_of(accent: Color32, base: Color32) -> Color32 {
        accent.lerp_to_gamma(base, 0.25)
    }
    pub fn nav_accent() -> Color32 {
        nav_accent_of(accent(), bg())
    }

    /// How much darker a shaded table row is by default (Settings →
    /// Appearance): the alpha of the black veil over every other row. A
    /// whisper over `content_bg`, enough to follow a wide row across its
    /// columns.
    pub const ROW_SHADE: u8 = 40;

    /// A look: an app background and the accent that suits it, picked as
    /// one in Settings → Appearance. The first is the stock look.
    pub struct Look {
        pub name: &'static str,
        pub background: Color32,
        pub accent: Color32,
    }
    pub const LOOKS: &[Look] = &[
        Look { name: "Graphite", background: BG_STOCK, accent: ACCENT_STOCK },
        Look { name: "Midnight", background: Color32::from_rgb(16, 19, 31), accent: Color32::from_rgb(99, 140, 255) },
        Look { name: "Pitch", background: Color32::from_rgb(9, 9, 9), accent: GREEN },
        Look { name: "Mocha", background: Color32::from_rgb(33, 28, 25), accent: ORANGE },
        Look { name: "Slate", background: Color32::from_rgb(40, 42, 47), accent: ACCENT_STOCK },
    ];

    // --- System status hues (Apple HIG, dark) ---
    pub const RED: Color32 = Color32::from_rgb(255, 69, 58);
    pub const ORANGE: Color32 = Color32::from_rgb(255, 159, 10);
    pub const YELLOW: Color32 = Color32::from_rgb(255, 214, 10);
    pub const GREEN: Color32 = Color32::from_rgb(48, 209, 88);
    pub const BLUE: Color32 = Color32::from_rgb(10, 132, 255);
    pub const GRAY: Color32 = Color32::from_rgb(142, 142, 147);
    pub const TEAL: Color32 = Color32::from_rgb(64, 200, 224);
    pub const INDIGO: Color32 = Color32::from_rgb(94, 92, 230);
    pub const PURPLE: Color32 = Color32::from_rgb(191, 90, 242);
    pub const PINK: Color32 = Color32::from_rgb(255, 55, 95);
    pub const MINT: Color32 = Color32::from_rgb(102, 212, 207);
    pub const BROWN: Color32 = Color32::from_rgb(172, 142, 104);

    // --- Meaning inks ---
    // Status as text, toned to sit inside the palette; the full system
    // hues above are for marks and fills.
    /// Good news: a price, a record in stock, a copy you own, terms met.
    pub const POSITIVE: Color32 = Color32::from_rgb(120, 200, 140);
    /// Bad news: a failed fetch, a deletion, a lossy copy.
    pub const NEGATIVE: Color32 = Color32::from_rgb(224, 108, 108);
    /// A caution: a suspect transcode, a check that could not run.
    pub const CAUTION: Color32 = Color32::from_rgb(225, 180, 90);
    /// Playback: the played part of a track, the transport bar, the playing
    /// mark, the in-catalog chip. Fixed, unlike the accent, so a playing
    /// thing reads the same in every look.
    pub const PLAY: Color32 = Color32::from_rgb(90, 200, 120);
    /// [`PLAY`] under the pointer.
    pub const PLAY_HOVER: Color32 = Color32::from_rgb(120, 220, 150);

    /// The colours a user can give a playlist, in picker order. A fixed set
    /// rather than a free wheel: the sidebar reads as one palette, and every
    /// hue here is checked against the dark surfaces it sits on.
    pub const TAG_PALETTE: &[(&str, Color32)] = &[
        ("Red", RED),
        ("Orange", ORANGE),
        ("Yellow", YELLOW),
        ("Green", GREEN),
        ("Mint", MINT),
        ("Teal", TEAL),
        ("Blue", BLUE),
        ("Indigo", INDIGO),
        ("Purple", PURPLE),
        ("Pink", PINK),
        ("Brown", BROWN),
        ("Gray", GRAY),
    ];

    /// Unpack a catalog colour (`0xRRGGBB`, see `Playlist::color`).
    pub fn from_packed(c: u32) -> Color32 {
        Color32::from_rgb((c >> 16) as u8, (c >> 8) as u8, c as u8)
    }

    /// Pack a colour for the catalog (`0xRRGGBB`). Alpha is dropped.
    pub fn to_packed(c: Color32) -> u32 {
        ((c.r() as u32) << 16) | ((c.g() as u32) << 8) | c.b() as u32
    }

    /// A chosen colour as the config stores it, `#rrggbb`.
    pub fn to_hex(c: Color32) -> String {
        format!("#{:06x}", to_packed(c))
    }
    /// Parse `#rrggbb`; anything else is `None`.
    pub fn from_hex(s: &str) -> Option<Color32> {
        let hex = s.strip_prefix('#').filter(|h| h.len() == 6)?;
        u32::from_str_radix(hex, 16).ok().map(from_packed)
    }
}

/// A size ramp for consistent text hierarchy, named after the role each size
/// plays. [`super::theme`] binds these to egui's `TextStyle`s, so stock widgets
/// pick them up automatically; call them directly for bespoke text.
pub mod font {
    use super::FontId;
    use eframe::egui::FontFamily;

    /// Name of the Inter SemiBold family installed by [`super::super::theme`].
    /// egui 0.29 has no weight axis, so the second weight is a named family.
    pub const STRONG_FAMILY: &str = "strong";

    /// The same size at SemiBold. Weight is the *first* tool for emphasis —
    /// reach for this before a larger size, so hierarchy doesn't inflate the
    /// type scale. `size` should come from the ramp below, e.g.
    /// `font::strong(font::body().size)`.
    pub fn strong(size: f32) -> FontId {
        FontId::new(size, FontFamily::Name(STRONG_FAMILY.into()))
    }

    /// Oversized numerals / empty-state art — the one step above `large_title`,
    /// for display type that is deliberately graphic rather than read as prose.
    pub fn display() -> FontId {
        FontId::proportional(40.0)
    }

    pub fn large_title() -> FontId {
        FontId::proportional(26.0)
    }
    pub fn title() -> FontId {
        FontId::proportional(20.0)
    }
    pub fn headline() -> FontId {
        FontId::proportional(15.0)
    }
    pub fn body() -> FontId {
        FontId::proportional(13.0)
    }
    pub fn callout() -> FontId {
        FontId::proportional(12.0)
    }
    pub fn footnote() -> FontId {
        FontId::proportional(11.0)
    }
    pub fn caption() -> FontId {
        FontId::proportional(10.0)
    }
    pub fn mono() -> FontId {
        FontId::monospace(12.5)
    }

    /// Name of the family that holds only the Phosphor icon face (installed
    /// by [`super::super::theme`]). Icons are set in *this* family, never in
    /// the proportional chain: Inter carries its own private-use glyphs at
    /// some of the codepoints Phosphor uses (its heart came out as a
    /// diacritic), and Phosphor cannot lead the chain because it also
    /// carries a space and lowercase letters of its own.
    pub const ICON_FAMILY: &str = "icons";

    /// An icon from the app's icon face at `size`. Beside text, set it two
    /// points over the text's size: a Phosphor glyph fills about four fifths
    /// of its em, so that is what brings it to the cap height.
    pub fn icon(size: f32) -> FontId {
        FontId::new(size, FontFamily::Name(ICON_FAMILY.into()))
    }
    /// Small monospace — transport timecodes and other tabular numerals that sit
    /// beside `footnote`-sized copy. Matched to `footnote` so a clock next to a
    /// label shares its baseline weight rather than looking a step larger.
    pub fn mono_small() -> FontId {
        FontId::monospace(11.0)
    }
}
