//! Persistent GUI settings, stored at `~/.ordnung/config.toml` (next to the
//! catalog). Policy and process I/O live in the GUI boundary per
//! `ordnung-architecture`; `ordnung-core` stays pure.

use ordnung_core::discogs::FillField;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// The `discogs_skip_fields` key for the cover; the tag fields use
/// [`FillField::key`].
pub const COVER_FILL: &str = "cover";

/// User settings that must survive across launches — including launches from
/// Finder/Dock, which inherit none of the shell environment. Currently just the
/// Discogs token; extend in place as more settings appear.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Discogs personal access token. Empty means "not set" — callers then fall
    /// back to the `DISCOGS_TOKEN` environment variable.
    pub discogs_token: String,
    /// Release carriers to hide in the Discogs release picker, as stable
    /// [`ReleaseMedium`] keys. Empty (the default, and what older configs get)
    /// means show every format.
    ///
    /// Stored as what's *hidden* rather than what's shown so the default stays
    /// "show everything" and a medium added in a later build appears without
    /// needing a config migration. A collector who only buys records hides
    /// `cd`/`digital`/`cassette` and stops scrolling past pressings they'd
    /// never want.
    pub hidden_release_mediums: Vec<String>,
    /// Match each new import to a Discogs release automatically, with no picker:
    /// the imported track is searched as soon as it lands (after auto-analysis)
    /// and the best candidate by [`Config::discogs_auto_match`] is applied —
    /// release link, cover art, and empty tag fields. On by default; does
    /// nothing until a Discogs token is configured. The right-click menu's
    /// "Find Discogs release" picker remains the manual override and re-pick.
    pub discogs_auto_fetch: bool,
    /// Which candidate the automatic match commits to, as a stable
    /// [`ReleaseAutoMatch`] key. Defaults to `"most_collected"` — the release
    /// the most Discogs users own, the strongest signal for "the normal one".
    pub discogs_auto_match: ReleaseAutoMatch,
    /// What a matched release leaves alone: [`FillField::key`] keys and
    /// [`COVER_FILL`]. Empty (the default) fills everything. Stored as what's *skipped*,
    /// like `hidden_release_mediums`, so a field added later fills without a
    /// config migration.
    pub discogs_skip_fields: Vec<String>,
    /// Discogs username of the token owner, captured on the first collection
    /// sync. Lets the "Vinyl Collection" view link to the user's collection
    /// page across launches without re-resolving it. Empty until a sync runs.
    pub discogs_username: String,
    /// The folder the user's music library lives in — the persistent answer to
    /// "where does my music come from?". Picked in the welcome tour (or
    /// Settings → General) and used to kick off the first import; later it is
    /// what makes "scan for new arrivals" possible without re-picking a folder
    /// every time. `None` — the default, and what every config predating the
    /// field gets — means no root is set and music arrives only via
    /// "Add songs…" or drag-drop, exactly as before.
    pub library_root: Option<PathBuf>,
    /// Track-table column order as stable column keys (see `TableColumn::key`).
    /// Empty means "use the default order". Tolerant of unknown or missing keys
    /// on load, so a config from an older build keeps working as columns change.
    pub column_order: Vec<String>,
    /// Track-table columns the user has hidden, as stable column keys.
    pub hidden_columns: Vec<String>,
    /// Track-table column widths in points, keyed by stable column key (see
    /// `TableColumn::key`). Shared across every view (library and all playlists)
    /// and durable across rebuilds — unlike egui's own per-layout width memory,
    /// whose id shifts when the surrounding UI changes. Missing keys fall back to
    /// the per-column default width. A `BTreeMap` so the saved TOML is stable.
    pub column_widths: BTreeMap<String, f32>,
    /// Sort applied to the track table on launch, as a stable column key (see
    /// `TableColumn::key`). Empty (the default) means "natural order" — catalog
    /// or playlist order, the prior behavior. Unknown or unsortable keys also
    /// fall back to natural order.
    pub default_sort: String,
    /// Direction for `default_sort` (`true` = ascending). Ignored when
    /// `default_sort` is empty.
    pub default_sort_ascending: bool,
    /// How the "Vinyl Collection" grid is ordered: `"added"` (Discogs date
    /// added), `"price"` (lowest marketplace listing), or `"artist"`. Unknown
    /// values fall back to artist, the original fixed order. See `VinylSort`.
    pub vinyl_sort: crate::views::VinylSort,
    /// Direction for `vinyl_sort` (`true` = ascending: oldest / cheapest / A–Z
    /// first). Defaults to descending, so the default view is newest first.
    pub vinyl_sort_ascending: bool,
    /// How the vinyl tabs lay their records out: `"grid"` (a wall of covers,
    /// the default) or `"list"` (compact rows — more records per screen, with
    /// the caption fields readable at a glance). One choice covers all three
    /// tabs: collection, wantlist and a seller's crates are the same kind of
    /// browsing. Unknown values fall back to grid.
    pub vinyl_view: String,
    /// Where the radio bar sits on the map: `"top"` (the default), `"bottom"`,
    /// `"left"` or `"right"`. Set by dragging the bar; at a side it stands
    /// upright with the cover on top.
    pub radio_dock: crate::radio::RadioDock,
    /// How the record map gathers its records: `"artists"` (hubs are
    /// artists, the default) or `"genres"` (hubs are styles, so the map
    /// reads as genre clouds). Unknown values fall back to artists.
    pub graph_arrange: crate::graph::Arrange,
    /// Kinds of record the map leaves out: any of `"owned"`, `"wanted"`
    /// and `"dug"` (see `graph::Status::key`). Empty shows everything. A
    /// long dig fills the map with discovered records; hiding them brings
    /// the shelves back into view without forgetting the finds.
    pub graph_hide: Vec<String>,
    /// Which library sits at the top of the left navigation sidebar:
    /// `"digital"` (Library / New / playlists first, the default) or
    /// `"vinyl"` (the Discogs vinyl collection first). A vinyl-led collector
    /// gets their shelf up top and the digital library pinned below. Unknown
    /// values fall back to `"digital"`. See `NavPrimary`.
    pub nav_primary: NavPrimary,
    /// USB exports convert FLAC and anything above 48 kHz to AIFF on the
    /// stick, for players that can't play them (CDJ-2000/NXS/900 and
    /// older). Remembered from the export confirm's checkbox; maps to
    /// `ordnung_rbdb::export::PlayerTarget::Classic`.
    pub export_convert_for_older_players: bool,
    /// What a USB export writes into each track's comment, the field a CDJ
    /// shows under the song info: `"notes"` (the track's Notes tag, the
    /// default), `"tags"` (its Ordnung tags, `dub, dark`), `"notes_tags"`
    /// (both, notes first) or `"none"`. Remembered from the export
    /// confirm's dropdown. See `ExportComment`.
    pub export_comment: ExportComment,
    /// The id this install stamps on the sticks it writes as their library
    /// id (rekordbox stamps its library's DBID); minted on the first export.
    pub export_db_id: u32,
    /// Which of the sidebar's three width tiers is in force: `"icon"`,
    /// `"narrow"` or `"wide"` (the default). The sidebar snaps between designed
    /// layouts rather than resizing freely, so what persists is the chosen tier,
    /// not a pixel width. Unknown values fall back to `"wide"`. See `NavDensity`.
    pub nav_density: crate::sidebar::NavDensity,
    /// The sidebar lists folded shut by their caption's chevron:
    /// `"playlists"`, `"crates"`, `"tags"`. Stored as what is hidden so the
    /// default, and every older config, shows every list.
    pub nav_collapsed: Vec<String>,
    /// Which of the inspector drawer's two width tiers is in force:
    /// `"narrow"` or `"regular"` (the default). Like `nav_density`, a tier
    /// rather than a pixel width. See `InspectorDensity`.
    pub inspector_density: crate::inspector::InspectorDensity,
    /// Whether the right-hand track inspector drawer is showing. Mirrors the
    /// pull tab's toggle so the drawer comes back the way it was left. Off by
    /// default (and for older configs that predate the field): a fresh install
    /// opens on the full-width table, with the pull tab there to discover.
    pub inspector_open: bool,
    /// Which section the app opens on: `"library"` (Library, the default),
    /// `"vinyl"` (the vinyl collection), or `"recent"` (new imports). Unknown
    /// values fall back to `"library"`. See `StartupView`.
    pub startup_view: StartupView,
    /// Run analysis (BPM, key, waveform) automatically on each track as it's
    /// imported, instead of waiting for the explicit "Analyze" action. On by
    /// default; defaults to on for older configs that predate the field too.
    pub auto_analyze: bool,
    /// The cue bar (hot cue pads, loop, memory cues) is showing above the
    /// player's zoom lane. Mirrors the lane's CUES tab so it comes back the
    /// way it was left. Hidden by default: the player stays a bar until a
    /// DJ asks for pads.
    pub cue_bar_open: bool,
    /// Quantize: new cues and loops snap to the track's beatgrid, and beat
    /// loops are exact beat multiples, the way rekordbox's Q button works.
    /// Off, cues land exactly where the playhead is.
    pub cue_quantize: bool,
    /// Write tag edits straight into the source files instead of parking them
    /// behind the toolbar's "Write N edited to files" button. When on, the
    /// inspector's Save writes the file too, and edits made elsewhere (Discogs
    /// enrichment, bulk fetches, fetched cover art) are flushed by a background
    /// write as soon as no other job is running.
    ///
    /// **On by default**, at the user's explicit direction. This is the one
    /// place the GUI departs from the `ordnung-architecture` "tag writeback is
    /// opt-in" rule: a catalog edit that never reaches the file is a surprise,
    /// not a safeguard, so the app keeps files in sync unless told otherwise.
    /// Only ever touches the tag block of tracks the user actually edited, and
    /// turning it off restores the explicit two-step. Older configs that predate
    /// the field get the new default too.
    pub auto_write_tags: bool,
    /// Version of the first-run welcome tour this install has completed (see
    /// `onboarding::TOUR_VERSION`). `0` — the default, and what every config
    /// predating the tour gets — means "never seen it", which is what opens the
    /// tour on a fresh install. Storing a version rather than a bool lets a
    /// materially changed tour show once more to existing users.
    pub onboarding_completed_version: u32,
    /// Default target format pre-selected in the convert dialogs, as a stable
    /// lowercase key (`mp3`/`aac`/`flac`/`wav`/`aiff`; see `util::format_key`).
    /// Empty or unknown falls back to AIFF, the prior hard-coded default.
    pub convert_format: String,
    /// Bitrate (kbps) prefilled for lossy convert targets (MP3/AAC), as the text
    /// shown in the field. Empty means "use the per-format hint" (320 / 256).
    pub convert_bitrate_kbps: String,
    /// Default output folder for conversions. `None` (the default) means
    /// "alongside each source file".
    pub convert_out_dir: Option<PathBuf>,
    /// Whether the convert dialogs default to replacing the source file in place.
    /// On by default, preserving the prior hard-coded behavior.
    pub convert_in_place: bool,
    /// Convert files automatically as they enter the library (scan, drag-drop,
    /// USB transfer), using the convert defaults above. **Off by default** —
    /// like `auto_analyze`/`auto_write_tags` this is GUI policy the user
    /// switches on explicitly; core's convert engine stays explicit-only and
    /// is only ever driven from here with a snapshotted spec.
    pub auto_convert: bool,
    /// Which source formats auto-convert applies to, as stable lowercase keys
    /// (see `util::format_key`). Empty — the default — means every format;
    /// e.g. `["flac"]` converts only imported FLACs. Files already in the
    /// target format are always skipped regardless of this filter.
    pub auto_convert_sources: Vec<String>,
    /// Master playback volume as a linear amplitude factor, `0.0`–`1.0`. Driven
    /// by the toolbar knob and restored at startup so the app comes back at the
    /// level it was left at.
    pub volume: f32,
    /// Key lock (a CDJ's master tempo) for the player's pitch fader: on, the
    /// fader changes the tempo and keeps the musical key; off, it behaves
    /// like a turntable's and shifts both. Remembered like a deck setting.
    pub key_lock: bool,
    /// Vinyl mode for the table's harmonic cue: on, a Key cell lights green
    /// when the row's key mixes with the playing song's *after* the pitch
    /// shift a turntable applies to beatmatch the two (from their tagged
    /// BPMs); off (the default), the keys are compared at face value.
    pub vinyl_mode: bool,
    /// Start a record's videos part-way in rather than at the top, so a
    /// preview of a release lands past the intro and the build. The
    /// fraction is `playback::MID_START_FRACTION`. Carries the old single
    /// `mid_start` key forward, which covered both engines.
    #[serde(alias = "mid_start")]
    pub mid_start_videos: bool,
    /// The same for library files in the player, on its own switch: a
    /// record you are sizing up wants the drop, a track you own is one you
    /// may want to hear whole.
    pub mid_start_files: bool,
    /// How the player's waveform is colored: `"energy"` (cool→hot gradient by
    /// each section's energy — perceived loudness × spectral occupancy) or
    /// `"spectrum"` (additive RGB from the low/mid/high band balance, like
    /// rekordbox/Serato). Unknown values fall back to `"energy"`. See
    /// `WaveformColorMode`.
    pub waveform_color_mode: WaveformColorMode,
    /// Render-time height companding for the waveform. `1.0` keeps the stored
    /// sqrt-companded amplitude (most compressed); `2.0` cancels the sqrt back to
    /// linear amplitude (least compressed, rekordbox-like). See `wave_height`.
    pub waveform_height_exp: f32,
    /// The player's zoom lane zooms in fixed steps (a doubling ladder, like
    /// a CDJ's zoom levels) instead of smoothly.
    pub wave_zoom_steps: bool,
    /// Per-band visual height gain for spectrum mode `[low, mid, high]`. The bass
    /// band swamps the others, so the default trims it and lifts mid/high.
    pub waveform_band_gain: [f32; 3],
    /// Visual height gain for the single envelope in energy mode. `1.0` keeps the
    /// stored amplitude; lower trims, higher lifts. The spectrum-mode equivalent
    /// is `waveform_band_gain`.
    pub waveform_energy_gain: f32,
    /// RGB colors for the three spectrum bands `[low, mid, high]`. Defaults to the
    /// Serato/rekordbox convention (low = red, mid = green, high = light blue).
    pub waveform_band_colors: [[u8; 3]; 3],
    /// RGB stops for the energy-mode cool→hot gradient, quiet → loudest (5 stops).
    pub waveform_energy_colors: [[u8; 3]; 5],
    /// Low/mid band crossover (Hz) for the zoom detail lane's live hi-res bands.
    /// Lower it toward kick + sub so low-mid energy stays out of the bass band.
    /// Only the zoom lane honors this live; the full-track overview uses the split
    /// baked in at analysis time. See `compute_hires_bands`.
    pub waveform_low_hz: f32,
    /// Mid/high band crossover (Hz) for the zoom detail lane's live hi-res bands.
    /// Everything above this reads as the high band. See `compute_hires_bands`.
    pub waveform_mid_hz: f32,
    /// Waveform smoothing strength `[0, 1]`: scales the attack/release time
    /// constants below from `0` (raw envelope) to their full values, so one knob
    /// sweeps raw → fully smoothed. See `smooth_source` and
    /// `WaveformStyle::smoothing`.
    pub waveform_smoothing: f32,
    /// Waveform smoothing attack time constant (ms of audio) at full smoothing:
    /// how much a *rising* edge is rounded. A few ms irons out pixel-scale
    /// jaggies while keeping transient onsets crisp. See `smooth_source`.
    pub waveform_smooth_attack_ms: f32,
    /// Waveform smoothing release time constant (ms of audio) at full smoothing:
    /// how long a *falling* tail rings out. Beat-scale (~450 ms) keeps a kick's
    /// tail standing until the next kick so the envelope reads as a connected
    /// silhouette; short values let it pinch to the centerline between beats
    /// (separate petals). See `smooth_source`.
    pub waveform_smooth_release_ms: f32,
    /// Bass floor threshold `[0, 1]` (fraction of full scale): low-band content
    /// quieter than this is treated as sustained sub (the tail lingering under a
    /// kick) rather than a transient peak, and is dimmed by
    /// `waveform_bass_floor_amount`. Louder bass (kick attacks) is kept at full
    /// height. See `bass_floor_gain`.
    pub waveform_bass_floor_threshold: f32,
    /// How much to dim sustained sub below `waveform_bass_floor_threshold`:
    /// `0` keeps it (no change), `1` removes it entirely, leaving only bass
    /// transients. See `bass_floor_gain`.
    pub waveform_bass_floor_amount: f32,
    /// Saved snapshots of the Waveform settings tab (Settings → Waveform →
    /// Presets), keyed by their 1-based `slot`. At most one entry per slot; a
    /// plain `Vec` (not `[Option<_>; 5]`) because TOML can't represent `None`
    /// holes in an array.
    pub waveform_presets: Vec<WaveformPreset>,
    /// Shade every other row of the song and record tables, so a wide row
    /// can be followed across its columns. On by default.
    pub zebra_rows: bool,
    /// Carry the shading on under a list too short to fill its view, so the
    /// blank space below the last row reads as empty rows. Off by default;
    /// ignored while `zebra_rows` is off.
    pub zebra_fill: bool,
    /// How much darker a shaded row is: the alpha of a black veil over
    /// every other row, so forty is a whisper and a hundred a clear band.
    /// Ignored while `zebra_rows` is off.
    pub row_shade: u8,
    /// The accent colour (selections, links, the primary button) as
    /// `#rrggbb`. Empty, the default, is the stock blue; so is anything
    /// unparseable. A lowercase `color::TAG_PALETTE` name, what 0.189
    /// wrote, still loads.
    pub accent: String,
    /// Accent colours the user kept from the picker as presets, `#rrggbb`
    /// each, in the order they were saved.
    pub accent_presets: Vec<String>,
    /// The app background as `#rrggbb`; every surface is a fixed step from
    /// it (see `color::bg`). Empty, the default, is the stock graphite.
    pub background: String,
}

/// One saved snapshot of every tunable on the Waveform settings tab. Saving to
/// an occupied slot overwrites it; loading applies the whole snapshot at once,
/// so an in-progress tweak can be parked and recalled without redialing each
/// slider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WaveformPreset {
    /// 1-based UI slot this preset lives in.
    pub slot: u8,
    pub color_mode: WaveformColorMode,
    pub height_exp: f32,
    pub band_gain: [f32; 3],
    pub energy_gain: f32,
    pub band_colors: [[u8; 3]; 3],
    pub energy_colors: [[u8; 3]; 5],
    pub low_hz: f32,
    pub mid_hz: f32,
    pub smoothing: f32,
    pub smooth_attack_ms: f32,
    pub smooth_release_ms: f32,
    pub bass_floor_threshold: f32,
    pub bass_floor_amount: f32,
}

pub(crate) fn default_waveform_height_exp() -> f32 {
    1.01
}

pub(crate) fn default_waveform_band_gain() -> [f32; 3] {
    [1.0, 0.85, 0.38]
}

pub(crate) fn default_waveform_energy_gain() -> f32 {
    0.9
}

pub(crate) fn default_waveform_band_colors() -> [[u8; 3]; 3] {
    [[0, 50, 255], [207, 156, 42], [230, 241, 255]]
}

pub(crate) fn default_waveform_low_hz() -> f32 {
    120.0
}

pub(crate) fn default_waveform_mid_hz() -> f32 {
    2000.0
}

pub(crate) fn default_waveform_smoothing() -> f32 {
    0.79
}

pub(crate) fn default_waveform_smooth_attack_ms() -> f32 {
    1.2
}

pub(crate) fn default_waveform_smooth_release_ms() -> f32 {
    60.0
}

pub(crate) fn default_waveform_bass_floor_threshold() -> f32 {
    0.2
}

pub(crate) fn default_waveform_bass_floor_amount() -> f32 {
    0.25
}

pub(crate) fn default_waveform_energy_colors() -> [[u8; 3]; 5] {
    [
        [45, 80, 150],
        [40, 160, 170],
        [70, 190, 110],
        [235, 195, 70],
        [225, 75, 55],
    ]
}

/// Which library leads the left navigation sidebar. Parsed from
/// `Config::nav_primary`; presentation policy, so it lives in the GUI boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NavPrimary {
    /// The vinyl collection on top, the digital library pinned below.
    #[serde(rename = "vinyl")]
    Vinyl,
    /// Library / New / playlists on top, vinyl pinned below (the default).
    #[serde(rename = "digital", other)]
    Digital,
}

impl NavPrimary {
    /// Label shown in the settings picker.
    pub fn label(self) -> &'static str {
        match self {
            NavPrimary::Digital => "Digital library",
            NavPrimary::Vinyl => "Vinyl collection",
        }
    }

    /// Both options, in picker order.
    pub const ALL: [NavPrimary; 2] = [NavPrimary::Digital, NavPrimary::Vinyl];
}

/// Which candidate the automatic Discogs match commits to when a new import
/// turns up several releases. Parsed from `Config::discogs_auto_match`; GUI
/// policy over the popularity counts every search hit already carries, so
/// picking costs no extra API requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReleaseAutoMatch {
    /// The release the most Discogs users want.
    #[serde(rename = "most_wanted")]
    MostWanted,
    /// The earliest year — usually the original release.
    #[serde(rename = "oldest")]
    Oldest,
    /// Discogs's own first search result.
    #[serde(rename = "top_hit")]
    TopHit,
    /// The release the most Discogs users own — the default, and the strongest
    /// signal for "the normal pressing" over promos and limited variants.
    #[serde(rename = "most_collected", other)]
    MostCollected,
}

impl ReleaseAutoMatch {
    /// Label shown in the settings picker.
    pub fn label(self) -> &'static str {
        match self {
            ReleaseAutoMatch::MostCollected => "Most popular",
            ReleaseAutoMatch::MostWanted => "Most wanted",
            ReleaseAutoMatch::Oldest => "Earliest release",
            ReleaseAutoMatch::TopHit => "Best search match",
        }
    }

    /// One line of hover help per rule.
    pub fn hint(self) -> &'static str {
        match self {
            ReleaseAutoMatch::MostCollected => "The release most Discogs users own",
            ReleaseAutoMatch::MostWanted => "The release most Discogs users want",
            ReleaseAutoMatch::Oldest => "The oldest pressing, usually the original",
            ReleaseAutoMatch::TopHit => "The top Discogs search result",
        }
    }

    /// All rules, in picker order.
    pub const ALL: [ReleaseAutoMatch; 4] = [
        ReleaseAutoMatch::MostCollected,
        ReleaseAutoMatch::MostWanted,
        ReleaseAutoMatch::Oldest,
        ReleaseAutoMatch::TopHit,
    ];
}

/// A physical (or digital) carrier a Discogs release can come on, used to filter
/// the release picker down to the formats the user actually collects.
///
/// Discogs reports a release's format as a free-text list like `Vinyl, 12", 45
/// RPM` or `CD, Album, Reissue`, mixing the carrier with descriptors. Only the
/// carrier matters here, so each variant matches on the substrings Discogs
/// actually uses for it. `Other` is the catch-all that keeps an unrecognized or
/// blank format visible rather than silently dropping it — better to show one
/// odd row than to hide the pressing the user was looking for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseMedium {
    Vinyl,
    Cd,
    Digital,
    Cassette,
    Other,
}

impl ReleaseMedium {
    /// Classify a Discogs format string. Non-vinyl carriers are tested first so
    /// a `CD, Comp` can't be pulled into `Vinyl` by the `lp` inside a word like
    /// "sampler" — the same ordering [`crate::dig`]'s vinyl filter relies on.
    pub fn classify(format: &str) -> Self {
        let f = format.to_ascii_lowercase();
        if f.trim().is_empty() {
            return ReleaseMedium::Other;
        }
        if f.contains("cd") || f.contains("dvd") {
            return ReleaseMedium::Cd;
        }
        if f.contains("file") || f.contains("mp3") || f.contains("flac") {
            return ReleaseMedium::Digital;
        }
        if f.contains("cassette") {
            return ReleaseMedium::Cassette;
        }
        if f.contains("vinyl")
            || f.contains("lp")
            || f.contains("12\"")
            || f.contains("10\"")
            || f.contains("7\"")
            || f.contains("shellac")
        {
            return ReleaseMedium::Vinyl;
        }
        ReleaseMedium::Other
    }

    /// Stable lowercase key stored in the config TOML.
    pub fn key(self) -> &'static str {
        match self {
            ReleaseMedium::Vinyl => "vinyl",
            ReleaseMedium::Cd => "cd",
            ReleaseMedium::Digital => "digital",
            ReleaseMedium::Cassette => "cassette",
            ReleaseMedium::Other => "other",
        }
    }

    /// Label shown next to the setting's checkbox.
    pub fn label(self) -> &'static str {
        match self {
            ReleaseMedium::Vinyl => "Vinyl",
            ReleaseMedium::Cd => "CD / DVD",
            ReleaseMedium::Digital => "Digital / file",
            ReleaseMedium::Cassette => "Cassette",
            ReleaseMedium::Other => "Other or unlisted",
        }
    }

    /// One line of hover help per medium.
    pub fn hint(self) -> &'static str {
        match self {
            ReleaseMedium::Vinyl => "Records: LPs, 12\", 10\", 7\", shellac",
            ReleaseMedium::Cd => "Compact discs and DVDs",
            ReleaseMedium::Digital => "Download and streaming releases",
            ReleaseMedium::Cassette => "Tapes",
            ReleaseMedium::Other => "Anything Discogs lists no format for",
        }
    }

    /// All mediums, in the order the settings list shows them.
    pub const ALL: [ReleaseMedium; 5] = [
        ReleaseMedium::Vinyl,
        ReleaseMedium::Cd,
        ReleaseMedium::Digital,
        ReleaseMedium::Cassette,
        ReleaseMedium::Other,
    ];
}

/// Which section the app selects on launch. Parsed from `Config::startup_view`;
/// presentation policy, so it lives in the GUI boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StartupView {
    /// The Discogs vinyl collection grid.
    #[serde(rename = "vinyl")]
    Vinyl,
    /// The self-clearing inbox of fresh imports.
    #[serde(rename = "recent")]
    Recent,
    /// The whole catalog ("Library") — the default.
    #[serde(rename = "library", other)]
    Library,
}

impl StartupView {
    /// Label shown in the settings picker.
    pub fn label(self) -> &'static str {
        match self {
            StartupView::Library => "Library",
            StartupView::Vinyl => "Vinyl collection",
            StartupView::Recent => "New imports",
        }
    }

    /// All options, in picker order.
    pub const ALL: [StartupView; 3] = [
        StartupView::Library,
        StartupView::Vinyl,
        StartupView::Recent,
    ];
}

/// What a USB export puts in the comment a CDJ shows under the song info.
/// Parsed from `Config::export_comment`; export policy, so it lives in the
/// GUI boundary and the exporter only ever sees a finished comment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExportComment {
    /// The track's Ordnung tags, `dub, dark`.
    #[serde(rename = "tags")]
    Tags,
    /// Notes, then the tags on a second line.
    #[serde(rename = "notes_tags")]
    NotesAndTags,
    /// An empty comment.
    #[serde(rename = "none")]
    None,
    /// The track's Notes tag, as it is.
    #[serde(rename = "notes", other)]
    Notes,
}

#[cfg(feature = "usb-export")]
impl ExportComment {
    pub const ALL: [ExportComment; 4] = [
        ExportComment::Notes,
        ExportComment::Tags,
        ExportComment::NotesAndTags,
        ExportComment::None,
    ];

    /// The dropdown's wording. Notes is the default because it is what
    /// rekordbox itself puts there: the track's comment tag.
    pub fn label(self) -> &'static str {
        match self {
            ExportComment::Notes => "Track notes (what rekordbox shows)",
            ExportComment::Tags => "Ord tags",
            ExportComment::NotesAndTags => "Notes and tags",
            ExportComment::None => "Nothing",
        }
    }

    /// Whether this choice reads the song's tags at all (so the export can
    /// skip loading them otherwise).
    pub fn uses_tags(self) -> bool {
        matches!(self, ExportComment::Tags | ExportComment::NotesAndTags)
    }

    /// The comment to export for a track with these `notes` and `tags`
    /// (`dub, dark`, empty when untagged). Empty parts are dropped so an
    /// untagged track under "Notes and tags" shows its notes alone.
    pub fn compose(self, notes: &str, tags: &str) -> String {
        let (notes, tags) = (notes.trim(), tags.trim());
        match self {
            ExportComment::Notes => notes.to_string(),
            ExportComment::Tags => tags.to_string(),
            ExportComment::NotesAndTags => [notes, tags]
                .iter()
                .filter(|s| !s.is_empty())
                .copied()
                .collect::<Vec<_>>()
                .join("\n"),
            ExportComment::None => String::new(),
        }
    }
}

/// How the player waveform is colored. Parsed from `Config::waveform_color_mode`;
/// presentation policy, so it lives in the GUI boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WaveformColorMode {
    /// Additive RGB from the low/mid/high band balance (rekordbox/Serato style).
    #[serde(rename = "spectrum")]
    Spectrum,
    /// Cool→hot gradient driven by each section's total energy.
    #[serde(rename = "energy", other)]
    Energy,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            discogs_token: String::new(),
            discogs_auto_fetch: true,
            discogs_auto_match: ReleaseAutoMatch::MostCollected,
            discogs_skip_fields: Vec::new(),
            discogs_username: String::new(),
            hidden_release_mediums: Vec::new(),
            library_root: None,
            column_order: Vec::new(),
            hidden_columns: Vec::new(),
            column_widths: BTreeMap::new(),
            default_sort: String::new(),
            default_sort_ascending: true,
            vinyl_sort: crate::views::VinylSort::Added,
            vinyl_sort_ascending: false,
            vinyl_view: "grid".to_string(),
            radio_dock: crate::radio::RadioDock::Top,
            graph_arrange: crate::graph::Arrange::Artists,
            graph_hide: Vec::new(),
            nav_primary: NavPrimary::Digital,
            export_convert_for_older_players: false,
            export_comment: ExportComment::Notes,
            export_db_id: 0,
            nav_density: crate::sidebar::NavDensity::Narrow,
            nav_collapsed: Vec::new(),
            inspector_density: crate::inspector::InspectorDensity::Expanded,
            inspector_open: false,
            startup_view: StartupView::Library,
            auto_analyze: true,
            cue_bar_open: false,
            cue_quantize: true,
            auto_write_tags: true,
            onboarding_completed_version: 0,
            convert_format: "aiff".to_string(),
            convert_bitrate_kbps: String::new(),
            convert_out_dir: None,
            convert_in_place: true,
            auto_convert: false,
            auto_convert_sources: Vec::new(),
            volume: 1.0,
            key_lock: false,
            vinyl_mode: false,
            mid_start_videos: false,
            mid_start_files: false,
            waveform_color_mode: WaveformColorMode::Energy,
            waveform_height_exp: default_waveform_height_exp(),
            wave_zoom_steps: false,
            waveform_band_gain: default_waveform_band_gain(),
            waveform_energy_gain: default_waveform_energy_gain(),
            waveform_band_colors: default_waveform_band_colors(),
            waveform_energy_colors: default_waveform_energy_colors(),
            waveform_low_hz: default_waveform_low_hz(),
            waveform_mid_hz: default_waveform_mid_hz(),
            waveform_smoothing: default_waveform_smoothing(),
            waveform_smooth_attack_ms: default_waveform_smooth_attack_ms(),
            waveform_smooth_release_ms: default_waveform_smooth_release_ms(),
            waveform_bass_floor_threshold: default_waveform_bass_floor_threshold(),
            waveform_bass_floor_amount: default_waveform_bass_floor_amount(),
            waveform_presets: Vec::new(),
            zebra_rows: true,
            zebra_fill: false,
            row_shade: crate::ui::tokens::color::ROW_SHADE,
            accent: String::new(),
            accent_presets: Vec::new(),
            background: String::new(),
        }
    }
}

impl Config {
    /// The chosen accent, `None` for the stock blue.
    pub fn accent_color(&self) -> Option<eframe::egui::Color32> {
        use crate::ui::tokens::color;
        color::from_hex(&self.accent).or_else(|| {
            color::TAG_PALETTE
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case(&self.accent))
                .map(|(_, c)| *c)
        })
    }

    /// Push the appearance settings into the running style: at launch,
    /// and again whenever one of them changes.
    pub fn apply_appearance(&self, ctx: &eframe::egui::Context) {
        crate::ui::tokens::color::set_background(crate::ui::tokens::color::from_hex(&self.background));
        crate::ui::tokens::color::set_accent(self.accent_color());
        crate::ui::theme::restyle(ctx, self.row_shade);
    }

    /// Whether releases on `medium` should be shown in the release picker.
    pub fn shows_release_medium(&self, medium: ReleaseMedium) -> bool {
        !self
            .hidden_release_mediums
            .iter()
            .any(|k| k == medium.key())
    }

    /// Whether a Discogs format string passes the user's medium filter.
    ///
    /// Hiding *every* medium would leave the picker permanently empty and make
    /// the app look broken, so an all-hidden config is treated as no filter at
    /// all — the setting is a way to narrow the list, never to disable matching.
    pub fn shows_release_format(&self, format: &str) -> bool {
        if ReleaseMedium::ALL
            .iter()
            .all(|m| !self.shows_release_medium(*m))
        {
            return true;
        }
        self.shows_release_medium(ReleaseMedium::classify(format))
    }

    /// Show or hide one medium in the release picker. Caller persists with
    /// [`Config::save`].
    pub fn set_release_medium_shown(&mut self, medium: ReleaseMedium, shown: bool) {
        self.hidden_release_mediums.retain(|k| k != medium.key());
        if !shown {
            self.hidden_release_mediums.push(medium.key().to_string());
        }
    }

    /// Whether a matched release writes what `key` names, a [`FillField::key`]
    /// or [`COVER_FILL`]. Everything is written unless the key sits in
    /// `discogs_skip_fields`.
    pub fn fills(&self, key: &str) -> bool {
        !self.discogs_skip_fields.iter().any(|k| k == key)
    }

    pub fn set_fill(&mut self, key: &str, on: bool) {
        self.discogs_skip_fields.retain(|k| k != key);
        if !on {
            self.discogs_skip_fields.push(key.to_string());
        }
    }

    /// The tag fields a match leaves alone, for `Catalog::apply_release_tags`.
    pub fn skipped_fill_fields(&self) -> Vec<FillField> {
        FillField::ALL.into_iter().filter(|f| !self.fills(f.key())).collect()
    }

    /// The waveform preset saved in 1-based `slot`, if any.
    pub fn waveform_preset(&self, slot: u8) -> Option<&WaveformPreset> {
        self.waveform_presets.iter().find(|p| p.slot == slot)
    }

    /// Snapshot the current waveform settings into `slot`, overwriting whatever
    /// was there. Caller persists with [`Config::save`].
    pub fn save_waveform_preset(&mut self, slot: u8) {
        let preset = WaveformPreset {
            slot,
            color_mode: self.waveform_color_mode,
            height_exp: self.waveform_height_exp,
            band_gain: self.waveform_band_gain,
            energy_gain: self.waveform_energy_gain,
            band_colors: self.waveform_band_colors,
            energy_colors: self.waveform_energy_colors,
            low_hz: self.waveform_low_hz,
            mid_hz: self.waveform_mid_hz,
            smoothing: self.waveform_smoothing,
            smooth_attack_ms: self.waveform_smooth_attack_ms,
            smooth_release_ms: self.waveform_smooth_release_ms,
            bass_floor_threshold: self.waveform_bass_floor_threshold,
            bass_floor_amount: self.waveform_bass_floor_amount,
        };
        self.waveform_presets.retain(|p| p.slot != slot);
        self.waveform_presets.push(preset);
        // Keep the saved TOML stable regardless of save order.
        self.waveform_presets.sort_by_key(|p| p.slot);
    }

    /// Apply the preset in `slot` to the live settings. Returns whether the
    /// band crossovers changed — the caller must then invalidate the loaded
    /// track's hi-res bands so the zoom lane recomputes — or `None` if the slot
    /// is empty.
    pub fn load_waveform_preset(&mut self, slot: u8) -> Option<bool> {
        let p = self.waveform_preset(slot)?.clone();
        let freq_changed = p.low_hz != self.waveform_low_hz || p.mid_hz != self.waveform_mid_hz;
        self.waveform_color_mode = p.color_mode;
        self.waveform_height_exp = p.height_exp;
        self.waveform_band_gain = p.band_gain;
        self.waveform_energy_gain = p.energy_gain;
        self.waveform_band_colors = p.band_colors;
        self.waveform_energy_colors = p.energy_colors;
        self.waveform_low_hz = p.low_hz;
        self.waveform_mid_hz = p.mid_hz;
        self.waveform_smoothing = p.smoothing;
        self.waveform_smooth_attack_ms = p.smooth_attack_ms;
        self.waveform_smooth_release_ms = p.smooth_release_ms;
        self.waveform_bass_floor_threshold = p.bass_floor_threshold;
        self.waveform_bass_floor_amount = p.bass_floor_amount;
        Some(freq_changed)
    }

    /// Load settings from disk, or return defaults if the file is missing or
    /// unreadable. Never fails: a broken/absent config simply yields defaults.
    pub fn load() -> Self {
        config_path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|text| toml::from_str(&text).ok())
            .unwrap_or_default()
    }

    /// Persist settings to `~/.ordnung/config.toml`, creating the directory if
    /// needed. Returns a user-facing error string on failure.
    pub fn save(&self) -> Result<(), String> {
        let path = config_path().ok_or("could not resolve HOME for config path")?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let text = toml::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(&path, text).map_err(|e| e.to_string())
    }
}

/// `~/.ordnung/config.toml` — same directory as the catalog database.
pub fn config_path() -> Option<PathBuf> {
    Some(ordnung_core::tools::data_dir()?.join("config.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The exported comment follows the choice, drops empty parts, and an
    /// old config without the key still means "notes".
    #[test]
    #[cfg(feature = "usb-export")]
    fn export_comment_composes_per_choice() {
        use ExportComment::*;
        assert_eq!(Notes.compose(" late night ", "dub, dark"), "late night");
        assert_eq!(Tags.compose("late night", "dub, dark"), "dub, dark");
        assert_eq!(NotesAndTags.compose("late night", "dub, dark"), "late night\ndub, dark");
        assert_eq!(NotesAndTags.compose("", "dub, dark"), "dub, dark");
        assert_eq!(NotesAndTags.compose("late night", ""), "late night");
        assert_eq!(None.compose("late night", "dub, dark"), "");
        let c: Config = toml::from_str("").unwrap();
        assert_eq!(c.export_comment, Notes);
    }

    #[test]
    fn classify_reads_the_carrier_out_of_a_discogs_format_string() {
        use ReleaseMedium::*;
        assert_eq!(ReleaseMedium::classify("Vinyl, 12\", 45 RPM"), Vinyl);
        assert_eq!(ReleaseMedium::classify("Vinyl, LP, Album"), Vinyl);
        assert_eq!(ReleaseMedium::classify("2 x Vinyl, 12\""), Vinyl);
        assert_eq!(ReleaseMedium::classify("Shellac, 10\""), Vinyl);
        assert_eq!(ReleaseMedium::classify("CD, Album, Reissue"), Cd);
        assert_eq!(ReleaseMedium::classify("DVD, Compilation"), Cd);
        assert_eq!(ReleaseMedium::classify("File, MP3, 320 kbps"), Digital);
        assert_eq!(ReleaseMedium::classify("Cassette, Album"), Cassette);
        // A master row names no format; it must stay visible rather than vanish.
        assert_eq!(ReleaseMedium::classify(""), Other);
        assert_eq!(ReleaseMedium::classify("Box Set"), Other);
    }

    /// The ordering trap the dig filter already guards against: "sampler"
    /// contains "lp", so a CD comp must be classified before vinyl is tried.
    #[test]
    fn classify_does_not_let_a_cd_sampler_pass_as_vinyl() {
        assert_eq!(ReleaseMedium::classify("CD, Sampler"), ReleaseMedium::Cd);
    }

    #[test]
    fn hiding_a_medium_filters_only_that_carrier() {
        let mut cfg = Config::default();
        // Default is show-everything.
        assert!(cfg.shows_release_format("CD, Album"));
        assert!(cfg.shows_release_format("Vinyl, 12\""));

        cfg.set_release_medium_shown(ReleaseMedium::Cd, false);
        cfg.set_release_medium_shown(ReleaseMedium::Digital, false);
        assert!(!cfg.shows_release_format("CD, Album"));
        assert!(!cfg.shows_release_format("File, FLAC"));
        assert!(cfg.shows_release_format("Vinyl, 12\""));
        // Unlisted formats stay visible: better one odd row than a hidden pressing.
        assert!(cfg.shows_release_format(""));

        // Re-showing removes the key rather than stacking duplicates.
        cfg.set_release_medium_shown(ReleaseMedium::Cd, true);
        assert!(cfg.shows_release_format("CD, Album"));
        assert_eq!(cfg.hidden_release_mediums, vec!["digital".to_string()]);
    }

    #[test]
    fn skipping_a_field_leaves_only_that_field_out() {
        let mut cfg = Config::default();
        assert!(cfg.skipped_fill_fields().is_empty());
        cfg.set_fill("country", false);
        cfg.set_fill("country", false);
        cfg.set_fill("year", false);
        cfg.set_fill(COVER_FILL, false);
        assert_eq!(cfg.skipped_fill_fields(), [FillField::Country, FillField::Year]);
        assert!(!cfg.fills(COVER_FILL));
        // Re-filling removes the key rather than stacking duplicates.
        cfg.set_fill("country", true);
        assert_eq!(cfg.discogs_skip_fields, ["year", "cover"]);
    }

    /// Hiding everything would make the picker permanently empty, so it's
    /// treated as no filter at all.
    #[test]
    fn hiding_every_medium_is_ignored_rather_than_hiding_everything() {
        let mut cfg = Config::default();
        for m in ReleaseMedium::ALL {
            cfg.set_release_medium_shown(m, false);
        }
        assert!(cfg.shows_release_format("Vinyl, 12\""));
        assert!(cfg.shows_release_format("CD, Album"));
    }

    /// A config predating the library root loads with none set (music keeps
    /// arriving only via explicit adds), and a chosen root survives the TOML
    /// round trip.
    #[test]
    fn library_root_defaults_to_none_and_round_trips() {
        let old: Config = toml::from_str("").unwrap();
        assert_eq!(old.library_root, None);

        let cfg = Config {
            library_root: Some(PathBuf::from("/Users/dj/Music/seeker")),
            ..Config::default()
        };
        let text = toml::to_string_pretty(&cfg).unwrap();
        let back: Config = toml::from_str(&text).unwrap();
        assert_eq!(
            back.library_root,
            Some(PathBuf::from("/Users/dj/Music/seeker"))
        );
    }

    /// The one-switch `mid_start` of v0.153 carries over to the records
    /// switch, and the files switch starts off; both round-trip once set.
    #[test]
    fn mid_start_splits_into_two_switches_and_keeps_the_old_key() {
        let old: Config = toml::from_str("mid_start = true").unwrap();
        assert!(old.mid_start_videos);
        assert!(!old.mid_start_files);

        let cfg = Config {
            mid_start_videos: false,
            mid_start_files: true,
            ..Config::default()
        };
        let back: Config = toml::from_str(&toml::to_string_pretty(&cfg).unwrap()).unwrap();
        assert!(!back.mid_start_videos);
        assert!(back.mid_start_files);
    }

    /// Proves the token survives a save → fresh-load cycle (the whole point of
    /// the feature). Uses a throwaway HOME so it touches no real config.
    #[test]
    fn token_round_trips_through_disk() {
        let tmp = std::env::temp_dir().join(format!("ordnung-cfg-test-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        // SAFETY: single-threaded test; we restore HOME before returning.
        let prev = std::env::var_os("HOME");
        std::env::set_var("HOME", &tmp);

        let cfg = Config {
            discogs_token: "secret-token-123".into(),
            ..Config::default()
        };
        cfg.save().unwrap();

        // A brand-new load (no shared state) must see the saved token.
        let loaded = Config::load();
        assert_eq!(loaded.discogs_token, "secret-token-123");

        // The file really lives at ~/.ordnung/config.toml.
        assert!(tmp.join(".ordnung/config.toml").exists());

        match prev {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
        std::fs::remove_dir_all(&tmp).ok();
    }
    const FULL: &str = include_str!("../tests/fixtures/config-full.toml");
    const DEFAULTS: &str = include_str!("../tests/fixtures/config-defaults.toml");
    const LEGACY: &str = include_str!("../tests/fixtures/config-legacy.toml");

    /// The file format is pinned: a config written by this build, with every
    /// field off its default, parses to exactly these values and writes back
    /// byte for byte.
    #[test]
    fn full_fixture_parses_every_field_and_writes_back_identically() {
        use crate::graph::Arrange;
        use crate::inspector::InspectorDensity;
        use crate::radio::RadioDock;
        use crate::sidebar::NavDensity;
        use crate::views::VinylSort;
        let expected = Config {
            discogs_token: "tok-abc".into(),
            hidden_release_mediums: ["vinyl", "cd", "digital", "cassette", "other"]
                .map(String::from)
                .to_vec(),
            discogs_auto_fetch: false,
            discogs_auto_match: ReleaseAutoMatch::Oldest,
            discogs_skip_fields: ["cover", "country", "release_date"].map(String::from).to_vec(),
            discogs_username: "kai".into(),
            library_root: Some(PathBuf::from("/Users/dj/Music/seeker")),
            column_order: [
                "notes", "quality", "bitrate", "format", "key", "bpm", "added", "waveform",
                "duration", "tags", "genre", "album", "title", "artist", "cover",
            ]
            .map(String::from)
            .to_vec(),
            hidden_columns: ["waveform", "notes"].map(String::from).to_vec(),
            column_widths: [("artist".to_string(), 180.5), ("title".to_string(), 240.0)]
                .into_iter()
                .collect(),
            default_sort: "bpm".into(),
            default_sort_ascending: false,
            vinyl_sort: VinylSort::Price,
            vinyl_sort_ascending: true,
            vinyl_view: "list".into(),
            radio_dock: RadioDock::Left,
            graph_arrange: Arrange::Genres,
            graph_hide: ["owned", "wanted", "dug"].map(String::from).to_vec(),
            nav_primary: NavPrimary::Vinyl,
            export_convert_for_older_players: true,
            export_comment: ExportComment::NotesAndTags,
            nav_density: NavDensity::Icon,
            nav_collapsed: ["playlists", "crates", "tags"].map(String::from).to_vec(),
            inspector_density: InspectorDensity::Compact,
            inspector_open: true,
            startup_view: StartupView::Recent,
            auto_analyze: false,
            cue_bar_open: true,
            cue_quantize: false,
            auto_write_tags: false,
            onboarding_completed_version: 3,
            convert_format: "mp3".into(),
            convert_bitrate_kbps: "256".into(),
            convert_out_dir: Some(PathBuf::from("/Users/dj/Music/converted")),
            convert_in_place: false,
            auto_convert: true,
            auto_convert_sources: ["flac", "wav", "aiff", "aac", "mp3"].map(String::from).to_vec(),
            volume: 0.5,
            key_lock: true,
            vinyl_mode: true,
            mid_start_videos: true,
            mid_start_files: true,
            waveform_color_mode: WaveformColorMode::Spectrum,
            waveform_height_exp: 1.5,
            wave_zoom_steps: true,
            waveform_band_gain: [0.875, 0.75, 0.625],
            waveform_energy_gain: 1.125,
            waveform_band_colors: [[1, 2, 3], [4, 5, 6], [7, 8, 9]],
            waveform_energy_colors: [[10, 11, 12], [13, 14, 15], [16, 17, 18], [19, 20, 21], [22, 23, 24]],
            waveform_low_hz: 100.0,
            waveform_mid_hz: 1800.0,
            waveform_smoothing: 0.5,
            waveform_smooth_attack_ms: 2.5,
            waveform_smooth_release_ms: 45.0,
            waveform_bass_floor_threshold: 0.375,
            waveform_bass_floor_amount: 0.25,
            waveform_presets: vec![
                WaveformPreset {
                    slot: 1,
                    color_mode: WaveformColorMode::Energy,
                    height_exp: 1.25,
                    band_gain: [0.5, 0.625, 0.75],
                    energy_gain: 0.75,
                    band_colors: [[30, 31, 32], [33, 34, 35], [36, 37, 38]],
                    energy_colors: [[40, 41, 42], [43, 44, 45], [46, 47, 48], [49, 50, 51], [52, 53, 54]],
                    low_hz: 90.0,
                    mid_hz: 1500.0,
                    smoothing: 0.25,
                    smooth_attack_ms: 3.0,
                    smooth_release_ms: 80.0,
                    bass_floor_threshold: 0.125,
                    bass_floor_amount: 0.625,
                },
                WaveformPreset {
                    slot: 5,
                    color_mode: WaveformColorMode::Spectrum,
                    height_exp: 2.0,
                    band_gain: [1.5, 1.25, 1.0],
                    energy_gain: 1.5,
                    band_colors: [[60, 61, 62], [63, 64, 65], [66, 67, 68]],
                    energy_colors: [[70, 71, 72], [73, 74, 75], [76, 77, 78], [79, 80, 81], [82, 83, 84]],
                    low_hz: 150.0,
                    mid_hz: 2500.0,
                    smoothing: 1.0,
                    smooth_attack_ms: 0.5,
                    smooth_release_ms: 120.0,
                    bass_floor_threshold: 0.5,
                    bass_floor_amount: 0.875,
                },
            ],
            zebra_rows: false,
            zebra_fill: true,
            row_shade: 20,
            accent: "#bf5af2".to_string(),
            accent_presets: vec!["#ff453a".to_string(), "#30d158".to_string()],
            background: "#10131f".to_string(),
        };
        let parsed: Config = toml::from_str(FULL).unwrap();
        // Every field, through Debug: exact for every type the config holds,
        // and the literal above cannot omit a field.
        assert_eq!(format!("{parsed:?}"), format!("{expected:?}"));
        assert_eq!(toml::to_string_pretty(&parsed).unwrap(), FULL);
    }

    /// The defaults are pinned on disk, and an empty file loads exactly them.
    #[test]
    fn defaults_are_pinned_and_an_empty_file_loads_them() {
        assert_eq!(toml::to_string_pretty(&Config::default()).unwrap(), DEFAULTS);
        let empty: Config = toml::from_str("").unwrap();
        assert_eq!(format!("{empty:?}"), format!("{:?}", Config::default()));
    }

    /// Old keys and values no build ever wrote keep loading, each landing on
    /// its documented fallback.
    #[test]
    fn legacy_keys_and_unknown_values_still_load() {
        use crate::graph::{Arrange, Status};
        use crate::inspector::InspectorDensity;
        use crate::radio::RadioDock;
        use crate::sidebar::NavDensity;
        use crate::util::format_from_key;
        use crate::views::VinylSort;
        use crate::TableColumn;

        let c: Config = toml::from_str(LEGACY).unwrap();
        assert!(c.mid_start_videos);
        assert!(!c.mid_start_files);
        assert_eq!(c.nav_density, NavDensity::Narrow);
        assert_eq!(c.inspector_density, InspectorDensity::Expanded);
        assert_eq!(c.discogs_auto_match, ReleaseAutoMatch::MostCollected);
        assert_eq!(c.startup_view, StartupView::Library);
        assert_eq!(c.export_comment, ExportComment::Notes);
        assert_eq!(c.nav_primary, NavPrimary::Digital);
        assert_eq!(c.waveform_color_mode, WaveformColorMode::Energy);
        assert_eq!(c.radio_dock, RadioDock::Top);
        assert_eq!(c.graph_arrange, Arrange::Artists);
        assert_eq!(c.vinyl_sort, VinylSort::Artist);
        assert_eq!(format_from_key(&c.convert_format), None);
        assert!(TableColumn::from_key(&c.default_sort).is_none());
        assert!(Status::hidden(&c.graph_hide).is_empty());
        assert!(c.shows_release_format("CD, Album"));
        assert_eq!(
            c.column_order
                .iter()
                .map(|k| TableColumn::from_key(k).map(TableColumn::key))
                .collect::<Vec<_>>(),
            [None, Some("artist")]
        );
        // The grid/list switch is private to `views`; the string reaches it
        // untouched.
        assert_eq!(c.vinyl_view, "nope");
        assert_eq!(c.accent_color(), Some(crate::ui::tokens::color::PURPLE));
        assert_eq!(c.row_shade, crate::ui::tokens::color::ROW_SHADE);
    }

    /// Every spelling each enum-valued field accepts writes back as itself,
    /// and each enum's key pairs agree with those spellings.
    #[test]
    fn every_enum_key_round_trips_through_the_file() {
        use crate::graph::{Arrange, Status};
        use crate::inspector::InspectorDensity;
        use crate::radio::RadioDock;
        use crate::sidebar::NavDensity;
        use crate::util::format_from_key;
        use crate::views::VinylSort;
        use crate::TableColumn;

        const COLUMNS: [&str; 15] = [
            "cover", "artist", "title", "album", "genre", "tags", "duration", "waveform",
            "added", "bpm", "key", "format", "bitrate", "quality", "notes",
        ];
        const KEYS: &[(&str, &[&str])] = &[
            ("discogs_auto_match", &["most_collected", "most_wanted", "oldest", "top_hit"]),
            ("vinyl_sort", &["added", "price", "artist"]),
            ("vinyl_view", &["grid", "list"]),
            ("radio_dock", &["top", "bottom", "left", "right"]),
            ("graph_arrange", &["artists", "genres"]),
            ("nav_primary", &["digital", "vinyl"]),
            ("export_comment", &["notes", "tags", "notes_tags", "none"]),
            ("nav_density", &["icon", "narrow"]),
            ("inspector_density", &["narrow", "regular"]),
            ("startup_view", &["library", "vinyl", "recent"]),
            ("waveform_color_mode", &["energy", "spectrum"]),
            ("convert_format", &["mp3", "aac", "flac", "wav", "aiff"]),
            ("default_sort", &COLUMNS),
        ];
        for (field, keys) in KEYS {
            for key in *keys {
                let line = format!("{field} = \"{key}\"");
                let c: Config = toml::from_str(&line).unwrap();
                let out = toml::to_string_pretty(&c).unwrap();
                assert!(out.lines().any(|l| l == line), "{line} wrote back as something else");
            }
        }

        /// Every variant survives the round trip, and an unknown key lands
        /// on the fallback.
        fn pin<T: PartialEq + std::fmt::Debug + Copy + Serialize + serde::de::DeserializeOwned>(
            all: &[T],
            fallback: T,
        ) {
            #[derive(Serialize, Deserialize)]
            struct Field<T> {
                v: T,
            }
            for v in all {
                let text = toml::to_string(&Field { v: *v }).unwrap();
                assert_eq!(toml::from_str::<Field<T>>(&text).unwrap().v, *v);
            }
            assert_eq!(toml::from_str::<Field<T>>("v = \"nope\"").unwrap().v, fallback);
        }
        pin(&NavPrimary::ALL, NavPrimary::Digital);
        pin(&ReleaseAutoMatch::ALL, ReleaseAutoMatch::MostCollected);
        pin(&StartupView::ALL, StartupView::Library);
        #[cfg(feature = "usb-export")]
        pin(&ExportComment::ALL, ExportComment::Notes);
        pin(&[WaveformColorMode::Energy, WaveformColorMode::Spectrum], WaveformColorMode::Energy);
        pin(&[RadioDock::Top, RadioDock::Bottom, RadioDock::Left, RadioDock::Right], RadioDock::Top);
        pin(&[Arrange::Artists, Arrange::Genres], Arrange::Artists);
        pin(&[VinylSort::Added, VinylSort::Price, VinylSort::Artist], VinylSort::Artist);
        pin(&NavDensity::ALL, NavDensity::Narrow);
        pin(&InspectorDensity::ALL, InspectorDensity::Expanded);
        assert_eq!(
            ReleaseMedium::ALL.map(ReleaseMedium::key),
            ["vinyl", "cd", "digital", "cassette", "other"]
        );
        assert_eq!(
            FillField::ALL.map(FillField::key),
            ["genre", "label", "catalog_number", "country", "album", "release_date", "year"]
        );
        for s in Status::ALL {
            assert_eq!(Status::hidden(&[s.key().to_string()]), [s]);
        }
        assert_eq!(TableColumn::DEFAULT_ORDER.map(TableColumn::key), COLUMNS);
        for col in TableColumn::DEFAULT_ORDER {
            assert!(TableColumn::from_key(col.key()) == Some(col));
        }
        for key in ["mp3", "aac", "flac", "wav", "aiff"] {
            assert_eq!(format_from_key(key).map(crate::util::format_key), Some(key));
        }
    }
}
