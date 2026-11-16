//! The Sellers tab of the vinyl view: dig through a saved Discogs seller's
//! crates the way you'd flip through the bins in their shop.
//!
//! Sellers are saved by username (Discogs removed the release→sellers
//! direction, so the shop is the only way in — see
//! `docs/design/bulk-sellers-spike.md`). An explicit Update pages the shop's
//! for-sale inventory into the `seller_listings` cache; from then on the
//! crates browse offline — filtered, price/condition on every card, records
//! you already own or want badged from the same membership sets the shelves
//! use. A card opens the ordinary record sheet, so listening (YouTube via the
//! mini-player), wantlisting and digging all come along for free; buying
//! happens on discogs.com, one click from the card or the sheet.

use super::*;
use crate::ui::phosphor_icons::{app as icons, named};
use crate::vinyl_sheet::SellerOffer;
use ordnung_core::model::{seller_baskets, SellerBasket};

/// The shop's cart: a button counting what the shop stocks from the
/// wantlist, with the records in a dropdown under it, cheapest copy of each.
/// A row opens the record carrying that offer, as the watch's rows do; the
/// last row opens the shop's "in my wantlist" page on discogs.com, where the
/// real cart lives (the API has no cart endpoint).
fn draw_cart_button(
    ui: &mut egui::Ui,
    seller: &str,
    cart: Option<&SellerBasket>,
    max: usize,
    open: &mut Option<u64>,
) {
    let n = cart.map_or(0, |b| b.wants());
    let label = match n {
        0 => "Cart".to_string(),
        n => format!("Cart · {n}"),
    };
    let cart_text = |s: &str| {
        crate::ui::icon_text(named(icons::SHOPPING_CART), s, crate::ui::tokens::font::body())
    };
    // As wide as the whole wantlist's count needs, so the buttons left of it
    // hold still as the shop's stock changes.
    let widest = cart_text(&format!("Cart · {max}")).into_galley(
        ui,
        Some(egui::TextWrapMode::Extend),
        f32::INFINITY,
        egui::TextStyle::Button,
    );
    let btn = ui
        .add(
            egui::Button::new(cart_text(&label))
                .min_size(widest.size() + 2.0 * ui.spacing().button_padding),
        )
        .on_hover_note("What this shop stocks from your wantlist");
    crate::ui::menu::dropdown_right(&btn, 320.0, |m| {
        let Some(b) = cart.filter(|b| b.wants() > 0) else {
            m.note("Nothing from your wantlist in stock here");
            m.note("Update the shop to refresh its stock");
            return;
        };
        m.scroll(360.0, |m| {
            for l in &b.offers {
                let mut terms = crate::vinyl_sheet::fmt_market_price(&discogs::MarketPrice {
                    value: l.price,
                    currency: l.currency.clone(),
                });
                if let Some(c) = l.condition.as_deref() {
                    terms.push_str(&format!(" · {}", cond_short(c)));
                }
                if m.item_detail(format!("{} – {}", l.artist, l.title), terms) {
                    *open = Some(l.listing_id);
                    m.close();
                }
            }
        });
        m.separator();
        let mut sum = b
            .subtotal
            .iter()
            .map(|(currency, value)| {
                crate::vinyl_sheet::fmt_market_price(&discogs::MarketPrice {
                    value: *value,
                    currency: currency.clone(),
                })
            })
            .collect::<Vec<_>>()
            .join(" + ");
        match &b.shipping {
            Some((p, c)) => sum.push_str(&format!(
                " + {} shipping",
                crate::vinyl_sheet::fmt_market_price(&discogs::MarketPrice {
                    value: *p,
                    currency: c.clone(),
                })
            )),
            None => sum.push_str(" · shipping not quoted"),
        }
        m.note(sum);
        if m.item("Shop wants on discogs.com ↗") {
            open_url(&format!("https://www.discogs.com/seller/{seller}/mywants"));
            m.close();
        }
    });
}

/// What the shop menu asked for, applied once its borrows are released.
#[derive(Default)]
struct SellerPick {
    switch_to: Option<String>,
    /// Browse every saved shop at once.
    all: bool,
    sweep: Option<String>,
    remove: Option<String>,
    /// Open the Add seller box.
    add: bool,
}

/// What a card click or context-menu pick asked for, applied after the grid
/// releases its borrows.
enum SellerAct {
    /// Open the label page for the listing's release.
    LabelPage(usize),
    /// Open the record sheet, carrying this seller's concrete offer.
    Open(usize),
    /// Open the record sheet and play its first track (⏎ on the cursor).
    Play(usize),
    /// Open the listing itself on discogs.com — where the purchase happens.
    Buy(usize),
    /// Add the listing's release to the Discogs wantlist.
    Want(usize),
    /// Start a crate dig from the listing's release — see [`crate::dig`].
    Dig(usize),
    /// Land in that shop with this listing ringed (a shops-mark pick).
    ShowIn(String, u64),
}

/// One read of the Market tab's stock, made off the UI thread: one shop's
/// listings, or the cheapest copy of every record across all of them.
pub(crate) struct SellerStock {
    listings: Vec<SellerListing>,
    /// The shop selling each listing in the all-shops read; empty for one shop.
    shops: Vec<String>,
    genres: HashMap<u64, Vec<String>>,
    shop_counts: HashMap<u64, u32>,
}

/// A listing's sale terms on a card or row: `$13.99 · VG+/VG+`. `from` marks
/// the cheapest of several shops' copies, whose grades differ: `from $13.99`.
fn sale_terms(l: &SellerListing, from: bool) -> String {
    let mut terms = crate::vinyl_sheet::fmt_market_price(&discogs::MarketPrice {
        value: l.price,
        currency: l.currency.clone(),
    });
    if from {
        return format!("from {terms}");
    }
    if let Some(c) = l.condition.as_deref() {
        terms.push_str(&format!(" · {}", cond_short(c)));
        if let Some(s) = l.sleeve_condition.as_deref() {
            terms.push_str(&format!("/{}", cond_short(s)));
        }
    }
    terms
}

/// A seller username, out of either a bare name or a pasted discogs.com URL
/// (`…/seller/NAME/profile`, `…/user/NAME`). `None` when nothing usable is in
/// the box.
fn parse_seller_input(input: &str) -> Option<String> {
    let s = input.trim().trim_end_matches('/');
    if s.is_empty() {
        return None;
    }
    for marker in ["/seller/", "/user/"] {
        if let Some(i) = s.find(marker) {
            let tail = &s[i + marker.len()..];
            let name = tail.split(['/', '?', '#']).next().unwrap_or("");
            return (!name.is_empty()).then(|| name.to_string());
        }
    }
    // A bare username: Discogs allows letters, digits, dot, underscore and
    // hyphen. Anything else in the box is a typo or an unrelated URL.
    let ok = s
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    (ok && !s.contains("discogs")).then(|| s.to_string())
}

/// `2h ago` / `3d ago` for the sweep stamp.
fn fmt_ago(unix: i64) -> String {
    let secs = (unix_now() - unix).max(0);
    match secs {
        0..=59 => "just now".to_string(),
        60..=3599 => format!("{}m ago", secs / 60),
        3600..=86_399 => format!("{}h ago", secs / 3600),
        _ => format!("{}d ago", secs / 86_400),
    }
}

/// The listing's "year · format · label catno" line, shared by the list
/// row and the record sheet's subtitle.
fn listing_sub(l: &SellerListing) -> String {
    [
        l.year.map(|y| y.to_string()),
        l.format.clone(),
        match (&l.label, &l.catalog_number) {
            (Some(a), Some(c)) => Some(format!("{a} {c}")),
            (Some(a), None) => Some(a.clone()),
            (None, Some(c)) => Some(c.clone()),
            (None, None) => None,
        },
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" · ")
}

/// The short grade out of Discogs's long form: `Very Good Plus (VG+)` → `VG+`.
/// Falls back to the full text when there are no parentheses (e.g. `Generic`).
pub(crate) fn cond_short(s: &str) -> &str {
    match (s.rfind('('), s.rfind(')')) {
        (Some(a), Some(b)) if a + 1 < b => &s[a + 1..b],
        _ => s,
    }
}

/// Rank of a media grade for the condition-floor filter, higher = better:
/// P=0, F=1, G=2, G+=3, VG=4, VG+=5, NM=6, M=7. `None` when the string isn't
/// a recognizable grade — a floor filter drops those, since it can't vouch
/// for them.
pub(crate) fn grade_rank(condition: &str) -> Option<u8> {
    match cond_short(condition) {
        "M" => Some(7),
        s if s.starts_with("NM") => Some(6), // Discogs writes "NM or M-"
        "VG+" => Some(5),
        "VG" => Some(4),
        "G+" => Some(3),
        "G" => Some(2),
        "F" => Some(1),
        "P" => Some(0),
        _ => None,
    }
}

/// The condition floors the Filters popup offers: label + minimum rank.
pub(crate) const GRADE_FLOORS: [(&str, u8); 5] = [
    ("G+ or better", 3),
    ("VG or better", 4),
    ("VG+ or better", 5),
    ("NM or better", 6),
    ("M only", 7),
];

/// Does a listing match the crates search? Same contract as
/// [`vinyl_matches`](crate::views::vinyl_matches): every whitespace-separated
/// term must appear somewhere in the folded haystack.
fn listing_matches(hay: &str, query: &str) -> bool {
    query.split_whitespace().all(|term| hay.contains(term))
}

/// The search haystack for one listing, built once per load rather than per
/// frame — a swept mega-shop is tens of thousands of rows, and re-formatting
/// them every keystroke is what this avoids.
fn listing_hay(l: &SellerListing) -> String {
    format!(
        "{} {} {} {} {} {}",
        l.artist,
        l.title,
        l.label.as_deref().unwrap_or(""),
        l.year.map(|y| y.to_string()).unwrap_or_default(),
        l.format.as_deref().unwrap_or(""),
        l.condition.as_deref().unwrap_or(""),
    )
    .to_lowercase()
}

impl App {
    /// Make sure the current seller's cached listings are in memory, or on
    /// their way. A swept shop can be tens of thousands of rows, so they are
    /// read off the UI thread (a cold read held the Market tab's first frame
    /// for a quarter of a second) and land here on a later frame. Lazy
    /// rather than part of `reload`, which runs on every search keystroke;
    /// a finished sweep drops them (see [`Self::drop_seller_listings`]) so
    /// a fresh read shows up.
    fn ensure_seller_listings(&mut self) {
        // The all-shops scope reads under the empty name, which no shop has.
        let scope = if self.seller_all { Some(String::new()) } else { self.seller_current.clone() };
        let Some(cur) = scope else {
            self.drop_seller_listings();
            return;
        };
        if self.seller_listings_for.as_deref() == Some(cur.as_str()) {
            return;
        }
        if let Some((who, rx)) = &self.seller_listings_rx {
            if *who == cur {
                match rx.try_recv() {
                    Ok(SellerStock { listings, shops, mut genres, shop_counts }) => {
                        // A listed record you own or want is tagged by your
                        // own sync, whatever the caches know.
                        for r in self.vinyl.iter().chain(self.wantlist.iter()) {
                            if !r.genres.is_empty() {
                                genres
                                    .entry(r.release_id)
                                    .or_insert_with(|| r.genres.clone());
                            }
                        }
                        self.seller_hay = listings.iter().map(listing_hay).collect();
                        self.seller_listings = listings;
                        self.seller_listing_shops = shops;
                        self.seller_shop_counts = shop_counts;
                        self.seller_genres = genres;
                        self.seller_listings_for = Some(cur);
                        self.seller_listings_rx = None;
                    }
                    Err(mpsc::TryRecvError::Empty) => {}
                    // The read failed: the shop shows empty rather than
                    // being asked for again every frame.
                    Err(mpsc::TryRecvError::Disconnected) => {
                        self.seller_listings_for = Some(cur);
                        self.seller_listings_rx = None;
                    }
                }
                return;
            }
        }
        // Another shop's rows must not show under this one's name while
        // its own are read.
        self.drop_seller_listings();
        let db = self.db_path.clone();
        let who = cur.clone();
        self.seller_listings_rx = Some((
            cur,
            util::read_off_thread(&self.db_path, &self.egui_ctx, move |c| {
                // ponytail: the all-shops scope holds every record in memory
                // and is filtered per frame like one shop's crates (78k
                // records across 37 shops is fine); page it in SQL if the
                // roster grows past a few hundred thousand listings.
                let (shops, listings) = if who.is_empty() {
                    c.cheapest_seller_listings()?.into_iter().unzip()
                } else {
                    (Vec::new(), c.list_seller_listings(&who)?)
                };
                let shop_counts = c.seller_shop_counts()?;
                // Genre tags for the crates. The inventory endpoint carries
                // none, so mine the release-detail cache, then the bulk
                // genre database for whatever it doesn't cover (after one
                // import, nearly everything). Listings in neither stay
                // unknown and drop out of a genre filter.
                let mut ids: Vec<u64> = listings.iter().map(|l| l.release_id).collect();
                ids.sort_unstable();
                ids.dedup();
                let mut genres = c.release_genres(&ids)?;
                if let Ok(Some(gdb)) = genredb::GenreDb::open(&genredb::default_path(&db)) {
                    let missing: Vec<u64> = ids
                        .iter()
                        .copied()
                        .filter(|id| !genres.contains_key(id))
                        .collect();
                    if let Ok(map) = gdb.genres_for(&missing) {
                        for (id, tags) in map {
                            genres.entry(id).or_insert(tags);
                        }
                    }
                }
                Ok(SellerStock { listings, shops, genres, shop_counts })
            }),
        ));
    }

    /// Land the sellers, shipping floors and wantlist watch read off the UI
    /// thread (see `reload`), and keep the current seller valid against the
    /// list: one no longer saved gives way to the first.
    pub(crate) fn poll_seller_meta(&mut self) {
        let Some((sellers, shipping, watch)) = self
            .seller_meta_rx
            .as_ref()
            .and_then(|rx| rx.try_recv().ok())
        else {
            return;
        };
        self.seller_meta_rx = None;
        self.sellers = sellers;
        // One aggregate row per seller; sellers updated before shipping was
        // cached simply have no entry.
        self.seller_shipping = shipping
            .into_iter()
            .map(|(seller, price, currency)| (seller, (price, currency)))
            .collect();
        self.wantlist_watch = watch;
        if let Some(cur) = &self.seller_current {
            if !self.sellers.iter().any(|s| &s.username == cur) {
                self.seller_current = None;
            }
        }
        if self.seller_current.is_none() {
            self.seller_current = self.sellers.first().map(|s| s.username.clone());
        }
    }

    /// Let the current seller's listings go, read in flight included: the
    /// Market tab's next frame reads them again (a sweep finished, the shop
    /// changed, the section was left).
    pub(crate) fn drop_seller_listings(&mut self) {
        self.seller_listings = Vec::new();
        self.seller_listing_shops = Vec::new();
        self.seller_shop_counts = HashMap::new();
        self.seller_mark_rows = None;
        self.seller_hay = Vec::new();
        self.seller_genres = HashMap::new();
        self.seller_listings_for = None;
        self.seller_listings_rx = None;
    }

    /// What the shop segment of the Market search field reads: the scope
    /// being browsed, or the way in when no shop is saved yet. A shop name
    /// longer than `max_chars` is cut so the search text keeps its room.
    pub(crate) fn shop_segment_label(&self, max_chars: usize) -> String {
        if self.sellers.is_empty() {
            return "Add shop".to_string();
        }
        let name = match &self.seller_current {
            _ if self.seller_all => format!("All {} shops", self.sellers.len()),
            Some(cur) => cur.clone(),
            None => "Pick a shop".to_string(),
        };
        if name.chars().count() > max_chars {
            format!("{}… ▾", name.chars().take(max_chars - 1).collect::<String>())
        } else {
            format!("{name} ▾")
        }
    }

    /// The shop segment of the Market search field. The shops live in a
    /// menu hung from `btn` (the field's trailing button, which names the
    /// scope) rather than a strip of chips: a strip stops working past a
    /// couple of dozen names, a sorted list scrolls. The add box hangs from
    /// the same button, opened by the menu's Add shop row; with no shop
    /// saved yet there is no menu and the button opens the box itself.
    /// Whatever either asked for is applied here.
    pub(crate) fn draw_shop_segment(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        btn: &egui::Response,
    ) {
        let mut pick_act = SellerPick::default();
        let mut add_clicked = false;
        let popup_id = ui.make_persistent_id("seller-add-popup");
        if self.sellers.is_empty() {
            self.seller_all = false;
            if btn.clicked() {
                ui.memory_mut(|m| m.toggle_popup(popup_id));
            }
        } else {
            self.draw_seller_picker(btn, &mut pick_act);
            // A click on the button while the add box is up would stack
            // the menu under the box.
            if btn.clicked() && ui.memory(|m| m.is_popup_open(popup_id)) {
                ui.memory_mut(|m| m.close_popup());
            }
        }
        // Opened this frame, by the button or by the menu's row last
        // frame: the field takes focus either way.
        let is_open = ui.memory(|m| m.is_popup_open(popup_id));
        let was_open: bool = ui.data(|d| d.get_temp(popup_id.with("was"))).unwrap_or(false);
        ui.data_mut(|d| d.insert_temp(popup_id.with("was"), is_open));
        let just_opened = is_open && !was_open;
        egui::popup::popup_below_widget(
            ui,
            popup_id,
            btn,
            egui::PopupCloseBehavior::CloseOnClickOutside,
            |ui| {
                // As wide as the shop menu it took over from, so the
                // swap reads as one surface.
                ui.set_min_width(320.0);
                ui.horizontal(|ui| crate::ui::control_row(ui, |ui| {
                    let edit = ui.add(
                        crate::ui::field::Field::singleline(&mut self.seller_add)
                            .width(250.0)
                            .hint("Shop username or URL"),
                    );
                    if just_opened {
                        edit.request_focus();
                    }
                    let submitted =
                        edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    if ui.button("Add").clicked() || submitted {
                        add_clicked = true;
                    }
                }));
                // What the sweep keeps: every genre, or a few. The
                // inventory endpoint ignores a genre parameter, so the
                // sweep applies this against the genre database.
                crate::ui::menu::embedded(ui, |m| {
                    m.header("Genres");
                    if m.selectable(self.seller_add_genres.is_empty(), "All genres") {
                        self.seller_add_genres.clear();
                    }
                    m.scroll(240.0, |m| {
                        for g in DISCOGS_GENRES {
                            let on = self.seller_add_genres.iter().any(|s| s == g);
                            if m.selectable(on, g) {
                                if on {
                                    self.seller_add_genres.retain(|s| s != g);
                                } else {
                                    self.seller_add_genres.push(g.to_string());
                                }
                            }
                        }
                    });
                });
            },
        );

        let SellerPick { switch_to, all, sweep, remove, add } = pick_act;
        if add {
            ui.memory_mut(|m| m.open_popup(popup_id));
        }
        if all || switch_to.is_some() {
            self.seller_all = all;
            if let Some(u) = switch_to {
                self.seller_current = Some(u);
            }
            self.seller_highlight = None;
            self.seller_scroll_to = false;
            self.ensure_seller_listings();
        }
        if add_clicked {
            match parse_seller_input(&self.seller_add) {
                Some(name) => {
                    let saved = Catalog::open(&self.db_path).and_then(|c| {
                        c.add_seller(&name)?;
                        c.set_seller_genres(&name, &self.seller_add_genres)?;
                        c.list_sellers()
                    });
                    match saved {
                        Ok(sellers) => {
                            self.sellers = sellers;
                            self.seller_all = false;
                            self.seller_current = Some(name);
                            self.seller_add.clear();
                            self.seller_add_genres.clear();
                            ctx.memory_mut(|m| m.close_popup());
                            self.ensure_seller_listings();
                        }
                        Err(e) => self.status = format!("Couldn't save shop: {e}"),
                    }
                }
                None => {
                    self.status = "That doesn't look like a Discogs username or shop URL.".into();
                }
            }
        }
        if let Some(name) = remove {
            match Catalog::open(&self.db_path).and_then(|c| {
                c.remove_seller(&name)?;
                c.list_sellers()
            }) {
                Ok(sellers) => {
                    self.sellers = sellers;
                    // Their copies leave the open sheet's seller block too.
                    self.refresh_sheet_stocked();
                    if self.seller_current.as_deref() == Some(name.as_str()) {
                        self.seller_current = self.sellers.first().map(|s| s.username.clone());
                        self.drop_seller_listings();
                        self.ensure_seller_listings();
                    }
                }
                Err(e) => self.status = format!("Couldn't remove shop: {e}"),
            }
        }
        if let Some(u) = sweep {
            self.spawn_sweep_seller(ctx.clone(), u);
        }
    }

    /// The open shop's cart button, for the toolbar's right end: what the
    /// shop stocks from the wantlist, folded the way the watch folds it
    /// (cheapest copy per record, subtotal, shipping floor), so the two
    /// never disagree about a basket. Nothing in the all-shops scope, which
    /// has no one cart.
    pub(crate) fn draw_shop_cart(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let Some(cur) = self.seller_current.clone().filter(|_| !self.seller_all) else {
            return;
        };
        let mine: Vec<(String, SellerListing)> = self
            .wantlist_watch
            .iter()
            .filter(|(s, _)| *s == cur)
            .cloned()
            .collect();
        let mut cart = seller_baskets(&mine).pop();
        if let Some(b) = cart.as_mut().filter(|b| b.shipping.is_none()) {
            b.shipping = self.seller_shipping.get(&cur).map(|(p, c)| (*p, c.clone()));
        }
        // A row of the cart clicked: open that offer.
        let mut cart_open: Option<u64> = None;
        draw_cart_button(ui, &cur, cart.as_ref(), self.wantlist.len(), &mut cart_open);
        if let Some(id) = cart_open {
            self.open_watch_offer(id, ctx);
        }
    }

    /// The Market tab body: the crates of the scope the search field names
    /// (one shop, or every shop at once) as a virtualized card grid.
    /// `query` is the shared vinyl search, already trimmed and lowercased.
    pub(crate) fn draw_sellers(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, query: &str) {
        self.ensure_seller_listings();
        // Whatever shop is open is the one the next launch opens on.
        if let Some(cur) = &self.seller_current {
            if self.config.seller_current != *cur {
                self.config.seller_current = cur.clone();
                self.save_config();
            }
        }

        // --- Empty states. ----------------------------------------------------
        if self.sellers.is_empty() {
            ui.add_space(40.0);
            ui.vertical_centered(|ui| {
                ui.heading("Dig through a shop's stock");
                ui.add_space(6.0);
                ui.label("Add a shop from the search field, then update its stock");
                ui.add_space(4.0);
            });
            return;
        }
        // The open shop; `None` in the all-shops scope.
        let shop = if self.seller_all {
            None
        } else {
            let Some(shop) = self
                .seller_current
                .as_deref()
                .and_then(|cur| self.sellers.iter().find(|s| s.username == cur))
                .cloned()
            else {
                return;
            };
            Some(shop)
        };

        // --- Meta line: how fresh these crates are. ---------------------------
        // Filter up front so the count in the meta line matches the grid. The
        // genre facet reads `seller_genres`; a listing with no known tags
        // can't match a tag, so it drops out (the Filters popup says how many
        // records have known tags at all). Same rule for a year, format or
        // condition bound the listing can't answer — a filter never vouches
        // for a row it can't check.
        let flt = self.vinyl_flt.clone();
        let active = flt.active(true);
        let unfiltered = query.is_empty() && active == 0;
        let price_cap = flt.price_cap();
        let filtered: Vec<usize> = self
            .seller_listings
            .iter()
            .enumerate()
            .filter(|(i, l)| {
                (query.is_empty() || listing_matches(&self.seller_hay[*i], query))
                    && (flt.genres.is_empty()
                        || self
                            .seller_genres
                            .get(&l.release_id)
                            .is_some_and(|tags| flt.genres_keep(tags)))
                    && flt.year_keep(l.year)
                    && flt.format_keep(l.format.as_deref())
                    && price_cap.is_none_or(|cap| l.price <= cap)
                    && flt.min_grade.is_none_or(|floor| {
                        l.condition
                            .as_deref()
                            .and_then(grade_rank)
                            .is_some_and(|r| r >= floor)
                    })
                    && !(flt.hide_owned && self.vinyl_owned.contains(&l.release_id))
                    && !(flt.hide_wanted && self.vinyl_wanted.contains(&l.release_id))
                    && !(flt.hide_viewed && self.viewed_releases.contains(&l.release_id))
            })
            .map(|(i, _)| i)
            .collect();
        // Where the ringed pick sits in the visible crates, if it does:
        // what the scroll-to aims at, and what the meta line explains when
        // the current search or filters hide it.
        let highlight_pos = self.seller_highlight.and_then(|id| {
            filtered
                .iter()
                .position(|&i| self.seller_listings[i].listing_id == id)
        });
        let highlight_hidden = highlight_pos.is_none()
            && self.seller_highlight.is_some_and(|id| {
                self.seller_listings.iter().any(|l| l.listing_id == id)
            });
        ui.add_space(6.0);
        let mut go_all = false;
        ui.horizontal(|ui| {
            let mut meta = match (&shop, unfiltered) {
                (Some(s), true) => format!("{} records in stock", s.cached),
                (Some(s), false) => format!("{} of {} records match", filtered.len(), s.cached),
                // Still being read: no count to give yet.
                (None, _) if self.seller_listings_rx.is_some() => {
                    format!("All {} shops", self.sellers.len())
                }
                (None, true) => format!(
                    "{} records in {} shops",
                    self.seller_listings.len(),
                    self.sellers.len()
                ),
                (None, false) => {
                    format!("{} of {} records match", filtered.len(), self.seller_listings.len())
                }
            };
            // The freshness stamp says what the crates are a picture of. A
            // shop whose update never finished has a head of its stock, not
            // the stock: the watch can't count wants the sweep never reached,
            // so that gap is named (with the shop's size when the first page
            // stamped it) instead of reading as "never updated".
            match shop.as_ref().map(|s| (s.swept_at, s.cached, s.reported)) {
                None => {}
                Some((Some(t), _, _)) => meta.push_str(&format!(" · updated {}", fmt_ago(t))),
                Some((None, 0, _)) => meta.push_str(" · never updated"),
                Some((None, n, Some(total))) if total > n => meta.push_str(&format!(
                    " · the last update stopped at {n} of {total} listings, update again to finish"
                )),
                Some((None, _, _)) => {
                    meta.push_str(" · the last update stopped early, update again to finish")
                }
            }
            if highlight_hidden {
                meta.push_str(" · the record you picked is hidden by the search or filters");
            }
            ui.label(egui::RichText::new(meta).weak());
            // A search of one shop is one click from the same search of all.
            if shop.is_some() && !query.is_empty() && self.sellers.len() > 1 {
                let label = format!("Search all {} shops", self.sellers.len());
                go_all = crate::ui::button::compact(ui, &label, false).clicked();
            }
        });
        if go_all {
            self.seller_all = true;
            self.seller_highlight = None;
            self.ensure_seller_listings();
            return;
        }
        ui.add_space(4.0);

        // Being read (see `ensure_seller_listings`): the first of the stock
        // as ghosts, not an empty shop.
        if self.seller_listings_rx.is_some() && self.seller_listings.is_empty() {
            if self.config.vinyl_view == "list" {
                ui.spacing_mut().item_spacing.y = 0.0;
                for i in 0..3 {
                    crate::ui::ghost_row(ui, ROW_H, 60.0, i);
                }
            } else {
                crate::ui::ghost_wall(ui, CAPTION_H, 3, 2);
            }
            return;
        }
        if self.seller_listings.is_empty() {
            ui.add_space(crate::ui::tokens::space::S7);
            ui.vertical_centered(|ui| {
                match &shop {
                    Some(shop) if shop.swept_at.is_none() => ui.label(format!(
                        "Nothing cached for {} yet. Update to pull its stock.",
                        shop.username
                    )),
                    Some(_) => ui.label("No vinyl in this shop's stock"),
                    None => ui.label("No shop has stock cached yet"),
                };
            });
            return;
        }
        if filtered.is_empty() {
            ui.add_space(crate::ui::tokens::space::S7);
            ui.vertical_centered(|ui| {
                // Name the tags when they're the whole filter ("Techno and
                // Ambient", or "Techno or Ambient" in Any mode: the rule
                // actually applied); with year/price/condition bounds in
                // play a generic line beats a paragraph.
                let only_tags = active == flt.genres.len() && !flt.genres.is_empty();
                let msg = {
                    match (only_tags, active > 0, query.is_empty()) {
                        (true, _, true) => {
                            let tags_named = match flt.genres.as_slice() {
                                [one] => one.clone(),
                                [head @ .., last] => {
                                    let op = if flt.tags_any { "or" } else { "and" };
                                    format!("{} {op} {last}", head.join(", "))
                                }
                                [] => unreachable!(),
                            };
                            format!("Nothing in stock is tagged {tags_named}.")
                        }
                        (_, true, _) => "Nothing in stock matches those filters.".to_string(),
                        (_, false, _) => "Nothing in stock matches that search.".to_string(),
                    }
                };
                ui.label(egui::RichText::new(msg).weak());
            });
            return;
        }

        // --- The crates: a virtualized cover grid, or compact rows. -----------
        // Same sizing rules as the vinyl wall, but drawn through `show_rows` —
        // a swept shop can be tens of thousands of cards, and only the visible
        // rows should cost layout (or a cover download). The toolbar's grid/
        // list toggle applies here too: rows put price and grade in a column,
        // which is how a shop's crates get compared.
        let mut act: Option<SellerAct> = None;
        let list_mode = self.config.vinyl_view == "list";
        const GAP: f32 = crate::ui::cover::WALL_GAP;
        /// Caption budget under each cover: artist, title, price line.
        const CAPTION_H: f32 = 58.0;
        /// List rows run taller than the wall's: a shop is read one row at a
        /// time against a column of prices, and the thumb has to carry the
        /// sleeve on its own.
        const ROW_H: f32 = 72.0;
        let avail = ui.available_width();
        let (grid_cols, cover_side) = crate::ui::cover::wall_fit(avail);
        let cols = if list_mode { 1 } else { grid_cols };
        let row_h = if list_mode { ROW_H } else { cover_side + CAPTION_H + GAP };
        let n_rows = filtered.len().div_ceil(cols);

        // Keyboard over the crates, the same walk as the shelves: the ring
        // is the cursor, arrows and WASD move it, ⏎ opens the
        // record and plays its first track. A step onto a row scrolled away
        // brings it into view (the grid is virtualized, so the row's place
        // is computed rather than measured).
        let mut highlight_pos = highlight_pos;
        let mut key_scroll: Option<usize> = None;
        if self.shelf_keys(ctx) && !filtered.is_empty() && !crate::ui::field::wants_keyboard_input(ctx) {
            let (step, enter) = Self::shelf_key_step(ctx, cols);
            if step != 0 {
                let last = filtered.len() as isize - 1;
                let next = match highlight_pos {
                    None => 0,
                    Some(i) => (i as isize + step).clamp(0, last),
                } as usize;
                self.seller_highlight = Some(self.seller_listings[filtered[next]].listing_id);
                highlight_pos = Some(next);
                key_scroll = Some(next / cols);
            } else if enter {
                if let Some(p) = highlight_pos {
                    act = Some(SellerAct::Play(filtered[p]));
                }
            }
        }

        // `show_rows` pitches rows at `row_h` plus the outer item spacing;
        // every scroll target has to use that pitch or it drifts a few
        // pixels per row and lands short of its row deep in a long list.
        let row_h_sp = row_h + ui.spacing().item_spacing.y;
        let mut area = egui::ScrollArea::vertical().auto_shrink([false, false]);
        if let Some(p) = highlight_pos.filter(|_| self.seller_scroll_to) {
            // One row of context above the ringed one, so it doesn't sit
            // flush against the top edge.
            area = area.vertical_scroll_offset(((p / cols) as f32 - 1.0).max(0.0) * row_h_sp);
            self.seller_scroll_to = false;
        }
        // The records on screen get their tracklists cached once the scroll
        // rests, so the one clicked next opens without a request. Not while
        // scrolling: rows flying past would queue a request each.
        let settled = ui.input(|i| i.raw_scroll_delta == egui::Vec2::ZERO);
        area.show_rows(ui, row_h, n_rows, |ui, rows| {
            if settled {
                let n = filtered.len();
                for &idx in &filtered[(rows.start * cols).min(n)..(rows.end * cols).min(n)] {
                    self.warm_release_detail(self.seller_listings[idx].release_id);
                }
            }
            if let Some(r) = key_scroll {
                let top = ui.min_rect().top() - rows.start as f32 * row_h_sp;
                ui.scroll_to_rect(
                    egui::Rect::from_min_size(
                        egui::pos2(ui.min_rect().left(), top + r as f32 * row_h_sp),
                        egui::vec2(avail, row_h),
                    ),
                    None,
                );
            }
            if list_mode {
                ui.spacing_mut().item_spacing.y = 0.0;
                for i in rows {
                    let idx = filtered[i];
                    if let Some(a) = self.seller_row(ui, idx, ROW_H) {
                        act = Some(a);
                    }
                }
            } else {
                ui.spacing_mut().item_spacing = egui::vec2(GAP, GAP);
                for row in rows {
                    ui.horizontal_top(|ui| {
                        for slot in 0..cols {
                            let Some(&idx) = filtered.get(row * cols + slot) else {
                                break;
                            };
                            if let Some(a) = self.seller_card(ui, idx, cover_side) {
                                act = Some(a);
                            }
                        }
                    });
                }
                ui.add_space(8.0);
            }
        });

        match act {
            Some(SellerAct::Open(idx)) => self.open_seller_listing(idx, ctx),
            Some(SellerAct::Play(idx)) => {
                // The tracklist may still be loading; the sheet starts
                // playback itself once it has one (see `pending_play`).
                self.open_seller_listing(idx, ctx);
                if let Some(sheet) = self.vinyl_sheet.as_mut() {
                    sheet.pending_play = true;
                }
            }
            Some(SellerAct::Buy(idx)) => {
                if let Some(l) = self.seller_listings.get(idx) {
                    let url = l.uri.clone().unwrap_or_else(|| {
                        format!("https://www.discogs.com/release/{}", l.release_id)
                    });
                    open_url(&url);
                }
            }
            Some(SellerAct::Want(idx)) => {
                if let Some(l) = self.seller_listings.get(idx) {
                    let edit = VinylEdit::Want {
                        release_ids: vec![l.release_id],
                        label: format!("{} — {}", l.artist, l.title),
                    };
                    self.request_vinyl_edit(ctx.clone(), edit);
                }
            }
            Some(SellerAct::Dig(idx)) => {
                if let Some(l) = self.seller_listings.get(idx).cloned() {
                    let sub = match (l.year, l.format.as_deref()) {
                        (Some(y), Some(f)) => format!("{y} · {f}"),
                        (Some(y), None) => y.to_string(),
                        (None, Some(f)) => f.to_string(),
                        (None, None) => String::new(),
                    };
                    self.start_dig_release(
                        l.release_id,
                        l.artist,
                        l.title,
                        l.label,
                        sub,
                        l.thumb_url,
                    );
                    self.show_dig_window();
                }
            }
            Some(SellerAct::ShowIn(seller, listing_id)) => {
                self.seller_all = false;
                self.seller_current = Some(seller);
                self.ensure_seller_listings();
                self.seller_highlight = Some(listing_id);
                self.seller_scroll_to = true;
            }
            Some(SellerAct::LabelPage(idx)) => {
                if let Some(l) = self.seller_listings.get(idx).cloned() {
                    self.open_label_page(l.release_id, l.label);
                }
            }
            None => {}
        }
    }

    /// The shop picker: one menu under the search field's shop segment. A
    /// filter field leads it, then the all-shops scope; the shops holding wantlist records come first, by
    /// wants held, the rest by freshness; every row says how big the shop
    /// is, how many wants it holds and when it was last updated, a stale
    /// or never-updated shop in the caution ink. The current shop's
    /// actions and an Add seller row close the panel, so the row of
    /// buttons stays a row. Picks land in the out-params, applied once the
    /// borrows on `self.sellers` are released.
    fn draw_seller_picker(&self, btn: &egui::Response, act: &mut SellerPick) {
        use crate::ui::tokens::color;
        // Distinct wantlist records each shop stocks: the watch's rows
        // are per copy, so a shop with two copies of one want holds one.
        let mut wants: HashMap<&str, HashSet<u64>> = HashMap::new();
        for (seller, l) in &self.wantlist_watch {
            wants.entry(seller.as_str()).or_default().insert(l.release_id);
        }
        let wants_of = |s: &SellerShop| wants.get(s.username.as_str()).map_or(0, |w| w.len());
        let mut sorted: Vec<&SellerShop> = self.sellers.iter().collect();
        sorted.sort_by(|a, b| {
            wants_of(b)
                .cmp(&wants_of(a))
                .then(b.swept_at.cmp(&a.swept_at))
                .then_with(|| a.username.to_lowercase().cmp(&b.username.to_lowercase()))
        });
        let current = self.seller_current.as_deref().filter(|_| !self.seller_all);
        crate::ui::menu::dropdown(btn, 320.0, |m| {
            // The filter lives in egui's memory under the menu's id and
            // starts empty each visit; the field takes focus on the pass
            // the panel opens.
            let fresh = m.just_opened();
            let ui = m.ui();
            let filter_id = ui.id().with("shop_filter");
            let mut filter: String = if fresh {
                String::new()
            } else {
                ui.data(|d| d.get_temp(filter_id)).unwrap_or_default()
            };
            let field = crate::ui::field::Field::singleline(&mut filter)
                .hint("Filter shops")
                .width(ui.available_width() - crate::ui::field::margin().sum().x)
                .show(ui);
            if fresh {
                field.request_focus();
            }
            let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let query = filter.trim().to_lowercase();
            ui.data_mut(|d| d.insert_temp(filter_id, filter.clone()));

            let shown: Vec<&SellerShop> = sorted
                .iter()
                .copied()
                .filter(|s| query.is_empty() || s.username.to_lowercase().contains(&query))
                .collect();
            if enter {
                if let Some(first) = shown.first() {
                    act.switch_to = Some(first.username.clone());
                    m.close();
                }
            }
            if query.is_empty() {
                if m.selectable(self.seller_all, format!("All {} shops", self.sellers.len())) {
                    act.all = true;
                    m.close();
                }
                m.separator();
            }
            let (held, rest): (Vec<&SellerShop>, Vec<&SellerShop>) =
                shown.iter().partition(|s| wants_of(s) > 0);
            m.scroll(280.0, |m| {
                if shown.is_empty() {
                    m.note("No saved shop matches that");
                }
                for (header, group) in [("Hold your wants", held), ("Other shops", rest)] {
                    if group.is_empty() {
                        continue;
                    }
                    m.header(header);
                    for shop in group {
                        // Two facts in one shape every row: the wants the
                        // shop holds (the group's own order), then its age.
                        // The crate count waits for the status line under
                        // the row. Only a shop never swept wears the caution
                        // ink: every age is normal for a shop updated by hand.
                        // The ink is the whole detail's, so a never-swept
                        // shop says only that: its wants would wear the
                        // warning too.
                        let (detail, ink) = match shop.swept_at {
                            Some(t) => {
                                let age = fmt_ago(t);
                                let detail = match wants_of(shop) {
                                    0 => age,
                                    1 => format!("1 want · {age}"),
                                    n => format!("{n} wants · {age}"),
                                };
                                (detail, color::LABEL_3)
                            }
                            None => ("never updated".to_string(), color::CAUTION),
                        };
                        let active = current == Some(shop.username.as_str());
                        let name = match &shop.ships_from {
                            Some(c) => format!("{} · {c}", shop.username),
                            None => shop.username.clone(),
                        };
                        if m.selectable_detail_in(active, name, detail, ink) {
                            act.switch_to = Some(shop.username.clone());
                            m.close();
                        }
                        // The menu opens on the current shop, wherever the
                        // order put it.
                        if active && fresh {
                            m.ui().scroll_to_cursor(Some(egui::Align::Center));
                        }
                    }
                }
            });
            if let Some(cur) = current {
                m.separator();
                // The note sizes the job once the shop's size is known
                // (stamped by the first page of any earlier update): a
                // distributor takes half an hour, and saying so up front
                // is what makes the wait reasonable rather than a hang.
                let size_note = self
                    .sellers
                    .iter()
                    .find(|s| s.username == cur)
                    .and_then(|s| s.reported)
                    .map(|n| format!("{n} items, about {} min", crate::jobs::seller_sweep_minutes(n)))
                    .unwrap_or_default();
                let busy = self.is_busy();
                let detail = if busy { "waiting for the current job".to_string() } else { size_note };
                if m.item_detail("Update stock", detail) && !busy {
                    act.sweep = Some(cur.to_string());
                    m.close();
                }
                // Quotes are per record and location-specific; a seller
                // publishing only a free-text policy has none, and that
                // absence never reads as free, so it says nothing.
                let ships = self
                    .seller_shipping
                    .get(cur)
                    .map(|(price, currency)| {
                        format!(
                            "ships from {}",
                            crate::vinyl_sheet::fmt_market_price(&discogs::MarketPrice {
                                value: *price,
                                currency: currency.clone(),
                            })
                        )
                    })
                    .unwrap_or_default();
                if m.item_detail("Open shop on Discogs", ships) {
                    open_url(&format!("https://www.discogs.com/seller/{cur}/profile"));
                    m.close();
                }
                if m.item_danger("Remove shop").clicked() {
                    act.remove = Some(cur.to_string());
                    m.close();
                }
            }
            m.separator();
            if m.item("Add shop") {
                act.add = true;
                m.close();
            }
        });
    }

    /// The shops mark on a card or row: how many other saved shops stock
    /// this record (every shop, in the all-shops scope), with those copies
    /// in a menu under it, cheapest first; a row lands in that shop with
    /// the copy ringed. Set at `at` by `anchor`. Hands back its rect and
    /// whether it took the click, or `None` for a record one shop stocks.
    fn shops_mark(
        &mut self,
        ui: &mut egui::Ui,
        anchor: egui::Align2,
        at: egui::Pos2,
        idx: usize,
        act: &mut Option<SellerAct>,
    ) -> Option<(egui::Rect, bool)> {
        let l = self.seller_listings.get(idx)?;
        let (release_id, listing_id) = (l.release_id, l.listing_id);
        let n = *self.seller_shop_counts.get(&release_id)?;
        let here = self.seller_current.clone().filter(|_| !self.seller_all);
        let label = if here.is_some() { format!("+{}", n - 1) } else { n.to_string() };
        let galley = crate::ui::icon_text(
            named(icons::STOREFRONT),
            &label,
            crate::ui::tokens::font::footnote(),
        )
        .into_galley(ui, Some(egui::TextWrapMode::Extend), f32::INFINITY, egui::TextStyle::Small);
        let rect = anchor.anchor_size(at, galley.size() + egui::vec2(12.0, 6.0));
        let resp = ui
            .interact(rect, ui.id().with(("seller-shops", listing_id)), egui::Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_note(if here.is_some() {
                "Other shops with this record"
            } else {
                "Shops with this record"
            });
        let (bg, fg) = if resp.hovered() {
            (crate::ui::tokens::color::accent(), egui::Color32::from_gray(20))
        } else {
            (egui::Color32::from_black_alpha(190), egui::Color32::from_gray(240))
        };
        ui.painter().rect_filled(
            rect,
            egui::Rounding::same(crate::ui::tokens::radius::SM),
            bg,
        );
        ui.painter().galley(rect.center() - galley.size() / 2.0, galley, fg);
        crate::ui::menu::dropdown(&resp, 380.0, |m| {
            if self.seller_mark_rows.as_ref().map(|(id, _)| *id) != Some(release_id) {
                let mut rows =
                    crate::vinyl_sheet::stocked_rows(&self.sheet_stocked(release_id), None);
                rows.retain(|r| Some(&r.seller) != here.as_ref());
                self.seller_mark_rows = Some((release_id, rows));
            }
            let Some((_, rows)) = &self.seller_mark_rows else { return };
            m.scroll(240.0, |m| {
                for r in rows {
                    if m.item_detail(&r.label, &r.detail) {
                        *act = Some(SellerAct::ShowIn(r.seller.clone(), r.listing_id));
                        m.close();
                    }
                }
            });
        });
        Some((rect, resp.clicked()))
    }

    /// One card of the crates: cover, credit, and the sale terms — price and
    /// grade are what separate flipping through a shop from browsing a
    /// discography, so they get the caption's third line. Returns what the
    /// user asked of it, if anything.
    fn seller_card(&mut self, ui: &mut egui::Ui, idx: usize, cover_side: f32) -> Option<SellerAct> {
        use crate::ui::tokens::color;

        // Snapshot the card's strings before `dig_cover` needs `self` mutably.
        let (listing_id, release_id, artist, title, sub, price_line, thumb, owned, wanted, viewed) = {
            let l = self.seller_listings.get(idx)?;
            let sub = match (l.year, l.format.as_deref()) {
                (Some(y), Some(f)) => format!("{y} · {f}"),
                (Some(y), None) => y.to_string(),
                (None, Some(f)) => f.to_string(),
                (None, None) => String::new(),
            };
            let price_line = sale_terms(
                l,
                self.seller_all && self.seller_shop_counts.contains_key(&l.release_id),
            );
            (
                l.listing_id,
                l.release_id,
                l.artist.clone(),
                l.title.clone(),
                sub,
                price_line,
                l.thumb_url.clone(),
                self.vinyl_owned.contains(&l.release_id),
                self.vinyl_wanted.contains(&l.release_id),
                self.viewed_releases.contains(&l.release_id),
            )
        };
        let tex = thumb.as_deref().and_then(|u| self.dig_cover(u).cloned());
        let highlighted = self.seller_highlight == Some(listing_id);

        let mut act: Option<SellerAct> = None;
        ui.allocate_ui_with_layout(
            egui::vec2(cover_side, cover_side + 58.0),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                let (rect, resp) = ui
                    .allocate_exact_size(egui::vec2(cover_side, cover_side), egui::Sense::click());
                let resp = resp
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_note("Open the record to listen, want or buy");
                // The dig disc's hit area is claimed before anything paints, so
                // the cover's hover frame can see the disc's hover and the pair
                // reveals together (same reasoning as the shelf grid).
                const D: f32 = 30.0;
                let dig_rect = egui::Rect::from_min_size(
                    egui::pos2(rect.right() - D - 6.0, rect.bottom() - D - 6.0),
                    egui::vec2(D, D),
                );
                let dig_hit = ui.interact(
                    dig_rect,
                    ui.id().with(("seller-dig", listing_id)),
                    egui::Sense::click(),
                );
                let dig_hovered = dig_hit.hovered();
                // Cart disc to the dig disc's left: straight to the listing
                // on discogs.com, whose Add to Cart button is the real cart —
                // the API has no cart endpoint, so there's no local stand-in.
                let cart_rect = egui::Rect::from_min_size(
                    egui::pos2(rect.right() - 2.0 * D - 12.0, rect.bottom() - D - 6.0),
                    egui::vec2(D, D),
                );
                let cart_hit = ui.interact(
                    cart_rect,
                    ui.id().with(("seller-cart", listing_id)),
                    egui::Sense::click(),
                );
                let cart_hovered = cart_hit.hovered();
                let card_hovered = resp.hovered() || dig_hovered || cart_hovered;
                crate::ui::cover::paint_at(ui, rect, tex.as_ref(), 6.0, Some(28.0));
                if card_hovered {
                    ui.painter().rect_stroke(
                        rect,
                        egui::Rounding::same(6.0),
                        egui::Stroke::new(2.0, color::accent()),
                    );
                }
                // The ringed pick: a ring just outside the cover, so it
                // reads at a glance among a wall of cards.
                if highlighted {
                    ui.painter().rect_stroke(
                        rect.expand(3.0),
                        egui::Rounding::same(8.0),
                        egui::Stroke::new(3.0, color::accent_hover()),
                    );
                }
                // Membership chips: a record already on a shelf is the one
                // thing worth knowing before the price — you either have it
                // or you're already hunting it.
                let mut chip_x = rect.left() + 5.0;
                for (show, list) in [
                    (owned, VinylList::Collection),
                    (!owned && wanted, VinylList::Wantlist),
                ] {
                    if !show {
                        continue;
                    }
                    let chip = crate::ui::icon::shelf_chip(
                        ui.painter(),
                        egui::pos2(chip_x, rect.top() + 5.0),
                        list,
                    );
                    chip_x = chip.right() + 4.0;
                }
                // Viewed eye, top-right: you already pulled this record out
                // and listened — the mark that stops a long dig from
                // re-auditioning the same crates.
                if viewed {
                    let c = egui::pos2(rect.right() - 13.0, rect.top() + 13.0);
                    ui.painter()
                        .circle_filled(c, 10.0, egui::Color32::from_black_alpha(170));
                    crate::ui::icon::eye(
                        ui.painter(),
                        c,
                        5.0,
                        egui::Color32::from_gray(235),
                        true,
                    );
                }
                // Dig disc, bottom-right: start a crate dig from this listing.
                // Hover-revealed like the shelf grid's, and its click never
                // falls through to the cover underneath.
                let mut dig_clicked = false;
                if card_hovered {
                    let bg = if dig_hovered {
                        crate::ui::tokens::color::PLAY_HOVER
                    } else {
                        egui::Color32::from_black_alpha(190)
                    };
                    let fg = if dig_hovered {
                        egui::Color32::from_gray(20)
                    } else {
                        egui::Color32::from_gray(240)
                    };
                    ui.painter().circle_filled(dig_rect.center(), D / 2.0, bg);
                    ui.painter().text(
                        dig_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "🔍",
                        egui::FontId::proportional(14.0),
                        fg,
                    );
                    let dig_hit = dig_hit.on_hover_cursor(egui::CursorIcon::PointingHand);
                    if dig_hit
                        .on_hover_note(
                            "Dig from here: records like this on Discogs that \
                             aren't in your collection",
                        )
                        .clicked()
                    {
                        dig_clicked = true;
                        act = Some(SellerAct::Dig(idx));
                    }
                }
                // Cart disc: hover-revealed like the dig disc; opens the
                // listing page, whose Add to Cart button is the real cart.
                let mut cart_clicked = false;
                if card_hovered {
                    let bg = if cart_hovered {
                        crate::ui::tokens::color::accent()
                    } else {
                        egui::Color32::from_black_alpha(190)
                    };
                    let fg = if cart_hovered {
                        egui::Color32::from_gray(20)
                    } else {
                        egui::Color32::from_gray(240)
                    };
                    ui.painter().circle_filled(cart_rect.center(), D / 2.0, bg);
                    crate::ui::icon::cart(ui.painter(), cart_rect.center(), 6.0, fg);
                    let cart_hit = cart_hit.on_hover_cursor(egui::CursorIcon::PointingHand);
                    if cart_hit
                        .on_hover_note("Add to cart on discogs.com (opens the listing)")
                        .clicked()
                    {
                        cart_clicked = true;
                        act = Some(SellerAct::Buy(idx));
                    }
                }
                // Shops mark, bottom-left: who else stocks it.
                let mark_clicked = self
                    .shops_mark(
                        ui,
                        egui::Align2::LEFT_BOTTOM,
                        egui::pos2(rect.left() + 5.0, rect.bottom() - 5.0),
                        idx,
                        &mut act,
                    )
                    .is_some_and(|(_, clicked)| clicked);
                if resp.clicked() && !dig_clicked && !cart_clicked && !mark_clicked {
                    act = Some(SellerAct::Open(idx));
                }
                crate::ui::menu::context(&resp, |ui| {
                    crate::ui::menu::text(ui, egui::RichText::new(&title).strong());
                    crate::ui::menu::text(ui, egui::RichText::new(&artist).weak());
                    ui.separator();
                    if ui
                        .button("Add to cart on Discogs ↗")
                        .on_hover_note("Opens the listing; cart and checkout live on discogs.com")
                        .clicked()
                    {
                        act = Some(SellerAct::Buy(idx));
                        ui.close_menu();
                    }
                    if ui
                        .button("Dig from here")
                        .on_hover_note("Walk Discogs outward from this record, by artist or label")
                        .clicked()
                    {
                        act = Some(SellerAct::Dig(idx));
                        ui.close_menu();
                    }
                    let already = owned || wanted;
                    if ui
                        .add_enabled(!already, egui::Button::new("Add to wantlist"))
                        .clicked()
                    {
                        act = Some(SellerAct::Want(idx));
                        ui.close_menu();
                    }
                    if ui
                        .button("Browse the label")
                        .on_hover_note("The label's whole run as a list, your shelves marked")
                        .clicked()
                    {
                        act = Some(SellerAct::LabelPage(idx));
                        ui.close_menu();
                    }
                    if ui.button("Open release page ↗").clicked() {
                        open_url(&format!("https://www.discogs.com/release/{release_id}"));
                        ui.close_menu();
                    }
                });

                // Caption: artist (strong), title, then the sale terms.
                let clip = |s: &str| s.to_string();
                ui.add_space(4.0);
                ui.scope(|ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    ui.set_max_width(cover_side);
                    ui.add(
                        egui::Label::new(egui::RichText::new(clip(&artist)).small().strong())
                            .truncate(),
                    );
                    let title_text = if sub.is_empty() {
                        title.clone()
                    } else {
                        format!("{title} · {sub}")
                    };
                    ui.add(
                        egui::Label::new(egui::RichText::new(title_text).small().weak()).truncate(),
                    );
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(&price_line)
                                .small()
                                .color(crate::ui::tokens::color::POSITIVE),
                        )
                        .truncate(),
                    );
                });
                // Keep the ui id stable per listing so egui state (context
                // menu) doesn't jump between cards as the grid scrolls.
                let _ = listing_id;
            },
        );
        act
    }

    /// One row of the crates in list layout: thumb, credit, and the sale
    /// terms pinned to the right edge, where a column of prices and grades
    /// scans the way a wall of cards can't. Same actions as the card — click
    /// opens the record, right-click buys, wantlists or digs.
    fn seller_row(&mut self, ui: &mut egui::Ui, idx: usize, row_h: f32) -> Option<SellerAct> {
        const THUMB: f32 = 60.0;
        // Snapshot the row's strings before `dig_cover` needs `self` mutably
        // (same dance as the card).
        let (listing_id, release_id, artist, title, sub, genres, price_line, thumb, owned, wanted, viewed) = {
            let l = self.seller_listings.get(idx)?;
            let sub = listing_sub(l);
            let price_line = sale_terms(
                l,
                self.seller_all && self.seller_shop_counts.contains_key(&l.release_id),
            );
            (
                l.listing_id,
                l.release_id,
                l.artist.clone(),
                l.title.clone(),
                sub,
                self.seller_genres
                    .get(&l.release_id)
                    .map(|tags| tags.join(" · "))
                    .unwrap_or_default(),
                price_line,
                l.thumb_url.clone(),
                self.vinyl_owned.contains(&l.release_id),
                self.vinyl_wanted.contains(&l.release_id),
                self.viewed_releases.contains(&l.release_id),
            )
        };
        let tex = thumb.as_deref().and_then(|u| self.dig_cover(u).cloned());

        let mut act: Option<SellerAct> = None;
        let avail = ui.available_width();
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(avail, row_h), egui::Sense::click());
        let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
        if resp.hovered() {
            ui.painter().rect_filled(
                rect,
                egui::Rounding::same(6.0),
                crate::ui::tokens::color::surface(),
            );
        }
        // The ringed pick: a wash and a ring on the row, matching the
        // card grid's treatment.
        if self.seller_highlight == Some(listing_id) {
            ui.painter().rect_filled(
                rect.shrink(1.0),
                egui::Rounding::same(6.0),
                crate::ui::tokens::color::accent_soft().gamma_multiply(0.45),
            );
            ui.painter().rect_stroke(
                rect.shrink(1.0),
                egui::Rounding::same(6.0),
                egui::Stroke::new(2.0, crate::ui::tokens::color::accent_hover()),
            );
        }
        ui.painter().line_segment(
            [
                egui::pos2(rect.left() + THUMB + 14.0, rect.bottom()),
                egui::pos2(rect.right(), rect.bottom()),
            ],
            egui::Stroke::new(1.0, egui::Color32::from_gray(38)),
        );
        let thumb_rect = egui::Rect::from_min_size(
            egui::pos2(rect.left() + 4.0, rect.center().y - THUMB / 2.0),
            egui::vec2(THUMB, THUMB),
        );
        crate::ui::cover::paint_at(ui, thumb_rect, tex.as_ref(), 4.0, Some(24.0));
        // Sale terms on the right edge, then the membership chip to their
        // left — a record already on a shelf changes what the price means.
        // The marker column starts where the painted text actually ends, not
        // at a fixed budget: "$375.00 · VG+/Not Graded" is wider than
        // "$23.00 · M/M" and used to run under the eye.
        let terms = ui.painter().text(
            egui::pos2(rect.right() - 10.0, rect.center().y),
            egui::Align2::RIGHT_CENTER,
            &price_line,
            crate::ui::tokens::font::body(),
            crate::ui::tokens::color::POSITIVE,
        );
        let mut right_edge = terms.left() - 10.0;
        for (show, list) in [
            (!owned && wanted, VinylList::Wantlist),
            (owned, VinylList::Collection),
        ] {
            if !show {
                continue;
            }
            let chip = crate::ui::icon::shelf_chip(
                ui.painter(),
                egui::pos2(right_edge - 20.0, rect.center().y - 8.0),
                list,
            );
            right_edge = chip.left() - 8.0;
        }
        // Viewed eye, left of the chip column: same audition marker the card
        // wears in its corner.
        if viewed {
            let c = egui::pos2(right_edge - 9.0, rect.center().y);
            crate::ui::icon::eye(ui.painter(), c, 5.5, egui::Color32::from_gray(150), true);
            right_edge = c.x - 9.0 - 8.0;
        }
        // Shops mark, next in the marker column: who else stocks it.
        let mut mark_clicked = false;
        if let Some((mark, clicked)) = self.shops_mark(
            ui,
            egui::Align2::RIGHT_CENTER,
            egui::pos2(right_edge, rect.center().y),
            idx,
            &mut act,
        ) {
            right_edge = mark.left() - 8.0;
            mark_clicked = clicked;
        }
        // Cart disc, left of the marker column: the same shortcut the card
        // carries, straight to the listing page whose Add to Cart button is
        // the real cart. Revealed on hover.
        const CART_D: f32 = 22.0;
        let cart_rect = egui::Rect::from_min_size(
            egui::pos2(right_edge - CART_D, rect.center().y - CART_D / 2.0),
            egui::vec2(CART_D, CART_D),
        );
        let cart_hit = ui.interact(
            cart_rect,
            ui.id().with(("seller-row-cart", listing_id)),
            egui::Sense::click(),
        );
        let cart_hovered = cart_hit.hovered();
        let mut cart_clicked = false;
        if resp.hovered() || cart_hovered {
            let bg = if cart_hovered {
                crate::ui::tokens::color::accent()
            } else {
                egui::Color32::from_black_alpha(190)
            };
            let fg = if cart_hovered {
                egui::Color32::from_gray(20)
            } else {
                egui::Color32::from_gray(240)
            };
            ui.painter()
                .circle_filled(cart_rect.center(), CART_D / 2.0, bg);
            crate::ui::icon::cart(ui.painter(), cart_rect.center(), 5.0, fg);
            let cart_hit = cart_hit.on_hover_cursor(egui::CursorIcon::PointingHand);
            if cart_hit
                .on_hover_note("Add to cart on discogs.com (opens the listing)")
                .clicked()
            {
                cart_clicked = true;
                act = Some(SellerAct::Buy(idx));
            }
        }
        right_edge = cart_rect.left() - 8.0;
        // Genre tags in the row's middle column, the sheet's "Electronic ·
        // House" line: the credit keeps the left part of the row, the tags
        // start at a fixed share of its width so they line up down the list.
        let mid = rect.left() + avail * 0.42;
        let mut text_right = right_edge - 6.0;
        if !genres.is_empty() {
            let ink = ui.visuals().weak_text_color();
            let g = crate::ui::text::truncated(
                ui,
                genres,
                crate::ui::tokens::font::body(),
                ink,
                right_edge - 6.0 - mid,
                false,
            );
            ui.painter()
                .galley(egui::pos2(mid, rect.center().y - g.size().y / 2.0), g, ink);
            text_right = mid - 16.0;
        }
        // Artist over title · year · format, truncated between the thumb and
        // the genre column and centred on the thumb.
        let artist_font = crate::ui::tokens::font::headline();
        let line2_font = crate::ui::tokens::font::body();
        let text_h = ui.fonts(|f| f.row_height(&artist_font) + 1.0 + f.row_height(&line2_font));
        let text_rect = egui::Rect::from_min_max(
            egui::pos2(thumb_rect.right() + 12.0, rect.center().y - text_h / 2.0),
            egui::pos2(text_right, rect.bottom()),
        );
        let mut text_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(text_rect)
                .layout(egui::Layout::top_down(egui::Align::Min))
                .id_salt(("seller-row-text", listing_id)),
        );
        text_ui.spacing_mut().item_spacing.y = 1.0;
        text_ui.add(
            egui::Label::new(egui::RichText::new(&artist).font(artist_font).strong()).truncate(),
        );
        let line2 = if sub.is_empty() {
            title.clone()
        } else {
            format!("{title} · {sub}")
        };
        text_ui.add(egui::Label::new(egui::RichText::new(line2).font(line2_font).weak()).truncate());

        let resp = resp.on_hover_note("Open the record to listen, want or buy");
        if resp.clicked() && !cart_clicked && !mark_clicked {
            act = Some(SellerAct::Open(idx));
        }
        crate::ui::menu::context(&resp, |ui| {
            crate::ui::menu::text(ui, egui::RichText::new(&title).strong());
            crate::ui::menu::text(ui, egui::RichText::new(&artist).weak());
            ui.separator();
            if ui
                .button("Add to cart on Discogs ↗")
                .on_hover_note("Opens the listing; cart and checkout live on discogs.com")
                .clicked()
            {
                act = Some(SellerAct::Buy(idx));
                ui.close_menu();
            }
            if ui
                .button("Dig from here")
                .on_hover_note("Walk Discogs outward from this record, by artist or label")
                .clicked()
            {
                act = Some(SellerAct::Dig(idx));
                ui.close_menu();
            }
            let already = owned || wanted;
            if ui
                .add_enabled(!already, egui::Button::new("Add to wantlist"))
                .clicked()
            {
                act = Some(SellerAct::Want(idx));
                ui.close_menu();
            }
            if ui.button("Open release page ↗").clicked() {
                open_url(&format!("https://www.discogs.com/release/{release_id}"));
                ui.close_menu();
            }
        });
        act
    }

    /// Open the record sheet for a seller listing, carrying the seller's
    /// concrete offer (price, grading, listing link) so the sheet shows what
    /// *this copy* costs alongside the market floor.
    fn open_seller_listing(&mut self, idx: usize, ctx: &egui::Context) {
        let Some(l) = self.seller_listings.get(idx).cloned() else {
            return;
        };
        // The all-shops scope names each row's shop; one shop's rows are its.
        let Some(seller) = self
            .seller_listing_shops
            .get(idx)
            .cloned()
            .or_else(|| self.seller_current.clone())
        else {
            return;
        };
        // Opening a record puts the keyboard ring on it, so the arrows walk on
        // from the record last clicked rather than from an earlier pick.
        self.seller_highlight = Some(l.listing_id);
        let sub = listing_sub(&l);
        self.open_release_sheet(
            l.release_id,
            l.artist.clone(),
            l.title.clone(),
            sub,
            l.thumb_url.clone(),
            ctx,
        );
        if let Some(sheet) = self.vinyl_sheet.as_mut() {
            if sheet.release_id == l.release_id {
                sheet.offer = Some(SellerOffer {
                    listing_id: l.listing_id,
                    seller,
                    price: discogs::MarketPrice {
                        value: l.price,
                        currency: l.currency.clone(),
                    },
                    condition: l.condition.clone(),
                    sleeve_condition: l.sleeve_condition.clone(),
                    shipping: l.shipping_price.map(|value| discogs::MarketPrice {
                        value,
                        currency: l
                            .shipping_currency
                            .clone()
                            .unwrap_or_else(|| l.currency.clone()),
                    }),
                    uri: l.uri.clone(),
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seller_input_parses_names_and_urls() {
        assert_eq!(parse_seller_input(" hardwax "), Some("hardwax".into()));
        assert_eq!(
            parse_seller_input("https://www.discogs.com/seller/juno_records/profile"),
            Some("juno_records".into())
        );
        assert_eq!(
            parse_seller_input("https://www.discogs.com/user/some-shop"),
            Some("some-shop".into())
        );
        assert_eq!(parse_seller_input("https://www.discogs.com/"), None);
        assert_eq!(parse_seller_input(""), None);
        assert_eq!(parse_seller_input("not a name"), None);
    }

    #[test]
    fn condition_grades_shorten() {
        assert_eq!(cond_short("Very Good Plus (VG+)"), "VG+");
        assert_eq!(cond_short("Mint (M)"), "M");
        assert_eq!(cond_short("Generic"), "Generic");
    }
}
