//! The browse page: one label's or one artist's discography, read front to back.
//!
//! The dig's artist and label threads sample — one random find per pull. This
//! is the deliberate version: click a label anywhere (the record sheet's
//! header, a shelf or seller card's menu), or an artist in the search box's
//! Discogs results, and read the whole catalog as a list, with your own
//! shelves marked on every row. For the labels that define a sound — Chain
//! Reaction, Basic Channel, Underground Resistance — and for the artists whose
//! every 12" is worth knowing, reading the run in order *is* the dig.
//!
//! Rides `GET /labels/{id}/releases` and `GET /artists/{id}/releases` (the
//! same browses the dig's threads use). A label's run is read 100 rows a
//! page, in the order Discogs returns them: catalog order. An artist's is
//! read whole (500 rows a request, the rest behind the first) and sorted
//! here, because Discogs hands it back by *role* (their own records, then
//! the remixes, then the appearances), each group starting over at their
//! first year: by date, one timeline; or by alias, their own name's records
//! first and then each other name they record under
//! (`Config::artist_run_by_alias`). A record credited to one of those other
//! names (an alias, a group, a name variation, read from `GET
//! /artists/{id}`) wears that name as a pill in place of the credit. Rows are
//! narrowed by the same "Release formats" setting the Discogs release picker
//! honours (`Config::shows_release_format`): hide CDs and files there and
//! they drop out of the run here too. Master rows with no format of their
//! own are kept rather than mislabeled. A run with nothing on the shown
//! formats at all (a digital-only label for a records-only digger) shows
//! every format instead of an empty page. One paced request per page.

use super::*;
use ordnung_core::discogs::{BrowsePage, BrowseRelease, BrowseThread};

/// Window width — versions-panel wide: rows carry year, format and catalog
/// number beside the credit, and wrapping those makes the run unreadable.
const PANEL_W: f32 = 560.0;

/// Side of a row's sleeve thumbnail.
const THUMB: f32 = 48.0;

/// Rows of an artist's sorted run shown at a time: a label page's size, so a
/// page costs the frame what it does today however long the run.
const RUN_PAGE: usize = 100;

/// The open browse page: whose run (a label's or an artist's), which page of
/// it, and the rows.
pub(crate) struct BrowsePanel {
    /// Which association the page follows — the imprint's catalog or the
    /// artist's discography. Decides the endpoint, the title glyph and what a
    /// row's caption has room for.
    pub thread: BrowseThread,
    /// Discogs label or artist id — what the browse actually pages. 0 while
    /// the id is still being resolved from the release the page was opened
    /// from (label pages only; an artist page is opened by id).
    pub id: u64,
    /// Name for the title bar, best known so far (a release's own label string
    /// until the detail resolves the canonical one).
    pub name: String,
    /// 1-based page currently shown (or being fetched).
    pub page: u32,
    /// Total pages, at least 1 once loaded: Discogs's count for a label,
    /// pages of [`RUN_PAGE`] rows of the sorted whole for an artist.
    pub pages: u32,
    /// Total releases across the run, as Discogs counts them (all formats).
    pub items: u32,
    /// The rows, vinyl-filtered and pressing-deduplicated: this page's for a
    /// label, the whole sorted run's for an artist.
    pub releases: Vec<BrowseRelease>,
    pub loading: bool,
    /// Why there's nothing to show, when there isn't.
    pub error: Option<String>,
    /// Every page of this run fetched so far, raw, keyed by Discogs's page
    /// number — a turn back to a page already read costs nothing, and the
    /// page after the one on screen is read ahead into here (see
    /// [`App::browse_lookahead`]). An artist's run is read into here whole.
    pub cache: HashMap<u32, BrowsePage>,
    /// The page being read ahead, if one is.
    pub ahead: Option<u32>,
    /// An artist's kin (aliases, groups, name variations), once read: which
    /// rows wear a pill, and the by-alias grouping. A label has none.
    pub kin: Option<discogs::ArtistDetail>,
    /// Pages Discogs serves of an artist's run, 0 until the first arrives;
    /// the run is being read until `cache` holds them all.
    pub run_pages: u32,
    /// The names the alias bar offers, each a run of its own: the artist
    /// first, then their aliases, then the groups they are in (true). Set
    /// from the first run's kin and carried along as the bar switches runs,
    /// so it holds still under the cursor. Empty until the kin lands.
    pub family: Vec<(discogs::NamedRef, bool)>,
}

impl BrowsePanel {
    /// Pages of an artist's run are still on their way.
    fn reading(&self) -> bool {
        self.thread == BrowseThread::Artist && self.cache.len() < self.run_pages as usize
    }

    /// The artist's name as Discogs spells it, and the other names the same
    /// act records under (aliases, groups, name variations), once the kin
    /// has arrived.
    fn names(&self) -> (String, Vec<String>) {
        match &self.kin {
            Some(k) => (
                k.name.clone(),
                k.aliases
                    .iter()
                    .chain(&k.groups)
                    .map(|r| r.name.clone())
                    .chain(k.namevariations.iter().cloned())
                    .collect(),
            ),
            None => (self.name.clone(), Vec::new()),
        }
    }
}

/// One finished browse-page fetch, handed back to the UI thread.
pub(crate) struct BrowseFetched {
    /// Whose run the fetch was for — a result for a panel since re-pointed at
    /// another label or artist is dropped.
    pub thread: BrowseThread,
    pub id: u64,
    /// Canonical name, when the fetch resolved it (a label page's first fetch
    /// does; page turns and artist pages don't need to).
    pub name: Option<String>,
    pub page: u32,
    pub result: std::result::Result<BrowsePage, String>,
    /// Read ahead rather than asked for: parked in the panel's cache without
    /// turning the page, unless the user has since turned to it.
    pub prefetch: bool,
    /// An artist run's kin, when this reply carries it instead of a page.
    pub kin: Option<discogs::ArtistDetail>,
}

/// What a row asked for, applied after the window releases its borrows.
/// The open run, snapshotted for one pass of drawing (see
/// [`App::browse_snapshot`]).
pub(crate) struct BrowseSnap {
    pub thread: BrowseThread,
    pub name: String,
    page: u32,
    pages: u32,
    loading: bool,
    error: Option<String>,
    rows: Vec<BrowseRow>,
    /// Index in the panel's releases of the first row shown, so a row's act
    /// names the release it was drawn from.
    offset: usize,
    /// More of an artist's run is still being read.
    reading: bool,
    by_alias: bool,
    /// The alias bar's names (group or not), and which one is open.
    family: Vec<(String, bool)>,
    at: Option<usize>,
}

/// What one pass of the rows asked for (see [`App::browse_apply`]).
#[derive(Default)]
pub(crate) struct BrowseOut {
    act: Option<Act>,
    want_covers: Vec<String>,
    hover_row: Option<usize>,
}

struct BrowseRow {
    cover: Option<Tex>,
    /// Where the sleeve comes from, for rows that scroll into view
    /// before theirs has been asked for.
    thumb: Option<String>,
    artist: String,
    title: String,
    /// `1994 · 12" · CR-03`, whichever parts exist. An artist's rows
    /// name the label too, since it varies down their run.
    sub: String,
    /// Credited as remixer rather than as the artist (artist runs only).
    remix: bool,
    /// Another of the artist's names this record is credited to, worn as a
    /// pill in place of the credit (artist runs only).
    alias: Option<String>,
    owned: bool,
    wanted: bool,
    /// A wantlist edit on this record is in flight or queued.
    pending: bool,
    /// The record the open sheet shows: the row you came from.
    shown: bool,
}

enum Act {
    /// Open this release's sheet — listen before judging the sleeve.
    Open(usize),
    /// Add it to the Discogs wantlist.
    Want(usize),
    /// Start a dig from it.
    Dig(usize),
    /// Fetch another page of the run.
    Page(u32),
    /// Order an artist's run by alias (true) or by date.
    Sort(bool),
    /// Open the run of the alias bar's name at this index.
    Name(usize),
}

/// Keep the rows a record digger wants: pressings (or format-unknown master
/// rows, which are usually records too), one per *work* — a label page lists
/// the original, the repress and every regional edition separately.
/// The page's records, one row per work. Discogs lists every pressing of a
/// record in the run (test pressing, standard, repress) and puts them in no
/// useful order; the row that survives the collapse is the pressing on one of
/// your shelves when there is one, else the first listed. That keeps the row's
/// id pointing at *your* copy, so the shelf marks and the sheet it opens
/// agree with your collection instead of with whichever pressing came first.
///
/// Which formats make the page is the user's "Release formats" setting, the
/// one the Discogs release picker filters by — one config, so a collector who
/// hides CDs there never scrolls past them here either. A row with no format
/// of its own (a master entry) is unknown, not hidden, and stays.
///
/// A run with nothing on the shown formats (a digital-only label, an artist
/// who only ever released files) is still a run: rather than an empty page,
/// every format is shown, each row's caption naming its format so nothing
/// reads as a pressing that isn't one.
fn crate_rows(
    page: &BrowsePage,
    owned: &HashSet<u64>,
    wanted: &HashSet<u64>,
    formats: &crate::config::Config,
) -> Vec<BrowseRelease> {
    let rows = collapse_rows(
        page.releases
            .iter()
            .filter(|r| !r.format_known || formats.shows_release_format(&r.format)),
        owned,
        wanted,
    );
    if rows.is_empty() && !page.releases.is_empty() {
        return collapse_rows(page.releases.iter(), owned, wanted);
    }
    rows
}

/// One row per work out of `releases`, the surviving pressing chosen by
/// shelf rank (owned, then wanted, then first listed).
fn collapse_rows<'a>(
    releases: impl Iterator<Item = &'a BrowseRelease>,
    owned: &HashSet<u64>,
    wanted: &HashSet<u64>,
) -> Vec<BrowseRelease> {
    let mut slot: HashMap<String, usize> = HashMap::new();
    let mut rows: Vec<BrowseRelease> = Vec::new();
    for r in releases {
        let (a, t) = crate::dig::row_artist_title(r);
        let key = crate::dig::work_key(&a, &t);
        let rank = |id: u64| -> u8 {
            if owned.contains(&id) {
                2
            } else if wanted.contains(&id) {
                1
            } else {
                0
            }
        };
        match slot.get(&key) {
            None => {
                slot.insert(key, rows.len());
                rows.push(r.clone());
            }
            Some(&i) if rank(r.release_id) > rank(rows[i].release_id) => {
                rows[i] = r.clone();
            }
            Some(_) => {}
        }
    }
    rows
}

/// Whose name a row of an artist's run is credited to.
enum Credit<'a> {
    /// The artist's own.
    Own,
    /// Another name of theirs (an alias, a group, a name variation): worn as
    /// a pill, and the run's second group when sorted by alias.
    Alias(&'a str),
    /// Someone else's: a record they remixed or appear on.
    Other,
}

/// Which of `own` and the `kin` names the row's `credit` is, read the way
/// Discogs writes them: a `*` marks a name variation, `(2)` tells two
/// artists of one name apart, and neither is part of the name.
fn credit_of<'a>(credit: &str, own: &str, kin: &'a [String]) -> Credit<'a> {
    let fold = |s: &str| crate::dig::strip_disambiguator(s.trim().trim_end_matches('*')).to_lowercase();
    let c = fold(credit);
    if c == fold(own) {
        return Credit::Own;
    }
    match kin.iter().find(|k| fold(k) == c) {
        Some(k) => Credit::Alias(crate::dig::strip_disambiguator(k)),
        None => Credit::Other,
    }
}

/// Order an artist's run. Discogs lists it by role, each group starting over
/// at the artist's first year; here it is one timeline (undated rows last,
/// Discogs's order breaking ties) or, `by_alias`, the artist's own records
/// first, then each other name they record under, A to Z, then the records
/// of others they remixed or appear on, every group by date.
fn sort_run(rows: &mut [BrowseRelease], by_alias: bool, own: &str, kin: &[String]) {
    rows.sort_by_key(|r| r.year.filter(|y| *y > 0).unwrap_or(u16::MAX));
    if by_alias {
        rows.sort_by_cached_key(|r| match credit_of(&crate::dig::row_artist_title(r).0, own, kin) {
            Credit::Own => (0, String::new()),
            Credit::Alias(a) => (1, a.to_lowercase()),
            Credit::Other => (2, String::new()),
        });
    }
}

/// The mark a run carries wherever it is named: a head for an artist's, a
/// house for a label's.
pub(crate) fn thread_icon(thread: BrowseThread) -> &'static str {
    use crate::ui::phosphor_icons::{app, named};
    match thread {
        BrowseThread::Label => named(app::HOUSE),
        BrowseThread::Artist => named(app::USER),
    }
}

impl App {
    /// Swap in the run asked for when it was open earlier this session: a
    /// run another one replaces is parked, not dropped, so artist → label
    /// → artist costs no second fetch. `id` 0 matches a label by name, as
    /// a label page is opened before its id is known.
    fn browse_restore(&mut self, thread: BrowseThread, id: u64, name: &str) -> bool {
        // A half-read artist run is dropped too: its pages would land on
        // nobody, and a fresh read is one request.
        if let Some(mut p) = self.browse_panel.take().filter(|p| !p.loading && !p.reading()) {
            // Its read-ahead reply, if one is in flight, is dropped by
            // `poll_browse_page`; the next page turn asks again.
            p.ahead = None;
            self.browse_kept.retain(|k| (k.thread, k.id) != (p.thread, p.id));
            self.browse_kept.push(p);
            if self.browse_kept.len() > 8 {
                self.browse_kept.remove(0);
            }
        }
        let hit = self
            .browse_kept
            .iter()
            .position(|k| k.thread == thread && if id == 0 { k.name == name } else { k.id == id });
        match hit {
            Some(i) => {
                self.browse_panel = Some(self.browse_kept.remove(i));
                // The order setting may have changed while it was parked.
                if thread == BrowseThread::Artist {
                    self.adopt_artist_run();
                }
                true
            }
            None => false,
        }
    }

    /// Open the label page for the label of `release_id`. The label id rides
    /// the release detail, which is cached for any record whose sheet was
    /// opened — so this usually resolves without a request and spends its one
    /// paced call on the first page of the run. `hint` names the label until
    /// the detail corrects it.
    pub(crate) fn open_label_page(&mut self, release_id: u64, hint: Option<String>) {
        let name = hint.unwrap_or_else(|| "…".to_string());
        if self.browse_restore(BrowseThread::Label, 0, &name) {
            return;
        }
        self.browse_panel = Some(BrowsePanel {
            thread: BrowseThread::Label,
            id: 0,
            name: name.clone(),
            page: 1,
            pages: 1,
            items: 0,
            releases: Vec::new(),
            loading: true,
            error: None,
            cache: HashMap::new(),
            ahead: None,
            kin: None,
            run_pages: 0,
            family: Vec::new(),
        });
        let token = self.discogs_token();
        let db = self.db_path.clone();
        let tx = self.browse_tx.clone();
        let ctx = self.egui_ctx.clone();
        thread::spawn(move || {
            let client = discogs::Client::new(token);
            let id = release_id.to_string();
            let detail = Catalog::open(&db).ok().and_then(|cat| {
                if let Ok(Some(d)) = cat.cached_release(&id) {
                    return Some(d);
                }
                cat.release_cached_or(&id, || client.fetch_release(&id))
                    .ok()
            });
            let (label_id, label_name) = match &detail {
                Some(d) => (d.label_ids.first().copied(), d.label.clone()),
                None => (None, None),
            };
            let Some(label_id) = label_id else {
                let _ = tx.send(BrowseFetched {
                    thread: BrowseThread::Label,
                    id: 0,
                    name: label_name,
                    page: 1,
                    result: Err("Discogs lists no label for this record.".to_string()),
                    prefetch: false,
                    kin: None,
                });
                ctx.request_repaint();
                return;
            };
            let result = client
                .browse_by_id(BrowseThread::Label, label_id, 1)
                .map_err(|e| e.to_string());
            let _ = tx.send(BrowseFetched {
                thread: BrowseThread::Label,
                id: label_id,
                name: label_name,
                page: 1,
                result,
                prefetch: false,
                kin: None,
            });
            ctx.request_repaint();
        });
    }

    /// Open the browse page on an artist's or label's run. The id is known up
    /// front (it comes from a credit or a Discogs name search hit), so this
    /// spends its one paced call straight on the first page of the run.
    pub(crate) fn open_run_page(&mut self, thread: BrowseThread, id: u64, name: String) {
        if self.browse_restore(thread, id, &name) {
            return;
        }
        self.browse_panel = Some(BrowsePanel {
            thread,
            id,
            name,
            page: 1,
            pages: 1,
            items: 0,
            releases: Vec::new(),
            loading: false,
            error: None,
            cache: HashMap::new(),
            ahead: None,
            kin: None,
            run_pages: 0,
            family: Vec::new(),
        });
        self.fetch_browse_page(1);
    }

    /// Show a page of the open run (the first, for an artist page; another,
    /// on a page turn) — from the panel's cache when it's been read already,
    /// else fetched.
    fn fetch_browse_page(&mut self, page: u32) {
        let Some(panel) = self.browse_panel.as_mut() else {
            return;
        };
        if panel.id == 0 || panel.loading {
            return;
        }
        if panel.thread == BrowseThread::Artist {
            // The run is read whole, once; a page turn is a slice of it.
            if panel.cache.is_empty() {
                panel.loading = true;
                panel.error = None;
                let id = panel.id;
                self.spawn_artist_run(id);
            } else {
                panel.page = page;
                self.adopt_artist_run();
            }
            return;
        }
        if let Some(cached) = panel.cache.get(&page).cloned() {
            panel.page = page;
            panel.error = None;
            self.adopt_browse_page(page, &cached);
            self.browse_lookahead();
            return;
        }
        let (thread, id) = (panel.thread, panel.id);
        panel.loading = true;
        panel.page = page;
        panel.error = None;
        // Already being read ahead: wait for that rather than asking twice.
        // `poll_browse_page` turns to it on arrival because the panel now
        // says it's loading this page.
        if panel.ahead == Some(page) {
            return;
        }
        self.spawn_browse_fetch(thread, id, page, false);
    }

    /// Read the page after the one on screen into the cache, so the next
    /// turn forward is instant. Paced as background, so it never delays a
    /// click; at most one page is ever in flight ahead.
    fn browse_lookahead(&mut self) {
        let Some(panel) = self.browse_panel.as_ref() else {
            return;
        };
        // An artist's run reads itself whole (see `spawn_artist_run`).
        if panel.id == 0 || panel.loading || panel.ahead.is_some() || panel.thread == BrowseThread::Artist {
            return;
        }
        let next = panel.page + 1;
        if next > panel.pages || panel.cache.contains_key(&next) {
            return;
        }
        let (thread, id) = (panel.thread, panel.id);
        if let Some(panel) = self.browse_panel.as_mut() {
            panel.ahead = Some(next);
        }
        self.spawn_browse_fetch(thread, id, next, true);
    }

    fn spawn_browse_fetch(&mut self, thread: BrowseThread, id: u64, page: u32, prefetch: bool) {
        let token = self.discogs_token();
        let tx = self.browse_tx.clone();
        let ctx = self.egui_ctx.clone();
        thread::spawn(move || {
            let mut client = discogs::Client::new(token);
            if prefetch {
                client = client.background();
            }
            let result = client
                .browse_by_id(thread, id, page)
                .map_err(|e| e.to_string());
            let _ = tx.send(BrowseFetched {
                thread,
                id,
                name: None,
                page,
                result,
                prefetch,
                kin: None,
            });
            ctx.request_repaint();
        });
    }

    /// Read an artist's whole run: the first page now, then their kin (the
    /// alias pills and grouping) and the remaining pages behind it, each
    /// handed back as it lands so the rows fill in. One thread, so the
    /// pages take the pace in order and a click elsewhere goes first.
    fn spawn_artist_run(&mut self, id: u64) {
        let token = self.discogs_token();
        let tx = self.browse_tx.clone();
        let ctx = self.egui_ctx.clone();
        thread::spawn(move || {
            let reply = |page: u32,
                         result: std::result::Result<BrowsePage, String>,
                         kin: Option<discogs::ArtistDetail>| {
                let _ = tx.send(BrowseFetched {
                    thread: BrowseThread::Artist,
                    id,
                    name: None,
                    page,
                    result,
                    prefetch: page != 1,
                    kin,
                });
                ctx.request_repaint();
            };
            let client = discogs::Client::new(token);
            let first = client.browse_run(BrowseThread::Artist, id, 1);
            let pages = first.as_ref().map_or(0, |p| p.pages);
            reply(1, first.map_err(|e| e.to_string()), None);
            if pages == 0 {
                return;
            }
            if let Ok(kin) = client.fetch_artist(id) {
                reply(0, Ok(BrowsePage::default()), Some(kin));
            }
            let client = client.background();
            for page in 2..=pages {
                reply(
                    page,
                    client.browse_run(BrowseThread::Artist, id, page).map_err(|e| e.to_string()),
                    None,
                );
            }
        });
    }

    /// Rebuild an artist run's rows from every page read so far: one list,
    /// one row per record across pages, in the order the setting asks for,
    /// shown [`RUN_PAGE`] rows at a time.
    fn adopt_artist_run(&mut self) {
        let Some(panel) = self.browse_panel.as_ref() else {
            return;
        };
        let mut pages: Vec<(&u32, &BrowsePage)> = panel.cache.iter().collect();
        pages.sort_by_key(|(n, _)| **n);
        let all = BrowsePage {
            pages: 1,
            items: panel.items,
            releases: pages.into_iter().flat_map(|(_, p)| p.releases.iter().cloned()).collect(),
        };
        let mut rows = crate_rows(&all, &self.vinyl_owned, &self.vinyl_wanted, &self.config);
        let (own, kin) = panel.names();
        sort_run(&mut rows, self.config.artist_run_by_alias, &own, &kin);
        let Some(panel) = self.browse_panel.as_mut() else {
            return;
        };
        panel.pages = (rows.len().max(1) as u32).div_ceil(RUN_PAGE as u32);
        panel.page = panel.page.clamp(1, panel.pages);
        panel.releases = rows;
    }

    /// One reply of an artist run's read (see [`App::spawn_artist_run`]):
    /// the kin, or a page of the run parked in the cache, and the rows
    /// rebuilt from every page read so far.
    fn adopt_artist_reply(&mut self, msg: BrowseFetched) {
        let Some(panel) = self.browse_panel.as_mut() else {
            return;
        };
        if let Some(kin) = msg.kin {
            if panel.family.is_empty() {
                let me = discogs::NamedRef { id: kin.id, name: kin.name.clone() };
                panel.family = std::iter::once((me, false))
                    .chain(kin.aliases.iter().map(|a| (a.clone(), false)))
                    .chain(kin.groups.iter().map(|g| (g.clone(), true)))
                    .collect();
            }
            panel.kin = Some(kin);
            self.adopt_artist_run();
            return;
        }
        match msg.result {
            Ok(page) => {
                if msg.page == 1 {
                    panel.run_pages = page.pages.max(1);
                    panel.items = page.items;
                    panel.loading = false;
                }
                panel.cache.insert(msg.page, page);
                self.adopt_artist_run();
            }
            // The first page failing is the run failing; a later one leaves
            // the rows read so far, and says so.
            Err(e) if msg.page == 1 => {
                panel.loading = false;
                panel.error = Some(e);
            }
            Err(e) => self.status = format!("Couldn't read all of the run: {e}"),
        }
    }

    /// Turn the panel to `page`, building its rows.
    fn adopt_browse_page(&mut self, page_no: u32, page: &BrowsePage) {
        let rows = crate_rows(page, &self.vinyl_owned, &self.vinyl_wanted, &self.config);
        let Some(panel) = self.browse_panel.as_mut() else {
            return;
        };
        panel.page = page_no;
        panel.pages = page.pages.max(1);
        panel.items = page.items;
        panel.releases = rows;
    }

    /// Rebuild the open page's rows from the panel's cache — for when the
    /// "Release formats" setting changes under an open run, so the page
    /// follows the setting without a refetch.
    pub(crate) fn refilter_browse_page(&mut self) {
        let Some(panel) = self.browse_panel.as_ref() else {
            return;
        };
        if panel.thread == BrowseThread::Artist {
            self.adopt_artist_run();
            return;
        }
        let page_no = panel.page;
        if let Some(cached) = panel.cache.get(&page_no).cloned() {
            self.adopt_browse_page(page_no, &cached);
        }
    }

    /// Adopt finished browse-page fetches onto the open panel.
    pub(crate) fn poll_browse_page(&mut self) {
        while let Ok(msg) = self.browse_rx.try_recv() {
            let Some(panel) = self.browse_panel.as_mut() else {
                continue;
            };
            // A label page's first fetch is the only one that arrives while
            // the panel still has no id; everything else must match the run
            // on screen.
            if msg.thread != panel.thread || (panel.id != 0 && msg.id != panel.id) {
                continue;
            }
            if panel.thread == BrowseThread::Artist {
                self.adopt_artist_reply(msg);
                continue;
            }
            if msg.prefetch && panel.ahead == Some(msg.page) {
                panel.ahead = None;
            }
            // A read-ahead page the user hasn't turned to yet is parked; one
            // they turned to while it was in flight is shown like any fetch.
            let wanted = !msg.prefetch || (panel.loading && panel.page == msg.page);
            if msg.id != 0 {
                panel.id = msg.id;
            }
            if let Some(name) = msg.name.filter(|n| !n.trim().is_empty()) {
                panel.name = name;
            }
            match msg.result {
                Ok(page) => {
                    panel.cache.insert(msg.page, page.clone());
                    if wanted {
                        panel.loading = false;
                        self.adopt_browse_page(msg.page, &page);
                    }
                }
                Err(e) => {
                    if wanted {
                        panel.loading = false;
                        panel.error = Some(e);
                    }
                }
            }
            self.browse_lookahead();
        }
    }

    /// Draw the open browse page, if any.
    pub(crate) fn draw_browse_page(&mut self, ctx: &egui::Context) {
        // While a record sheet is open the run is one of its panes (see
        // `draw_vinyl_sheet`), never a window over it.
        if self.vinyl_sheet.as_ref().is_some_and(|s| !s.hidden) {
            return;
        }
        let Some(snap) = self.browse_snapshot() else {
            return;
        };
        let mut out = BrowseOut::default();
        let mut open = true;
        let title = crate::ui::icon_text(thread_icon(snap.thread), &snap.name, crate::ui::tokens::font::title());
        crate::ui::window::Window::new(title)
            .id(egui::Id::new("browse-page"))
            .open(&mut open)
            .resizable_height()
            .default_width(PANEL_W)
            .show(ctx, |ui| {
                ui.set_min_width(PANEL_W);
                ui.set_max_width(PANEL_W);
                // The alias bar: every other name the artist records under,
                // one press from its run, the way the record sheet offers
                // its artists and label.
                if snap.family.len() > 1 {
                    use crate::ui::phosphor_icons::{app, named};
                    let segments: Vec<crate::ui::button::Segment> = snap
                        .family
                        .iter()
                        .map(|(n, group)| crate::ui::button::Segment {
                            icon: named(if *group { app::USERS } else { app::USER }),
                            label: n,
                            tip: "",
                        })
                        .collect();
                    ui.add_space(crate::ui::tokens::space::S2);
                    if let Some(i) = crate::ui::button::segmented_scroll(ui, "browse-family", snap.at, &segments) {
                        out.act = Some(Act::Name(i));
                    }
                    ui.add_space(crate::ui::tokens::space::S2);
                    ui.separator();
                }
                Self::browse_rows_ui(ui, &snap, &mut out, false);
            });
        if !open {
            self.browse_panel = None;
            return;
        }
        self.browse_apply(ctx, snap.thread, out);
    }

    /// The open run as the rows draw it: whose, which page, and each row
    /// with its shelf marks and whatever sleeve is decoded already.
    pub(crate) fn browse_snapshot(&self) -> Option<BrowseSnap> {
        let panel = self.browse_panel.as_ref()?;
        let (name, page, pages, loading, error) = (
            panel.name.clone(),
            panel.page,
            panel.pages,
            panel.loading,
            panel.error.clone(),
        );
        let thread = panel.thread;
        let (own, kin) = panel.names();
        // An artist's run is one sorted list shown a page at a time; a
        // label's rows are the page Discogs served.
        let offset = match thread {
            BrowseThread::Artist => (panel.page.max(1) as usize - 1) * RUN_PAGE,
            BrowseThread::Label => 0,
        };
        let end = (offset + RUN_PAGE).min(panel.releases.len());
        let shown = match thread {
            BrowseThread::Artist => &panel.releases[offset.min(end)..end],
            BrowseThread::Label => &panel.releases[..],
        };
        // Snapshot the rows, then fill sleeves through the shared URL cache
        // (`dig_cover` needs `&mut self`, so two passes like the versions
        // panel).
        let specs: Vec<(String, u64, String, String, String, bool, Option<String>)> = shown
            .iter()
            .map(|r| {
                let (artist, title) = crate::dig::row_artist_title(r);
                // A label panel has no kin, so only an artist's rows can
                // come out an alias.
                let alias = match credit_of(&artist, &own, &kin) {
                    Credit::Alias(a) => Some(a.to_string()),
                    _ => None,
                };
                let imprint = match thread {
                    // A label page is the imprint; naming it on every row
                    // says nothing. An artist's run hops labels, so there it
                    // is the row's most telling fact after the year.
                    BrowseThread::Label => r.catno.trim().to_string(),
                    BrowseThread::Artist => {
                        crate::util::imprint(non_empty(&r.label).as_deref(), non_empty(&r.catno).as_deref())
                            .unwrap_or_default()
                    }
                };
                let sub = dots(&[
                    r.year.filter(|y| *y > 0).map(|y| y.to_string()).as_deref(),
                    (!r.format.trim().is_empty()).then_some(r.format.as_str()),
                    (!imprint.is_empty()).then_some(imprint.as_str()),
                ]);
                (
                    r.thumb_url.clone(),
                    r.release_id,
                    crate::dig::strip_disambiguator(&artist).to_string(),
                    title,
                    sub,
                    thread == BrowseThread::Artist && !r.main,
                    alias,
                )
            })
            .collect();
        let shown_release = self.vinyl_sheet.as_ref().map(|s| s.release_id);
        let mut rows: Vec<BrowseRow> = Vec::with_capacity(specs.len());
        for (thumb, release_id, artist, title, sub, remix, alias) in specs {
            // By record, not pressing: the run may list another pressing of a
            // record you own.
            let owned = self.owns_record(release_id, &artist, &title);
            let wanted = self.wants_record(release_id, &artist, &title);
            let pending = self.vinyl_pending(VinylList::Wantlist, release_id).is_some();
            // Only what's already decoded is read here; the rows on screen
            // ask for theirs as they draw (`want_covers`), so a hundred-row
            // page doesn't start a hundred downloads for the eight it shows.
            let thumb = (!thumb.trim().is_empty()).then_some(thumb);
            let cover = thumb.as_ref().and_then(|u| match self.dig_covers.get(u) {
                Some(ThumbState::Ready(t)) => t.clone(),
                _ => None,
            });
            rows.push(BrowseRow {
                cover,
                thumb,
                artist,
                title,
                sub,
                remix,
                alias,
                owned,
                wanted,
                pending,
                shown: shown_release == Some(release_id),
            });
        }

        Some(BrowseSnap {
            thread,
            name,
            page,
            pages,
            loading,
            error,
            rows,
            offset,
            reading: panel.reading(),
            by_alias: self.config.artist_run_by_alias,
            family: panel
                .family
                .iter()
                .map(|(r, g)| (crate::dig::strip_disambiguator(&r.name).to_string(), *g))
                .collect(),
            at: panel.family.iter().position(|(r, _)| r.id == panel.id),
        })
    }

    /// The run's rows and pager, drawn in whatever frame holds them: the
    /// browse window, or the record sheet's run pane.
    /// `fill`: the list takes the height it is given, the pager on its
    /// bottom edge (the record sheet's pane, in a held frame); else the list
    /// is as tall as its rows, to a cap, and the pager follows it.
    pub(crate) fn browse_rows_ui(ui: &mut egui::Ui, snap: &BrowseSnap, out: &mut BrowseOut, fill: bool) {
        let (thread, page, pages, loading) = (snap.thread, snap.page, snap.pages, snap.loading);
        let (error, rows) = (&snap.error, &snap.rows);
        let base = snap.offset;
        ui.add_space(6.0);
        if let Some(e) = &error {
            ui.label(egui::RichText::new(e).weak());
            ui.add_space(4.0);
            return;
        }
        if loading && rows.is_empty() {
            ui.horizontal(|ui| {
                ui.spinner();
                let reading = match thread {
                    BrowseThread::Label => "Reading the label's crate…",
                    BrowseThread::Artist => "Reading the artist's crate…",
                };
                ui.label(egui::RichText::new(reading).weak());
            });
            ui.add_space(4.0);
            return;
        }
        if rows.is_empty() {
            ui.label(egui::RichText::new("No records on this page of the run").weak());
        }
        let list = |ui: &mut egui::Ui, out: &mut BrowseOut| {
        egui::ScrollArea::vertical()
            .id_salt((thread as u8, &snap.name))
            .max_height(if fill { f32::INFINITY } else { 440.0 })
            .show(ui, |ui| {
                // A paged run keeps the list at full height on every
                // page, so the pager under it stays put under the
                // cursor when a short last page turns up.
                if pages > 1 && !fill {
                    ui.set_min_height(440.0);
                }
                for (i, r) in rows.iter().enumerate() {
                    // Acts name the release in the panel, not the row on
                    // the page.
                    let i = base + i;
                    let bg = ui.painter().add(egui::Shape::Noop);
                    let row = ui.horizontal(|ui| {
                        let (trect, tresp) = ui.allocate_exact_size(
                            egui::vec2(THUMB, THUMB),
                            egui::Sense::click(),
                        );
                        if r.cover.is_none() && ui.is_rect_visible(trect) {
                            if let Some(u) = &r.thumb {
                                out.want_covers.push(u.clone());
                            }
                        }
                        crate::ui::cover::paint_at(ui, trect, r.cover.as_ref(), 4.0, None);
                        let tresp = tresp
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .on_hover_note("Open the record");
                        if tresp.hovered() {
                            out.hover_row = Some(i);
                        }
                        if tresp.clicked() {
                            out.act = Some(Act::Open(i));
                        }
                        ui.add_space(8.0);
                        // The buttons claim their width first, from
                        // the right, so the text column gets what is
                        // left and cuts its title there: a long title
                        // would otherwise run under the buttons and
                        // widen the window, for good (egui keeps the
                        // widest width a window's content ever took).
                        ui.with_layout(
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                // Same square as the heart beside it,
                                // so the pair reads as one control row.
                                if crate::ui::icon::glyph_button(ui, "🔍", true)
                                    .on_hover_note("Dig from this record")
                                    .clicked()
                                {
                                    out.act = Some(Act::Dig(i));
                                }
                                let parked = r.owned || r.wanted;
                                if crate::ui::icon::shelf_button(
                                    ui,
                                    VinylList::Wantlist,
                                    false,
                                    "",
                                    !parked && !r.pending,
                                )
                                .on_hover_note("Add to your Discogs wantlist")
                                .clicked()
                                {
                                    out.act = Some(Act::Want(i));
                                }
                                ui.with_layout(
                                    egui::Layout::top_down(egui::Align::Min),
                                    |ui| {
                                        ui.spacing_mut().item_spacing.y = 1.0;
                                        ui.horizontal(|ui| {
                                            ui.spacing_mut().item_spacing.x = 6.0;
                                            // What follows the title on its
                                            // line, reserved before the title
                                            // is cut: a shelf mark and the
                                            // remix tag.
                                            let marks = [
                                                (r.owned, VinylList::Collection),
                                                (!r.owned && r.wanted, VinylList::Wantlist),
                                            ];
                                            let remix = r.remix.then(|| {
                                                egui::WidgetText::from(
                                                    egui::RichText::new("REMIX")
                                                        .small()
                                                        .color(egui::Color32::from_gray(140)),
                                                )
                                                .into_galley(
                                                    ui,
                                                    Some(egui::TextWrapMode::Extend),
                                                    f32::INFINITY,
                                                    egui::TextStyle::Small,
                                                )
                                            });
                                            let reserve = marks
                                                .iter()
                                                .filter(|(show, _)| *show)
                                                .count() as f32
                                                * (14.0 + 6.0)
                                                + remix.as_ref().map_or(0.0, |g| g.size().x + 6.0);
                                            // A record under another of the
                                            // artist's names wears that name
                                            // as a pill where the credit
                                            // would read.
                                            let line = match &r.alias {
                                                Some(a) => {
                                                    crate::ui::chip::tag(ui, a);
                                                    r.title.clone()
                                                }
                                                None => format!("{} — {}", r.artist, r.title),
                                            };
                                            let t = ui
                                                .scope(|ui| {
                                                    ui.set_max_width(
                                                        (ui.available_width() - reserve).max(0.0),
                                                    );
                                                    ui.add(
                                                        egui::Label::new(
                                                            egui::RichText::new(line).strong(),
                                                        )
                                                        .truncate()
                                                        .sense(egui::Sense::click()),
                                                    )
                                                })
                                                .inner
                                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                                .on_hover_note("Open the record");
                                            if t.clicked() {
                                                out.act = Some(Act::Open(i));
                                            }
                                            // Your shelves, marked: the mark
                                            // itself, solid, in the row's quiet ink.
                                            for (show, list) in marks {
                                                if show {
                                                    let (rect, _) = ui.allocate_exact_size(
                                                        egui::vec2(14.0, 14.0),
                                                        egui::Sense::hover(),
                                                    );
                                                    crate::ui::icon::shelf(
                                                        ui.painter(),
                                                        rect.center(),
                                                        5.5,
                                                        egui::Color32::from_gray(150),
                                                        true,
                                                        list,
                                                    );
                                                }
                                            }
                                            if let Some(g) = remix {
                                                ui.add(egui::Label::new(g));
                                            }
                                        });
                                        if !r.sub.is_empty() {
                                            // Cut, never widen: a long catalogue
                                            // line would push the frame out.
                                            ui.add(
                                                egui::Label::new(
                                                    egui::RichText::new(&r.sub).small().weak(),
                                                )
                                                .truncate(),
                                            );
                                        }
                                    },
                                );
                            },
                        );
                    });
                    if r.shown {
                        ui.painter().set(
                            bg,
                            egui::Shape::rect_filled(
                                row.response.rect.expand(crate::ui::tokens::space::S2),
                                crate::ui::tokens::radius::SM,
                                egui::Color32::from_white_alpha(10),
                            ),
                        );
                    }
                    ui.add_space(crate::ui::tokens::space::S2);
                    ui.separator();
                    ui.add_space(crate::ui::tokens::space::S2);
                }
            });
        };
        // Pager, when the run outgrows one page; an artist's run always has
        // its order to choose, so its row is always there.
        let pager = |ui: &mut egui::Ui, out: &mut BrowseOut| {
            if pages <= 1 && thread != BrowseThread::Artist {
                return;
            }
            // Pinned, the pager sits on the edge like the tracks footer does.
            if !fill {
                ui.add_space(4.0);
            }
            // At its buttons' height, so it never reaches past a held frame.
            crate::ui::control_row(ui, |ui| ui.horizontal(|ui| {
                if thread == BrowseThread::Artist {
                    let order = [
                        crate::ui::button::Segment { icon: "", label: "By date", tip: "One timeline, oldest first" },
                        crate::ui::button::Segment {
                            icon: "",
                            label: "By alias",
                            tip: "Their own records first, then each other name they record under",
                        },
                    ];
                    if let Some(i) = crate::ui::button::segmented(ui, Some(snap.by_alias as usize), &order) {
                        out.act = Some(Act::Sort(i == 1));
                    }
                }
                if pages > 1 {
                    if ui
                        .add_enabled(page > 1 && !loading, egui::Button::new("←"))
                        .on_hover_note("Earlier in the run")
                        .clicked()
                    {
                        out.act = Some(Act::Page(page - 1));
                    }
                    if ui
                        .add_enabled(page < pages && !loading, egui::Button::new("→"))
                        .on_hover_note("Later in the run")
                        .clicked()
                    {
                        out.act = Some(Act::Page(page + 1));
                    }
                    ui.label(egui::RichText::new(format!("Page {page} of {pages}")).weak());
                }
                // After the label, so the page number holds still
                // while a page loads.
                if loading {
                    ui.spinner();
                }
                if snap.reading {
                    ui.spinner();
                    ui.label(egui::RichText::new("Reading the rest of the run…").weak());
                }
            }));
            // The footer's hairline, over the pager: drawn after the row,
            // which in the pinned (bottom-up) layout puts it above.
            if fill {
                ui.separator();
            }
        };
        if fill {
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                pager(ui, out);
                ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| list(ui, out));
            });
        } else {
            list(ui, out);
            pager(ui, out);
        }
    }

    /// What the rows asked for: sleeves to fetch, a record to open, want
    /// or dig, a page turn, and the hovered row's detail warmed.
    pub(crate) fn browse_apply(&mut self, ctx: &egui::Context, thread: BrowseThread, out: BrowseOut) {
        // Sleeves for the rows that were on screen, requested once the frame
        // has released its borrows. CDN downloads, outside the request pace.
        for u in out.want_covers {
            self.dig_cover(&u);
        }
        let row_of = |i: usize| -> Option<&BrowseRelease> {
            self.browse_panel.as_ref().and_then(|p| p.releases.get(i))
        };
        match out.act {
            Some(Act::Open(i)) => {
                if let Some(r) = row_of(i).cloned() {
                    let (artist, title) = crate::dig::row_artist_title(&r);
                    let sub = dots(&[
                        r.year.filter(|y| *y > 0).map(|y| y.to_string()).as_deref(),
                        (!r.format.trim().is_empty()).then_some(r.format.as_str()),
                    ]);
                    let thumb = (!r.thumb_url.trim().is_empty()).then(|| r.thumb_url.clone());
                    self.open_release_sheet(r.release_id, artist, title, sub, thumb, ctx);
                }
            }
            Some(Act::Want(i)) => {
                if let Some(r) = row_of(i).cloned() {
                    let (artist, title) = crate::dig::row_artist_title(&r);
                    let edit = VinylEdit::Want {
                        release_ids: vec![r.release_id],
                        label: format!("{artist} — {title}"),
                    };
                    self.request_vinyl_edit(ctx.clone(), edit);
                }
            }
            Some(Act::Dig(i)) => {
                if let Some(r) = row_of(i).cloned() {
                    let (artist, title) = crate::dig::row_artist_title(&r);
                    let sub = dots(&[
                        r.year.filter(|y| *y > 0).map(|y| y.to_string()).as_deref(),
                        (!r.format.trim().is_empty()).then_some(r.format.as_str()),
                    ]);
                    // A label page knows the imprint; an artist's row carries
                    // its own, when Discogs listed one.
                    let label_name = match thread {
                        BrowseThread::Label => self
                            .browse_panel
                            .as_ref()
                            .map(|p| p.name.clone())
                            .filter(|n| n != "…"),
                        BrowseThread::Artist => {
                            Some(r.label.trim().to_string()).filter(|l| !l.is_empty())
                        }
                    };
                    let thumb = (!r.thumb_url.trim().is_empty()).then(|| r.thumb_url.clone());
                    self.start_dig_release(r.release_id, artist, title, label_name, sub, thumb);
                    self.show_dig_window();
                }
            }
            Some(Act::Page(p)) => self.fetch_browse_page(p),
            Some(Act::Name(i)) => {
                let family = self.browse_panel.as_ref().map(|p| p.family.clone()).unwrap_or_default();
                if let Some((r, _)) = family.get(i).cloned() {
                    self.open_run_page(BrowseThread::Artist, r.id, crate::dig::strip_disambiguator(&r.name).to_string());
                    if let Some(p) = self.browse_panel.as_mut() {
                        p.family = family;
                    }
                }
            }
            Some(Act::Sort(by_alias)) => {
                self.config.artist_run_by_alias = by_alias;
                self.save_config();
                self.adopt_artist_run();
            }
            None => {}
        }
        // Resting on a row: have its tracklist ready before the click.
        let warm = out.hover_row.and_then(|i| {
            self.browse_panel
                .as_ref()
                .and_then(|p| p.releases.get(i))
                .map(|r| r.release_id)
        });
        if let Some(id) = warm {
            self.warm_release_detail(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(id: u64, artist: &str, title: &str, format: &str) -> BrowseRelease {
        BrowseRelease {
            release_id: id,
            title: title.into(),
            artist: artist.into(),
            year: Some(2019),
            format: format.into(),
            format_known: true,
            label: "Nightime Drama".into(),
            catno: "NTD010".into(),
            thumb_url: String::new(),
            main: true,
            genres: Vec::new(),
            styles: Vec::new(),
        }
    }

    /// The screenshot case: Discogs lists the test pressing of Night Drive EP
    /// ahead of the standard 12" the collection holds. The collapsed row has
    /// to be the owned pressing, not whichever came first.
    #[test]
    fn crate_rows_keep_the_shelved_pressing_of_a_record() {
        let page = BrowsePage {
            pages: 1,
            items: 3,
            releases: vec![
                release(1, "Various", "Night Drive EP", "12\", EP, TP"),
                release(2, "Various", "Night Drive EP", "12\", EP"),
                release(3, "Trinity", "Cascade Drive", "12\""),
            ],
        };
        let cfg = crate::config::Config::default();
        let owned: HashSet<u64> = [2].into_iter().collect();
        let rows = crate_rows(&page, &owned, &HashSet::new(), &cfg);
        let ids: Vec<u64> = rows.iter().map(|r| r.release_id).collect();
        assert_eq!(ids, vec![2, 3], "owned pressing wins, order of first sight kept");

        // A wanted pressing wins over an unshelved one, but not over an owned one.
        let wanted: HashSet<u64> = [1].into_iter().collect();
        let rows = crate_rows(&page, &HashSet::new(), &wanted, &cfg);
        assert_eq!(rows[0].release_id, 1);
        let rows = crate_rows(&page, &owned, &wanted, &cfg);
        assert_eq!(rows[0].release_id, 2);

        // Nothing shelved: first listed stands, as before.
        let rows = crate_rows(&page, &HashSet::new(), &HashSet::new(), &cfg);
        assert_eq!(rows[0].release_id, 1);
    }

    /// A records-only collector's config: everything but vinyl hidden.
    fn vinyl_only() -> crate::config::Config {
        use crate::config::ReleaseMedium;
        let mut cfg = crate::config::Config::default();
        for m in [ReleaseMedium::Cd, ReleaseMedium::Digital, ReleaseMedium::Cassette] {
            cfg.set_release_medium_shown(m, false);
        }
        cfg
    }

    /// The page follows the release picker's "Release formats" setting: with
    /// nothing hidden every row shows; hide CDs and digital and they drop out
    /// of the run; a master row with no format of its own stays either way.
    #[test]
    fn crate_rows_follow_the_release_formats_setting() {
        let mut master = release(4, "Trinity", "Cascade Drive", "");
        master.format_known = false;
        let page = BrowsePage {
            pages: 1,
            items: 4,
            releases: vec![
                release(1, "Trinity", "Night Drive EP", "12\", EP"),
                release(2, "Trinity", "Night Drive EP", "CD, EP"),
                release(3, "Trinity", "Faceshift", "2xFile, MP3, EP"),
                master,
            ],
        };
        let none = HashSet::new();
        let ids = |cfg: &crate::config::Config| -> Vec<u64> {
            crate_rows(&page, &none, &none, cfg)
                .iter()
                .map(|r| r.release_id)
                .collect()
        };
        assert_eq!(
            ids(&crate::config::Config::default()),
            vec![1, 3, 4],
            "nothing hidden: every format shows, one row per work"
        );
        assert_eq!(ids(&vinyl_only()), vec![1, 4], "CDs and files hidden, master kept");
        let mut cd_only = crate::config::Config::default();
        cd_only.set_release_medium_shown(crate::config::ReleaseMedium::Vinyl, false);
        cd_only.set_release_medium_shown(crate::config::ReleaseMedium::Digital, false);
        assert_eq!(ids(&cd_only), vec![2, 4], "the CD pressing stands in for the work");
    }

    /// The Duty Cycle case: a label whose whole run is digital, read by a
    /// records-only collector. The filter would leave the page blank, so every
    /// format is shown instead; a page with any record on it still shows only
    /// the records.
    /// Discogs lists a run by role, each group starting over at the first
    /// year; by date the groups interleave into one timeline, by alias the
    /// artist's own records lead, then each other name A to Z, then others'.
    #[test]
    fn sort_run_orders_the_whole_run() {
        let row = |id, artist: &str, title: &str, year| BrowseRelease {
            year: Some(year),
            ..release(id, artist, title, "12\"")
        };
        let ids = |rows: &[BrowseRelease]| rows.iter().map(|r| r.release_id).collect::<Vec<_>>();
        let kin = vec!["AFX".to_string(), "Polygon Window".to_string()];
        let mut rows = vec![
            row(1, "Aphex Twin", "On", 1993),
            row(2, "AFX*", "Analord 01", 2005),
            row(3, "Aphex Twin", "Syro", 2014),
            row(4, "Mescalinum United", "We Have Arrived", 1992),
            row(5, "Polygon Window", "Surfing On Sine Waves", 1993),
            row(6, "Aphex Twin", "Undated", 0),
        ];
        sort_run(&mut rows, false, "Aphex Twin", &kin);
        assert_eq!(ids(&rows), [4, 1, 5, 2, 3, 6]);
        sort_run(&mut rows, true, "Aphex Twin", &kin);
        assert_eq!(ids(&rows), [1, 3, 6, 2, 5, 4]);
    }

    /// A name variation's `*` and a disambiguating `(2)` are Discogs's
    /// marks, not the name; a stranger's name variation is still a stranger.
    #[test]
    fn credit_of_reads_names_the_way_discogs_writes_them() {
        let kin = vec!["Point Blank (2)".to_string(), "AFX".to_string()];
        assert!(matches!(credit_of("Swag", "Swag", &kin), Credit::Own));
        assert!(matches!(credit_of("afx*", "Aphex Twin", &kin), Credit::Alias(a) if a == "AFX"));
        assert!(matches!(credit_of("Point Blank", "Swag", &kin), Credit::Alias(a) if a == "Point Blank"));
        assert!(matches!(credit_of("The Bobby Hughes Experience*", "Swag", &kin), Credit::Other));
    }

    #[test]
    fn crate_rows_show_every_format_when_a_run_has_no_records() {
        let cfg = vinyl_only();
        let digital = BrowsePage {
            pages: 1,
            items: 3,
            releases: vec![
                release(1, "Polyplay", "Resolve", "2xFile, MP3, EP, 320 kbps"),
                release(2, "Polyplay", "Faceshift", "4xFile, WAV, EP"),
                release(3, "Polyplay", "Resolve", "2xFile, MP3, EP, 320 kbps"),
            ],
        };
        let rows = crate_rows(&digital, &HashSet::new(), &HashSet::new(), &cfg);
        let ids: Vec<u64> = rows.iter().map(|r| r.release_id).collect();
        assert_eq!(ids, vec![1, 2], "digital rows shown, still one per work");

        let mixed = BrowsePage {
            pages: 1,
            items: 2,
            releases: vec![
                release(1, "Polyplay", "Resolve", "2xFile, MP3, EP"),
                release(2, "Polyplay", "Faceshift", "12\", EP"),
            ],
        };
        let rows = crate_rows(&mixed, &HashSet::new(), &HashSet::new(), &cfg);
        let ids: Vec<u64> = rows.iter().map(|r| r.release_id).collect();
        assert_eq!(ids, vec![2], "a run with a record on it keeps the format filter");

        let empty = BrowsePage { pages: 1, items: 0, releases: Vec::new() };
        assert!(crate_rows(&empty, &HashSet::new(), &HashSet::new(), &cfg).is_empty());
    }
}
