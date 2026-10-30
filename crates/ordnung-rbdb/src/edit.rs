//! Editing an existing rekordbox stick in place.
//!
//! The export command rebuilds a whole stick; this module instead applies one
//! user gesture to whatever export is already mounted. Playlist edits —
//! create, rename, delete a playlist, or add tracks to one — touch *only*
//! the playlist data; per-track edits ([`write_stick_cues`],
//! [`write_stick_beatgrid`]) touch only that track's ANLZ files (plus its
//! tempo in the databases for a grid change), the way rekordbox's own device
//! view saves a cue or a grid nudge straight onto the stick.
//!
//! * `export.pdb` — the PlaylistTree and PlaylistEntries tables are rewritten
//!   in place (surgically: every other table's pages are left byte-identical).
//!   Freed pages are blanked, extra pages are appended at the end of the file,
//!   and the table directory / page chains are relinked — see
//!   `docs/rekordbox-export-structure.md` for the bookkeeping being honored.
//! * `exportLibrary.db` — when the stick carries a Device Library Plus
//!   database (every rekordbox 6/7 export), its `playlist` /
//!   `playlist_content` tables are replaced to mirror the same tree, joining
//!   pdb track ids onto DLP content ids by file path (the only shared key).
//!
//! Deleting a track ([`delete_stick_tracks`]) is the one edit that touches
//! audio: the row loses its presence bit the way rekordbox's own device view
//! drops rows, both databases shed the track, and only then do its files go.
//! Everything else on the stick is never touched. Before the first edit ever
//! made to a stick (or to a track's analysis), the pristine files are copied
//! to `*.orig` alongside themselves, so the pre-Ordnung state is always
//! recoverable.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use ordnung_core::convert::ConvertSpec;
use ordnung_core::model::{AudioProperties, Beat, Cue, Format};

use crate::pdb::{
    rb_dir, u32_at, RbExport, RbPlaylist, RbTrack, ReadError, TYPE_PLAYLIST_ENTRIES,
    TYPE_PLAYLIST_TREE, TYPE_SUMMARY, TYPE_TRACKS,
};
use crate::pdbw;

/// One user-level playlist edit. Paths are relative to the volume root with
/// no leading slash (e.g. `Contents/Artist/track.mp3`), matched
/// case-insensitively against the export's stored paths (the volume is FAT32).
#[derive(Debug, Clone)]
pub enum PlaylistOp {
    /// Create an empty playlist under `parent_id` (`0` = top level).
    Create { name: String, parent_id: u32 },
    /// Rename a playlist or folder.
    Rename { id: u32, name: String },
    /// Delete a playlist, or a folder and everything under it.
    Delete { id: u32 },
    /// Append tracks (by path) to a playlist, skipping ones already in it and
    /// paths the export doesn't know.
    AddTracks { id: u32, rel_paths: Vec<String> },
    /// Take tracks (by path) out of a playlist; the rest keep their order.
    RemoveTracks { id: u32, rel_paths: Vec<String> },
}

/// What an edit produced: the id of a created playlist, and the stick's
/// post-edit state so the caller can refresh without re-reading the device.
#[derive(Debug)]
pub struct EditOutcome {
    /// The new playlist's id for `Create`; `None` for every other op.
    pub new_id: Option<u32>,
    /// The export as it now stands (playlists and entries reflect the edit;
    /// tracks are as read).
    pub export: RbExport,
}

/// Apply one playlist edit to the mounted stick at `volume_root` and write it
/// back to both databases. Returns the post-edit state.
pub fn edit_stick_playlists(
    volume_root: &Path,
    op: &PlaylistOp,
) -> Result<EditOutcome, ReadError> {
    let mut export = crate::pdb::read_stick(volume_root)?;
    let new_id = apply_op(&mut export, op)?;

    let dir = rb_dir(volume_root);
    let pdb_path = dir.join("export.pdb");
    let mut data = std::fs::read(&pdb_path).map_err(ReadError::io(&pdb_path))?;
    relink_playlist_tables(&mut data, &export.playlists, &export.entries)?;
    check_pdb(&pdb_path, &data)?;
    let dlp_path = dir.join("exportLibrary.db");
    if dlp_path.is_file() {
        sync_dlp_playlists(&dlp_path, &export)?;
    }
    // The pdb last, as the export places it: a player reads it first and
    // treats it as the stick's manifest.
    write_pdb(&pdb_path, &data)?;
    Ok(EditOutcome { new_id, export })
}

/// Copy `path` to `path.orig` the first time a stick is ever edited, so the
/// pre-Ordnung database can always be restored. Best-effort: a failed backup
/// (full stick) doesn't block the edit.
fn backup_once(path: &Path) {
    let orig = orig_path(path);
    if !orig.exists() {
        let _ = std::fs::copy(path, &orig);
    }
}

/// Judge an edited `export.pdb` before anything of the edit lands on the
/// stick: every editor calls this on its new bytes first, then writes the
/// Device Library Plus, then the pdb through [`write_pdb`], so a refusal
/// leaves both databases as they were instead of a DLP that is ahead of its
/// pdb. The lint judges Ord's writers, not rekordbox's files: a stick whose
/// current pdb already breaks a rule keeps its shape and stays editable;
/// only an edit that introduces the break is refused.
fn check_pdb(pdb_path: &Path, data: &[u8]) -> Result<(), ReadError> {
    if let Err(why) = crate::pdb::lint(data) {
        let live = std::fs::read(pdb_path).map_err(ReadError::io(pdb_path))?;
        if crate::pdb::lint(&live).is_ok() {
            return Err(ReadError::Lint(why));
        }
    }
    Ok(())
}

/// Every `export.pdb` rewrite lands through here: the pristine copy is kept
/// on the first edit, then the new bytes replace the file atomically, so a
/// stick yanked mid-write holds the old database or the new one, never a
/// torn page. Callers write the Device Library Plus first and this last.
fn write_pdb(pdb_path: &Path, data: &[u8]) -> Result<(), ReadError> {
    check_pdb(pdb_path, data)?;
    backup_once(pdb_path);
    crate::export::write_atomic(pdb_path, data).map_err(ReadError::io(pdb_path))
}

/// Mutate the in-memory export per `op`. Returns the created id for `Create`.
fn apply_op(export: &mut RbExport, op: &PlaylistOp) -> Result<Option<u32>, ReadError> {
    match op {
        PlaylistOp::Create { name, parent_id } => {
            let id = export.playlists.iter().map(|p| p.id).max().unwrap_or(0) + 1;
            let sort = export
                .playlists
                .iter()
                .map(|p| p.sort_order)
                .max()
                .unwrap_or(0)
                + 1;
            export.playlists.push(RbPlaylist {
                id,
                parent_id: *parent_id,
                sort_order: sort,
                is_folder: false,
                name: name.clone(),
            });
            export.entries.insert(id, Vec::new());
            Ok(Some(id))
        }
        PlaylistOp::Rename { id, name } => {
            let Some(p) = export.playlists.iter_mut().find(|p| p.id == *id) else {
                return Err(ReadError::Format("no such playlist"));
            };
            p.name = name.clone();
            Ok(None)
        }
        PlaylistOp::Delete { id } => {
            // A folder takes its whole subtree with it, exactly like the
            // desktop app: collect ids transitively, then drop nodes+entries.
            let mut doomed = vec![*id];
            let mut i = 0;
            while i < doomed.len() {
                let parent = doomed[i];
                doomed.extend(
                    export
                        .playlists
                        .iter()
                        .filter(|p| p.parent_id == parent && p.id != parent)
                        .map(|p| p.id),
                );
                i += 1;
            }
            export.playlists.retain(|p| !doomed.contains(&p.id));
            for d in &doomed {
                export.entries.remove(d);
            }
            Ok(None)
        }
        PlaylistOp::AddTracks { id, rel_paths } => {
            if !export.playlists.iter().any(|p| p.id == *id && !p.is_folder) {
                return Err(ReadError::Format("no such playlist"));
            }
            let ids = ids_by_path(export, rel_paths);
            let list = export.entries.entry(*id).or_default();
            for tid in ids {
                if !list.contains(&tid) {
                    list.push(tid);
                }
            }
            Ok(None)
        }
        PlaylistOp::RemoveTracks { id, rel_paths } => {
            let ids = ids_by_path(export, rel_paths);
            if let Some(list) = export.entries.get_mut(id) {
                list.retain(|tid| !ids.contains(tid));
            }
            Ok(None)
        }
    }
}

/// The export's track ids behind `rel_paths` (relative to the volume root,
/// matched through `path_key`: case-insensitively, the volume is FAT32, and
/// in NFC, macOS hands FAT names back decomposed), in the paths' order;
/// paths the export doesn't know are skipped.
fn ids_by_path(export: &RbExport, rel_paths: &[String]) -> Vec<u32> {
    let by_path: HashMap<String, u32> = export
        .tracks
        .iter()
        .map(|(tid, t)| (key(&t.file_path), *tid))
        .collect();
    rel_paths
        .iter()
        .filter_map(|rel| by_path.get(&key(rel)).copied())
        .collect()
}

// ---------------------------------------------------------------------------
// Deleting tracks from the stick
// ---------------------------------------------------------------------------

/// Delete tracks (by path relative to the volume root) from the mounted
/// stick at `volume_root`, the way rekordbox's own device view does: each
/// track's `export.pdb` row keeps its slot but loses its presence bit (its
/// page is flagged 0x34 and listed in the table's index page), its playlist
/// entries leave both databases, its Device Library Plus rows and My Tag
/// links go, and only then are its audio, ANLZ files and — when no other
/// track shares it — artwork removed from the device. Paths the export
/// doesn't know are skipped. Both databases are prepared before either is
/// written, so a refusal leaves the stick as it was. Returns how many tracks
/// were deleted. The pristine databases are kept as `*.orig` siblings from
/// the first edit on.
pub fn delete_stick_tracks(volume_root: &Path, rel_paths: &[String]) -> Result<usize, ReadError> {
    let mut export = crate::pdb::read_stick(volume_root)?;
    let mut doomed = ids_by_path(&export, rel_paths);
    doomed.sort_unstable();
    doomed.dedup();
    if doomed.is_empty() {
        return Ok(0);
    }
    let gone: Vec<RbTrack> = doomed.iter().filter_map(|id| export.tracks.remove(id)).collect();
    for list in export.entries.values_mut() {
        list.retain(|tid| !doomed.contains(tid));
    }

    let dir = rb_dir(volume_root);
    let pdb_path = dir.join("export.pdb");
    let mut data = std::fs::read(&pdb_path).map_err(ReadError::io(&pdb_path))?;
    relink_playlist_tables(&mut data, &export.playlists, &export.entries)?;
    let dropped = clear_rows(&mut data, TYPE_TRACKS, |data, row| {
        u32_at(data, row + 0x48).is_some_and(|id| doomed.contains(&id))
    })?;
    if dropped != doomed.len() {
        return Err(ReadError::Format("track row missing from export.pdb"));
    }
    set_summary_count(&mut data, export.tracks.len() as u32);
    check_pdb(&pdb_path, &data)?;

    let dlp_path = dir.join("exportLibrary.db");
    if dlp_path.is_file() {
        let keys: Vec<String> = gone
            .iter()
            .map(|t| key(&t.file_path))
            .collect();
        with_local_dlp(&dlp_path, |conn| {
            let mut stmt = conn.prepare("SELECT content_id, path FROM content")?;
            let ids: Vec<i64> = stmt
                .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?
                .flatten()
                .filter(|(_, p)| keys.contains(&key(p)))
                .map(|(id, _)| id)
                .collect();
            for id in ids {
                conn.execute(
                    "DELETE FROM hotCueBankList_cue WHERE cue_id IN \
                     (SELECT cue_id FROM cue WHERE content_id = ?1)",
                    [id],
                )?;
                for table in ["cue", "history_content", "myTag_content", "playlist_content", "content"] {
                    conn.execute(&format!("DELETE FROM {table} WHERE content_id = ?1"), [id])?;
                }
                conn.execute(
                    "DELETE FROM recommendedLike WHERE content_id_1 = ?1 OR content_id_2 = ?1",
                    [id],
                )?;
            }
            conn.execute(
                "UPDATE property SET numberOfContents = (SELECT COUNT(*) FROM content)",
                [],
            )?;
            mirror_playlists(conn, &export)
        })?;
    }
    write_pdb(&pdb_path, &data)?;

    // My Tag links in exportExt.pdb — type 4 rows, `u32 0, u32 track id,
    // u32 tag id`. A stick without the file, or one whose tag database
    // doesn't parse, simply keeps none.
    let ext_path = dir.join("exportExt.pdb");
    if let Ok(mut ext) = std::fs::read(&ext_path) {
        let cleared = clear_rows(&mut ext, 4, |data, row| {
            u32_at(data, row + 4).is_some_and(|id| doomed.contains(&id))
        });
        if cleared.is_ok_and(|n| n > 0) {
            backup_once(&ext_path);
            crate::export::write_atomic(&ext_path, &ext).map_err(ReadError::io(&ext_path))?;
        }
    }

    // Files last: a row that outlives its file is a missing track on the
    // player; a file that outlives its row is only a stray on the stick.
    let still_used: HashSet<u32> = export.tracks.values().map(|t| t.artwork_id).collect();
    for t in &gone {
        crate::export::remove_track_files(volume_root, t, &still_used);
    }
    Ok(gone.len())
}

/// Drop every row of table `ty` that `doomed` picks, rekordbox's own way: the
/// row keeps its slot (surviving rows' slot numbers, which their index_shift
/// encodes, never move) and loses its presence bit; the page's present-row
/// counter at 0x19 follows, its flags gain the has-deletes bit (0x24 → 0x34),
/// its transaction id becomes the header's sequence, and the table's index
/// page lists it as a collection candidate — the state a rekordbox export is
/// in after its own in-place deletes. Returns how many rows were dropped.
fn clear_rows(
    data: &mut [u8],
    ty: u32,
    doomed: impl Fn(&[u8], usize) -> bool,
) -> Result<usize, ReadError> {
    use crate::pdb::{table_dir, table_pages, u16_at};
    let tables = table_dir(data)?;
    let tx = u32_at(data, 0x14).unwrap_or(0);
    let page_size = tables.page_size;
    if page_size != pdbw::PAGE {
        return Err(ReadError::Format("unsupported page size"));
    }
    let Some((_, first, last)) = tables.find(ty) else {
        return Ok(0);
    };
    let pages = table_pages(data, page_size, first, last);
    let sentinel = pages.first().copied().filter(|&off| data[off + 0x1B] & 0x40 != 0);
    let mut dropped = 0;
    for page_off in pages {
        if u32_at(data, page_off + 8) != Some(ty) || data[page_off + 0x1B] & 0x40 != 0 {
            continue;
        }
        let mut hit = false;
        for (slot, (present, row)) in page_slots(data, page_off).into_iter().enumerate() {
            if !present || !doomed(data, row) {
                continue;
            }
            let end = page_off + pdbw::PAGE - 36 * (slot / 16);
            let bits = u16_at(data, end - 4).unwrap_or(0) & !(1u16 << (slot % 16));
            data[end - 4..end - 2].copy_from_slice(&bits.to_le_bytes());
            let live = u16_at(data, page_off + 0x19).unwrap_or(0).saturating_sub(32);
            data[page_off + 0x19..page_off + 0x1B].copy_from_slice(&live.to_le_bytes());
            dropped += 1;
            hit = true;
        }
        if hit {
            data[page_off + 0x1B] |= 0x10;
            data[page_off + 0x10..page_off + 0x14].copy_from_slice(&tx.to_le_bytes());
            if let Some(s) = sentinel {
                index_list_page(data, s, (page_off / page_size) as u32);
            }
        }
    }
    Ok(dropped)
}

/// List `page` in a table's index (sentinel) page as one holding deleted
/// rows, as rekordbox does: into the first empty slot below the high-water
/// mark (the word at 0x3A, 0x1FFF when there is none), else into a new slot
/// at the mark (0x26); the entry count at 0x38 follows. A page already
/// listed, or an index with no room, is left as it is — readers resolve
/// rows through the presence bits, not this list.
fn index_list_page(data: &mut [u8], sentinel: usize, page: u32) {
    use crate::pdb::u16_at;
    const EMPTY: u32 = 0x1FFF_FFF8;
    let entry = page * 8;
    let mark = u16_at(data, sentinel + 0x26).unwrap_or(0) as usize;
    let at = |i: usize| sentinel + 0x3C + 4 * i;
    if (0..mark).any(|i| u32_at(data, at(i)) == Some(entry)) {
        return;
    }
    let first_empty = u16_at(data, sentinel + 0x3A).unwrap_or(0x1FFF) as usize;
    let slot = if first_empty < mark { first_empty } else { mark };
    if at(slot) + 4 > sentinel + pdbw::PAGE - 20 {
        return;
    }
    data[at(slot)..at(slot) + 4].copy_from_slice(&entry.to_le_bytes());
    let mark = mark.max(slot + 1);
    data[sentinel + 0x26..sentinel + 0x28].copy_from_slice(&(mark as u16).to_le_bytes());
    let count = u16_at(data, sentinel + 0x38).unwrap_or(0) + 1;
    data[sentinel + 0x38..sentinel + 0x3A].copy_from_slice(&count.to_le_bytes());
    let next_empty = (0..mark)
        .find(|&i| u32_at(data, at(i)) == Some(EMPTY))
        .map_or(0x1FFFu16, |i| i as u16);
    data[sentinel + 0x3A..sentinel + 0x3C].copy_from_slice(&next_empty.to_le_bytes());
}

/// Patch the track count in the type-19 export-summary row (`u16 0x0280,
/// u16 index_shift, u32 numberOfContents`, mirroring the DLP's
/// `property.numberOfContents`). A database without the row is left alone.
fn set_summary_count(data: &mut [u8], count: u32) {
    use crate::pdb::{page_rows, table_dir, table_pages, u16_at};
    let Ok(tables) = table_dir(data) else {
        return;
    };
    let Some((_, first, last)) = tables.find(TYPE_SUMMARY) else {
        return;
    };
    for page_off in table_pages(data, tables.page_size, first, last) {
        for row in page_rows(data, tables.page_size, page_off, TYPE_SUMMARY) {
            if u16_at(data, row) == Some(0x0280) {
                data[row + 4..row + 8].copy_from_slice(&count.to_le_bytes());
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Per-track edits — cues and beatgrid, straight onto the stick
// ---------------------------------------------------------------------------

/// Write a track's cue set onto the stick it lives on, the way rekordbox's
/// own device view does: its `ANLZ0000.DAT` / `.EXT` cue lists are replaced
/// in place, and the track's cue update counter is bumped in `export.pdb`
/// and the Device Library Plus — the signal rekordbox uses to notice that a
/// device carries newer cues than its collection ("Update Collection").
/// `track_id` is the pdb track id; `dat_path` the absolute path of the
/// track's `ANLZ0000.DAT` on the mounted volume. The pristine files are
/// kept as `*.orig` siblings from the first edit on.
pub fn write_stick_cues(
    volume_root: &Path,
    track_id: u32,
    dat_path: &Path,
    cues: &[Cue],
) -> Result<(), ReadError> {
    backup_anlz(dat_path);
    crate::anlz::write_cues(dat_path, cues).map_err(ReadError::io(dat_path))?;
    bump_track_counter(volume_root, track_id, CUE_COUNTER, None)
}

/// The per-track change counters a rekordbox export carries in both
/// databases, as `(pdb track-row string slot, DLP content column)`. rekordbox
/// and the players compare them against the collection to find tracks edited
/// on the device.
const ANALYSIS_COUNTER: (usize, &str) = (3, "analysisDataUpdateCount");
const CUE_COUNTER: (usize, &str) = (4, "cueUpdateCount");
/// String slot 2 of a track row, `informationUpdateCount`: stepped when the
/// track's data (here its file) changes.
const INFO_COUNTER_SLOT: usize = 2;

/// Bump one change counter of a track in both stick databases, optionally
/// patching the pdb row's tempo in the same rewrite. The pdb row is
/// re-encoded (a longer counter string can grow it) and its page rebuilt in
/// place; the DLP row follows. Backups are taken before the first edit.
fn bump_track_counter(
    volume_root: &Path,
    track_id: u32,
    (slot, column): (usize, &str),
    centi_bpm: Option<u32>,
) -> Result<(), ReadError> {
    let dir = rb_dir(volume_root);
    let pdb_path = dir.join("export.pdb");
    let (data, file_path) = edit_track_row(&pdb_path, track_id, |row| {
        if let Some(centi) = centi_bpm {
            row.tempo_centi_bpm = centi;
        }
        let n: u32 = row.strings[slot].trim().parse().unwrap_or(0);
        row.strings[slot] = (n + 1).to_string();
    })?;
    check_pdb(&pdb_path, &data)?;
    let dlp_path = dir.join("exportLibrary.db");
    if dlp_path.is_file() {
        with_local_dlp(&dlp_path, |conn| {
            let wanted = key(&file_path);
            let mut stmt = conn.prepare("SELECT content_id, path FROM content")?;
            let ids: Vec<i64> = stmt
                .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?
                .flatten()
                .filter(|(_, p)| key(p) == wanted)
                .map(|(id, _)| id)
                .collect();
            for id in ids {
                if let Some(centi) = centi_bpm {
                    conn.execute(
                        "UPDATE content SET bpmx100 = ?1 WHERE content_id = ?2",
                        rusqlite::params![centi as i64, id],
                    )?;
                }
                conn.execute(
                    &format!(
                        "UPDATE content SET {c} = COALESCE({c}, 0) + 1 WHERE content_id = ?1",
                        c = column
                    ),
                    rusqlite::params![id],
                )?;
            }
            Ok(())
        })?;
    }
    write_pdb(&pdb_path, &data)
}

/// Write a track's beatgrid onto the stick it lives on: the ANLZ `PQTZ`
/// (and the `.EXT`'s extended grid, emptied) are replaced in place, the
/// tempo of the track's `export.pdb` row is patched to the grid's BPM, and
/// the Device Library Plus row follows suit. `track_id` is the pdb track id.
/// The pristine analysis files and databases are kept as `*.orig` siblings
/// from the first edit on.
pub fn write_stick_beatgrid(
    volume_root: &Path,
    track_id: u32,
    dat_path: &Path,
    beats: &[Beat],
) -> Result<(), ReadError> {
    backup_anlz(dat_path);
    crate::anlz::write_beatgrid(dat_path, beats).map_err(ReadError::io(dat_path))?;
    let centi = beats
        .first()
        .map(|b| b.bpm)
        .filter(|b| *b > 0.0)
        .map(|bpm| (bpm * 100.0).round().clamp(0.0, u32::MAX as f32) as u32);
    bump_track_counter(volume_root, track_id, ANALYSIS_COUNTER, centi)
}

// ---------------------------------------------------------------------------
// Converting a track's audio in place
// ---------------------------------------------------------------------------

/// Convert one of a stick's tracks in place, for a player that can't play
/// the file as it is: `spec` is encoded (at `rate` Hz, `None` keeping the
/// source's) into a sibling file under the track's own name and the new
/// extension, the row, the Device Library Plus and the ANLZ files are
/// pointed at it ([`relink_converted_track`]), and only then is the
/// original removed — so the stick never holds a track without a playable
/// file, and cues, grids, waveforms, artwork and playlist entries stay with
/// it. `track_id` is the pdb track id. Refuses when a file of the converted
/// name is already on the stick. Returns the converted file's path.
pub fn convert_stick_track(
    volume_root: &Path,
    track_id: u32,
    spec: &ConvertSpec,
    rate: Option<u32>,
) -> Result<PathBuf, ReadError> {
    let pdb_path = rb_dir(volume_root).join("export.pdb");
    let export = crate::pdb::read_export(&pdb_path)?;
    let track = export
        .tracks
        .get(&track_id)
        .ok_or(ReadError::Format("no such track row"))?;
    let src = volume_root.join(track.file_path.trim_start_matches('/'));
    let convert_err = |msg: String| ReadError::Convert {
        path: src.clone(),
        msg,
    };
    let stem = src
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = ordnung_core::convert::target_extension(spec.target);
    let dest = src.with_file_name(format!("{stem}.{ext}"));
    // A row that names the converted path is another track's audio, not
    // ours to replace. A file of that name no row names is a conversion that
    // failed before its row was repointed (kept, see below): redone.
    let dest_key = crate::export::path_key(&format!(
        "/{}",
        dest.strip_prefix(volume_root).unwrap_or(&dest).to_string_lossy()
    ));
    if dest == src || export.tracks.values().any(|t| crate::export::path_key(&t.file_path) == dest_key) {
        return Err(convert_err(
            "a file of the converted name is already on the device".into(),
        ));
    }
    let _ = std::fs::remove_file(&dest);
    // A 16-bit stereo AIFF is two to four times its FLAC; a stick that
    // cannot hold it is refused before the encode, not found out mid-way
    // with the databases next in line.
    let out_rate = u64::from(rate.unwrap_or(track.sample_rate_hz).max(44_100));
    let need = u64::from(track.duration_s) * out_rate * 4 + (1 << 20);
    if let Some(free) = crate::export::free_space(volume_root) {
        if free < need {
            return Err(convert_err(format!(
                "not enough space on the device: the converted file needs about {} MB, {} MB free",
                need >> 20,
                free >> 20
            )));
        }
    }
    // The source's own tags and cover ride along: ffmpeg carries only some
    // of them into an AIFF, and while the player reads the row, the file
    // should stay a complete file.
    let tags = ordnung_core::scan::scan_file(&src).map(|s| s.tags).unwrap_or_default();
    let cover = ordnung_core::tag::read_front_cover_raw(&src).ok().flatten();
    // Encode to a sibling temp name (real extension last so ffmpeg picks
    // the muxer), then move into place: an interrupted conversion never
    // leaves a half file under the track's name.
    let tmp = src.with_file_name(format!("{stem}.ordnung-tmp.{ext}"));
    let _ = std::fs::remove_file(&tmp);
    let placed = ordnung_core::convert::convert_file_resampled(&src, spec, &tmp, false, rate)
        .map_err(|e| convert_err(e.to_string()))
        .and_then(|_| {
            // Best effort: a file whose tags can't be written still plays.
            let _ = ordnung_core::tag::embed_full(&tmp, &tags, cover.as_ref());
            crate::export::sync_existing(&tmp).map_err(ReadError::io(&tmp))
        })
        .and_then(|_| std::fs::rename(&tmp, &dest).map_err(ReadError::io(&dest)));
    if let Err(e) = placed {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    let relinked = (|| {
        let props = ordnung_core::scan::scan_file(&dest)
            .map_err(|e| convert_err(e.to_string()))?
            .properties;
        let size = std::fs::metadata(&dest).map(|m| m.len()).unwrap_or(0);
        let new_rel = dest
            .strip_prefix(volume_root)
            .map_err(|_| ReadError::Format("converted file left the volume"))?
            .to_string_lossy()
            .into_owned();
        relink_converted_track(volume_root, track_id, &new_rel, spec.target, &props, size)
    })();
    // A relink that fails part-way may already have pointed the Device
    // Library Plus at the converted file, so it stays beside the original: a
    // stray on the stick beats a row whose audio is gone. The next attempt
    // starts over from it.
    relinked?;
    // Only now is the original expendable.
    let _ = std::fs::remove_file(&src);
    Ok(dest)
}

/// Point a stick's track at the file it was converted into, in place: the
/// `export.pdb` row takes the new path, name, type, size, rate, depth and
/// bitrate (and its information counter steps, as rekordbox's does when a
/// track's data changes), the Device Library Plus row follows, and the
/// track's ANLZ files are re-pathed — so cues, grids, waveforms, artwork
/// and every playlist entry stay with the track. `track_id` is the pdb
/// track id; `new_rel` the converted file's path relative to the volume
/// root. The caller owns the audio: it converts first and removes the old
/// file only after this returns. The pristine databases and analysis files
/// are kept as `*.orig` siblings from the first edit on.
pub fn relink_converted_track(
    volume_root: &Path,
    track_id: u32,
    new_rel: &str,
    format: Format,
    props: &AudioProperties,
    file_size: u64,
) -> Result<(), ReadError> {
    let new_path = format!("/{}", new_rel.trim_start_matches('/'));
    let new_name = new_path.rsplit('/').next().unwrap_or_default().to_string();
    let depth = u16::from(props.bit_depth.unwrap_or(16));
    // The bitrate a player shows: the stream's own for lossy files, the
    // PCM rate for the rest, as the export writes it.
    let bitrate = props
        .bitrate_kbps
        .unwrap_or(props.sample_rate_hz * u32::from(depth) * 2 / 1000);
    let dir = rb_dir(volume_root);
    let pdb_path = dir.join("export.pdb");
    let mut anlz = String::new();
    let (data, old_path) = edit_track_row(&pdb_path, track_id, |row| {
        anlz = row.strings[14].clone();
        row.header[0x08..0x0C].copy_from_slice(&props.sample_rate_hz.to_le_bytes());
        let size = file_size.min(u32::MAX as u64) as u32;
        row.header[0x10..0x14].copy_from_slice(&size.to_le_bytes());
        row.header[0x30..0x34].copy_from_slice(&bitrate.to_le_bytes());
        row.header[0x52..0x54].copy_from_slice(&depth.to_le_bytes());
        row.header[0x5A..0x5C].copy_from_slice(&crate::export::file_type(format).to_le_bytes());
        let n: u32 = row.strings[INFO_COUNTER_SLOT].trim().parse().unwrap_or(0);
        row.strings[INFO_COUNTER_SLOT] = (n + 1).to_string();
        row.strings[19] = new_name.clone();
        row.strings[20] = new_path.clone();
    })?;
    check_pdb(&pdb_path, &data)?;
    let dlp_path = dir.join("exportLibrary.db");
    if dlp_path.is_file() {
        with_local_dlp(&dlp_path, |conn| {
            let wanted = key(&old_path);
            let mut stmt = conn.prepare("SELECT content_id, path FROM content")?;
            let ids: Vec<i64> = stmt
                .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?
                .flatten()
                .filter(|(_, p)| key(p) == wanted)
                .map(|(id, _)| id)
                .collect();
            for id in ids {
                conn.execute(
                    "UPDATE content SET path = ?1, fileName = ?2, fileSize = ?3, fileType = ?4, \
                     bitrate = ?5, bitDepth = ?6, samplingRate = ?7, \
                     informationUpdateCount = COALESCE(informationUpdateCount, 0) + 1 \
                     WHERE content_id = ?8",
                    rusqlite::params![
                        new_path,
                        new_name,
                        file_size as i64,
                        i64::from(crate::export::file_type(format)),
                        i64::from(bitrate),
                        i64::from(depth),
                        i64::from(props.sample_rate_hz),
                        id,
                    ],
                )?;
            }
            Ok(())
        })?;
    }
    // The analysis files name the audio they describe.
    if !anlz.is_empty() {
        let dat = volume_root.join(anlz.trim_start_matches('/'));
        if dat.is_file() {
            backup_anlz(&dat);
            crate::anlz::write_path(&dat, &new_path).map_err(ReadError::io(&dat))?;
        }
    }
    write_pdb(&pdb_path, &data)
}

/// The beatgrid a track had before Ordnung first edited it on this stick —
/// read from the `ANLZ0000.DAT.orig` backup. `None` when the track was
/// never edited here (no backup) or the backup carries no grid.
pub fn original_stick_beatgrid(dat_path: &Path) -> Option<Vec<Beat>> {
    let orig = orig_path(dat_path);
    let beats = crate::anlz::read_beatgrid(&orig);
    (!beats.is_empty()).then_some(beats)
}

fn orig_path(path: &Path) -> std::path::PathBuf {
    crate::export::sibling(path, ".orig")
}

/// Put the export databases back as they were before the last export, from
/// the `.prev` siblings the export keeps: the way back when a player or
/// rekordbox rejects what that export wrote. Files the export added stay on
/// the stick; a row for a file deleted since comes back as a missing track.
/// Placed in the export's order, pdb last. Returns how many were restored.
pub fn restore_previous(volume_root: &Path) -> Result<usize, ReadError> {
    let dir = rb_dir(volume_root);
    let mut restored = 0;
    for name in crate::export::DB_NAMES {
        let live = dir.join(name);
        let prev = crate::export::sibling(&live, ".prev");
        if !prev.is_file() {
            continue;
        }
        crate::dlp::place_library(&prev, &live).map_err(ReadError::io(&live))?;
        restored += 1;
    }
    crate::export::sync_dir(&dir);
    Ok(restored)
}

/// Whether the stick at `volume_root` holds a previous library to go back to.
pub fn has_previous(volume_root: &Path) -> bool {
    crate::export::sibling(&rb_dir(volume_root).join("export.pdb"), ".prev").is_file()
}

/// Keep the pristine `.DAT` and `.EXT` beside themselves before the first
/// edit to a track's analysis (the `.2EX` carries no cues or grid).
fn backup_anlz(dat_path: &Path) {
    backup_once(dat_path);
    let ext = dat_path.with_extension("EXT");
    if ext.is_file() {
        backup_once(&ext);
    }
}

// The row codec (`TrackRowEdit`, `decode_track_row`, `encode_track_row`)
// lives in `pdbw`, where the export's merge carries stick rows through it too.
use crate::pdbw::{decode_track_row, encode_track_row, TrackRowEdit};

/// The form a stick path is matched in, with or without its leading slash:
/// the export's `path_key` (NFC, lowercase).
fn key(path: &str) -> String {
    crate::export::path_key(path.trim_start_matches('/'))
}

/// Every slot of a data page in order: whether it is live, and its row's
/// absolute offset. Deleted slots (pages flagged 0x34) keep their place so
/// the surviving rows' slot numbers — which their `index_shift` encodes —
/// never move.
fn page_slots(data: &[u8], page_off: usize) -> Vec<(bool, usize)> {
    use crate::pdb::{u16_at, PAGE_HEADER};
    let slots = data.get(page_off + 0x18).copied().unwrap_or(0) as usize;
    let mut out = Vec::with_capacity(slots);
    for slot in 0..slots {
        let g = slot / 16;
        let end = page_off + pdbw::PAGE - 36 * g;
        let present = u16_at(data, end - 4).unwrap_or(0) & (1 << (slot % 16)) != 0;
        let rel = u16_at(data, end - 6 - 2 * (slot % 16)).unwrap_or(0) as usize;
        out.push((present, page_off + PAGE_HEADER + rel));
    }
    out
}

/// Re-encode one track row of the DeviceSQL database: decode it, let `edit`
/// change it, and rebuild just its page in a copy of the file (every other
/// page is untouched, byte for byte). The page keeps its header words
/// (index, chain link, transaction id, slot count, flags, batch words) and
/// its presence bitmasks — including deleted slots — and only lays its heap
/// and offset words out afresh. Refuses a page the grown row no longer fits.
/// Returns the new file bytes, for [`write_pdb`] once the Device Library
/// Plus has followed, and the row's file path as it was before the edit
/// (the key the DLP shares).
fn edit_track_row(
    pdb_path: &Path,
    track_id: u32,
    edit: impl FnOnce(&mut TrackRowEdit),
) -> Result<(Vec<u8>, String), ReadError> {
    use crate::pdb::{table_dir, table_pages, u32_at, PAGE_HEADER, TYPE_TRACKS};
    let mut data = std::fs::read(pdb_path).map_err(ReadError::io(pdb_path))?;
    let tables = table_dir(&data)?;
    let page_size = tables.page_size;
    if page_size != pdbw::PAGE {
        return Err(ReadError::Format("unsupported page size"));
    }
    let mut hit: Option<(usize, usize)> = None; // (page_off, row)
    if let Some((_, first, last)) = tables.find(TYPE_TRACKS) {
        for page_off in table_pages(&data, page_size, first, last) {
            if u32_at(&data, page_off + 8) != Some(TYPE_TRACKS) || data[page_off + 0x1B] & 0x40 != 0 {
                continue;
            }
            for (present, row) in page_slots(&data, page_off) {
                if present && u32_at(&data, row + 0x48) == Some(track_id) {
                    hit = Some((page_off, row));
                }
            }
        }
    }
    let Some((page_off, target)) = hit else {
        return Err(ReadError::Format("no such track row"));
    };

    // Re-encode the heap: live rows in slot order (the edited one rebuilt,
    // the rest carried verbatim, padding trimmed to the writer's 4-byte
    // grid); deleted slots contribute no bytes.
    let slots = page_slots(&data, page_off);
    let mut heap: Vec<u8> = Vec::new();
    let mut offsets: Vec<u16> = Vec::with_capacity(slots.len());
    let mut file_path = String::new();
    let mut edit = Some(edit);
    for &(present, row) in &slots {
        offsets.push(heap.len() as u16);
        if !present {
            continue;
        }
        let (mut r, end) =
            decode_track_row(&data, row).ok_or(ReadError::Format("malformed track row"))?;
        if row == target {
            file_path = r.strings[20].clone();
            if let Some(edit) = edit.take() {
                edit(&mut r);
            }
            heap.extend_from_slice(&encode_track_row(&r));
        } else {
            heap.extend_from_slice(&pdbw::pad4(data[row..row + end].to_vec()));
        }
    }
    let groups = slots.len().div_ceil(16);
    let index_bytes = pdbw::index_bytes(slots.len());
    let Some(free) = (pdbw::PAGE - PAGE_HEADER)
        .checked_sub(heap.len())
        .and_then(|v| v.checked_sub(index_bytes))
    else {
        return Err(ReadError::Format("edited track row no longer fits its page"));
    };

    let mut page = vec![0u8; pdbw::PAGE];
    page[..PAGE_HEADER].copy_from_slice(&data[page_off..page_off + PAGE_HEADER]);
    page[0x1C..0x1E].copy_from_slice(&(free as u16).to_le_bytes());
    page[0x1E..0x20].copy_from_slice(&(heap.len() as u16).to_le_bytes());
    page[PAGE_HEADER..PAGE_HEADER + heap.len()].copy_from_slice(&heap);
    for g in 0..groups {
        let end = pdbw::PAGE - 36 * g;
        let src = page_off + end;
        // Presence and last-batch bitmasks exactly as they were.
        page[end - 4..end].copy_from_slice(&data[src - 4..src]);
        for r in 0..(slots.len() - g * 16).min(16) {
            let o = end - 6 - 2 * r;
            page[o..o + 2].copy_from_slice(&offsets[g * 16 + r].to_le_bytes());
        }
    }
    data[page_off..page_off + page_size].copy_from_slice(&page);
    Ok((data, file_path))
}

/// Run `f` against a local copy of the stick's Device Library Plus database
/// and place the result back whole — SQLite cannot write in place on macOS's
/// msdos (FAT32) driver (see [`crate::dlp::write_library`]). Every DLP edit
/// lands through here; the pristine copy is kept on the first one.
fn with_local_dlp(
    db_path: &Path,
    f: impl FnOnce(&rusqlite::Connection) -> rusqlite::Result<()>,
) -> Result<(), ReadError> {
    backup_once(db_path);
    let err_io = |e: std::io::Error| ReadError::Dlp(e.to_string());
    let tmp = crate::dlp::scratch_db_path("ordnung-dlp-edit");
    crate::dlp::scratch_copy(db_path, &tmp).map_err(err_io)?;
    let result = (|| {
        let conn = crate::dlp::open(&tmp, rusqlite::OpenFlags::default())?;
        conn.execute_batch("BEGIN")?;
        f(&conn)?;
        conn.execute_batch("COMMIT")?;
        crate::dlp::checkpoint(&conn)
    })();
    if result.is_ok() {
        crate::dlp::place_library(&tmp, db_path).map_err(err_io)?;
    }
    crate::dlp::remove_scratch(&tmp);
    result
}

// ---------------------------------------------------------------------------
// export.pdb surgery
// ---------------------------------------------------------------------------


/// Replace exactly the PlaylistTree and PlaylistEntries tables of the
/// DeviceSQL database in `data` (the whole file), leaving every other byte
/// of every other table untouched. The new row set may need more pages than
/// the old one, in which case fresh pages are appended at the end; pages
/// freed by shrinkage are blanked so no reader can pick up stale rows. The
/// header comes back bumped, ready for [`write_pdb`].
fn relink_playlist_tables(
    data: &mut Vec<u8>,
    playlists: &[RbPlaylist],
    entries: &HashMap<u32, Vec<u32>>,
) -> Result<(), ReadError> {
    let tables = crate::pdb::table_dir(data)?;
    let page_size = tables.page_size;
    if page_size != pdbw::PAGE {
        // Never seen in the wild (rekordbox always writes 4096); refuse
        // rather than build pages of the wrong size.
        return Err(ReadError::Format("unsupported page size"));
    }

    // Encode the replacement rows. Entries are grouped per playlist in tree
    // order with 0-based indices — the shape rekordbox writes and the shape
    // the full-export writer produces.
    let tree_rows: Vec<Vec<u8>> = playlists
        .iter()
        .map(|p| pdbw::pad4(pdbw::playlist_tree_row(p)))
        .collect();
    let mut entry_rows: Vec<Vec<u8>> = Vec::new();
    for p in playlists.iter().filter(|p| !p.is_folder) {
        if let Some(tracks) = entries.get(&p.id) {
            for (i, tid) in tracks.iter().enumerate() {
                entry_rows.push(pdbw::pad4(pdbw::playlist_entry_row(i as u32, *tid, p.id)));
            }
        }
    }

    // Fresh pages go at the end of the file; the header's next_unused_page
    // may carry slack beyond the file, so allocate from whichever is larger.
    let mut next_free = (data.len() / page_size) as u32;
    next_free = next_free.max(u32_at(&data, 0x0C).unwrap_or(0));

    let mut plan = Vec::new(); // (dir slot, type, first, last, chunks)
    for (ty, rows) in [
        (TYPE_PLAYLIST_TREE, &tree_rows),
        (TYPE_PLAYLIST_ENTRIES, &entry_rows),
    ] {
        let (slot, first, last) = tables
            .find(ty)
            .ok_or(ReadError::Format("playlist table missing from directory"))?;
        plan.push((slot, ty, first, last, pdbw::paginate(rows)));
    }

    // First pass: settle page assignments for both tables so every chain's
    // terminator can point one past the final file size.
    struct TablePlan {
        slot: usize,
        ty: u32,
        keep_first: Option<u32>, // the sentinel page, kept in place
        reusable: Vec<u32>,      // old data pages, rewritten or blanked
        chunks: Vec<Vec<Vec<u8>>>,
        fresh: Vec<u32>, // appended pages
    }
    let mut tables: Vec<TablePlan> = Vec::new();
    for (slot, ty, first, last, chunks) in plan {
        let chain: Vec<u32> = crate::pdb::table_pages(&data, page_size, first, last)
            .into_iter()
            .map(|off| (off / page_size) as u32)
            .collect();
        if chain.is_empty() {
            return Err(ReadError::Format("broken playlist table chain"));
        }
        // The first page is normally the table's sentinel ("strange") page,
        // which stays; a chain that starts on a data page has no sentinel and
        // every page is reusable.
        let first_flags = data
            .get(chain[0] as usize * page_size + 0x1B)
            .copied()
            .unwrap_or(0);
        let (keep_first, reusable) = if first_flags & 0x40 != 0 {
            (Some(chain[0]), chain[1..].to_vec())
        } else {
            (None, chain)
        };
        let need = chunks.len();
        let fresh: Vec<u32> = (reusable.len()..need)
            .map(|i| next_free + (i - reusable.len()) as u32)
            .collect();
        next_free += fresh.len() as u32;
        tables.push(TablePlan {
            slot,
            ty,
            keep_first,
            reusable,
            chunks,
            fresh,
        });
    }
    let total = next_free;
    if data.len() < total as usize * page_size {
        data.resize(total as usize * page_size, 0);
    }

    // Second pass: write pages, relink chains, update the directory.
    for t in &tables {
        let need = t.chunks.len();
        let used: Vec<u32> = t
            .reusable
            .iter()
            .copied()
            .take(need)
            .chain(t.fresh.iter().copied())
            .collect();
        for (i, chunk) in t.chunks.iter().enumerate() {
            let idx = used[i];
            let next = if i + 1 < need { used[i + 1] } else { total };
            let page = pdbw::data_page(idx, t.ty, next, chunk);
            let o = idx as usize * page_size;
            data[o..o + page_size].copy_from_slice(&page);
        }
        // Pages the shrunken table no longer needs are zeroed whole, the form
        // of rekordbox's empty-candidate pages: a reader that overruns the
        // chain finds nothing, and the first of them can end the chain.
        for &idx in t.reusable.iter().skip(need) {
            let o = idx as usize * page_size;
            data[o..o + page_size].fill(0);
        }
        // The chain ends at the directory's empty candidate: one past the end
        // like the full writer, or, for a table that emptied, its first freed
        // page. The kept sentinel links to the first data page (else straight
        // to the candidate) and its index body names the first data page
        // (else none), the way rekordbox leaves an empty table.
        let first_data = used.first().copied();
        let candidate = match first_data {
            Some(_) => total,
            None => t.reusable.first().copied().unwrap_or(total),
        };
        if let Some(sentinel) = t.keep_first {
            let s = sentinel as usize * page_size;
            data[s + 0x0C..s + 0x10].copy_from_slice(&first_data.unwrap_or(candidate).to_le_bytes());
            data[s + 0x2C..s + 0x30].copy_from_slice(&first_data.unwrap_or(0x03FF_FFFF).to_le_bytes());
        }
        // Directory: `first` is unchanged; `last` is the final live page (the
        // sentinel again when the table emptied).
        let base = 0x1C + t.slot * 16;
        let last = used.last().copied().or(t.keep_first).unwrap_or(candidate);
        data[base + 4..base + 8].copy_from_slice(&candidate.to_le_bytes());
        data[base + 12..base + 16].copy_from_slice(&last.to_le_bytes());
    }

    // Header: next_unused_page covers the appended pages; the sequence number
    // records that the database changed, as rekordbox does on every write.
    let seq = u32_at(data, 0x14).unwrap_or(0).wrapping_add(1);
    data[0x0C..0x10].copy_from_slice(&total.to_le_bytes());
    data[0x14..0x18].copy_from_slice(&seq.to_le_bytes());
    Ok(())
}

// ---------------------------------------------------------------------------
// exportLibrary.db sync
// ---------------------------------------------------------------------------

/// Replace the DLP database's `playlist` / `playlist_content` tables with the
/// export's current tree. Content ids are the DLP's own — resolved by file
/// path, the only key shared with the pdb; entries whose path the DLP doesn't
/// know are skipped rather than failing the edit.
fn sync_dlp_playlists(db_path: &Path, export: &RbExport) -> Result<(), ReadError> {
    with_local_dlp(db_path, |conn| mirror_playlists(conn, export))
}

/// The SQL behind [`sync_dlp_playlists`], on an open (local) copy.
fn mirror_playlists(conn: &rusqlite::Connection, export: &RbExport) -> rusqlite::Result<()> {
    let mut content_by_path: HashMap<String, i64> = HashMap::new();
    {
        let mut stmt = conn.prepare("SELECT content_id, path FROM content")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
        for (id, path) in rows.flatten() {
            content_by_path.insert(key(&path), id);
        }
    }
    conn.execute("DELETE FROM playlist", [])?;
    conn.execute("DELETE FROM playlist_content", [])?;
    for p in &export.playlists {
        conn.execute(
            "INSERT INTO playlist VALUES (?1, ?2, ?3, NULL, ?4, ?5)",
            rusqlite::params![
                p.id as i64,
                p.sort_order as i64,
                p.name,
                p.is_folder as i64,
                p.parent_id as i64,
            ],
        )?;
    }
    for p in export.playlists.iter().filter(|p| !p.is_folder) {
        let Some(tracks) = export.entries.get(&p.id) else {
            continue;
        };
        for (i, tid) in tracks.iter().enumerate() {
            let Some(track) = export.tracks.get(tid) else {
                continue;
            };
            let key = key(&track.file_path);
            let Some(content_id) = content_by_path.get(&key) else {
                continue;
            };
            conn.execute(
                "INSERT INTO playlist_content VALUES (?1, ?2, ?3)",
                rusqlite::params![p.id as i64, content_id, i as i64],
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anlz::AnlzInput;
    use crate::pdb::{read_export, read_stick};

    fn temp_root(tag: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("ordnung-edit-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("PIONEER").join("rekordbox")).unwrap();
        root
    }

    fn beats(bpm: f32) -> Vec<Beat> {
        (0..8)
            .map(|i| Beat {
                number: (i % 4) + 1,
                position_ms: 100 + i as u64 * 500,
                bpm,
            })
            .collect()
    }

    #[test]
    fn grid_edit_patches_only_that_row_s_tempo() {
        let root = temp_root("grid");
        let pdb = root.join("PIONEER/rekordbox/export.pdb");
        std::fs::copy(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/demo_tracks_export.pdb"),
            &pdb,
        )
        .unwrap();
        let before = read_export(&pdb).unwrap();
        let (&id, _) = before.tracks.iter().next().unwrap();

        let old = beats(120.0);
        let inp = AnlzInput {
            usb_path: "/Contents/x.mp3",
            beats: &old,
            duration_ms: 4_100,
            preview: &[128; 400],
            bands: &vec![64; 4 * 82],
            scroll: &[],
            cues: &[],
        };
        let dat = root.join("PIONEER/USBANLZ/P001/00000001/ANLZ0000.DAT");
        std::fs::create_dir_all(dat.parent().unwrap()).unwrap();
        std::fs::write(&dat, crate::anlz::build_dat(&inp)).unwrap();
        std::fs::write(dat.with_extension("EXT"), crate::anlz::build_ext(&inp)).unwrap();

        write_stick_beatgrid(&root, id, &dat, &beats(133.37)).unwrap();

        let after = read_export(&pdb).unwrap();
        assert_eq!(after.tracks[&id].tempo_centi_bpm, 13337);
        for (tid, t) in &before.tracks {
            if *tid != id {
                assert_eq!(&after.tracks[tid], t, "other rows must not change");
            }
        }
        assert_eq!(after.playlists, before.playlists);
        // Pristine copies exist, and the original grid reads back from them.
        assert!(pdb.with_extension("pdb.orig").is_file());
        assert!(dat.with_extension("DAT.orig").is_file());
        assert!(dat.with_extension("EXT.orig").is_file());
        assert_eq!(original_stick_beatgrid(&dat).unwrap(), old);
        assert_eq!(crate::anlz::read_beatgrid(&dat)[0].bpm, 133.37);

        // A second edit keeps the first backup.
        write_stick_beatgrid(&root, id, &dat, &beats(140.0)).unwrap();
        assert_eq!(original_stick_beatgrid(&dat).unwrap(), old);
        assert_eq!(read_export(&pdb).unwrap().tracks[&id].tempo_centi_bpm, 14000);

        assert!(matches!(
            write_stick_beatgrid(&root, 999_999, &dat, &beats(1.0)),
            Err(ReadError::Format("no such track row"))
        ));
    }

    #[test]
    fn cue_edit_lands_in_both_files_and_backs_up_once() {
        let root = temp_root("cues");
        let inp = AnlzInput {
            usb_path: "/Contents/x.mp3",
            beats: &[],
            duration_ms: 4_100,
            preview: &[],
            bands: &[],
            scroll: &[],
            cues: &[],
        };
        let dat = root.join("ANLZ0000.DAT");
        std::fs::write(&dat, crate::anlz::build_dat(&inp)).unwrap();
        std::fs::write(dat.with_extension("EXT"), crate::anlz::build_ext(&inp)).unwrap();
        let pristine = std::fs::read(&dat).unwrap();
        let cue = Cue {
            hot_slot: Some(1),
            position_ms: 2_000,
            loop_end_ms: None,
            label: Some("B".into()),
            color: Some([1, 2, 3]),
        };
        // No databases in this root: the ANLZ write alone must still land.
        assert!(matches!(
            write_stick_cues(&root, 1, &dat, std::slice::from_ref(&cue)),
            Err(ReadError::Io { .. })
        ));
        assert_eq!(crate::anlz::read_cues(&dat), vec![cue.clone()]);
        assert_eq!(std::fs::read(dat.with_extension("DAT.orig")).unwrap(), pristine);
        assert!(matches!(write_stick_cues(&root, 1, &dat, &[]), Err(ReadError::Io { .. })));
        assert!(crate::anlz::read_cues(&dat).is_empty());
        assert_eq!(std::fs::read(dat.with_extension("DAT.orig")).unwrap(), pristine);
    }

    /// The row with `id` as stored: header words and its 21 string slots.
    fn track_row(pdb: &Path, id: u32) -> TrackRowEdit {
        use crate::pdb::{page_rows, table_dir, table_pages};
        let data = std::fs::read(pdb).unwrap();
        if let Some((_, first, last)) = table_dir(&data).unwrap().find(TYPE_TRACKS) {
            for page_off in table_pages(&data, pdbw::PAGE, first, last) {
                for row in page_rows(&data, pdbw::PAGE, page_off, TYPE_TRACKS) {
                    if u32_at(&data, row + 0x48) == Some(id) {
                        return decode_track_row(&data, row).unwrap().0;
                    }
                }
            }
        }
        panic!("row {id} not found");
    }

    /// The 21 string slots of the row with `id`, as stored.
    fn row_strings(pdb: &Path, id: u32) -> [String; 21] {
        track_row(pdb, id).strings
    }

    #[test]
    fn cue_edit_bumps_the_counter_in_the_pdb_row_and_keeps_the_rest() {
        let root = temp_root("counter");
        let pdb = root.join("PIONEER/rekordbox/export.pdb");
        std::fs::copy(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/demo_tracks_export.pdb"),
            &pdb,
        )
        .unwrap();
        let before = read_export(&pdb).unwrap();
        let pristine = std::fs::read(&pdb).unwrap();
        // Pick a row in the middle of its page so both neighbours move.
        let mut ids: Vec<u32> = before.tracks.keys().copied().collect();
        ids.sort();
        let id = ids[ids.len() / 2];
        let strings_before = row_strings(&pdb, id);
        let others_before: Vec<[String; 21]> = ids.iter().map(|i| row_strings(&pdb, *i)).collect();

        let inp = AnlzInput {
            usb_path: "/Contents/x.mp3",
            beats: &[],
            duration_ms: 4_100,
            preview: &[],
            bands: &[],
            scroll: &[],
            cues: &[],
        };
        let dat = root.join("PIONEER/USBANLZ/P001/00000001/ANLZ0000.DAT");
        std::fs::create_dir_all(dat.parent().unwrap()).unwrap();
        std::fs::write(&dat, crate::anlz::build_dat(&inp)).unwrap();
        let cue = Cue {
            hot_slot: Some(0),
            position_ms: 1_000,
            loop_end_ms: None,
            label: None,
            color: None,
        };
        write_stick_cues(&root, id, &dat, std::slice::from_ref(&cue)).unwrap();
        write_stick_cues(&root, id, &dat, std::slice::from_ref(&cue)).unwrap();

        let after = read_export(&pdb).unwrap();
        // The parsed view of every row is unchanged; the stored bytes of the
        // edited row are not (its counter grew), so they are left out here
        // and checked slot by slot below.
        let parsed = |m: &std::collections::HashMap<u32, crate::pdb::RbTrack>| {
            let mut v: Vec<crate::pdb::RbTrack> =
                m.values().map(|t| crate::pdb::RbTrack { raw: None, ..t.clone() }).collect();
            v.sort_by(|a, b| a.file_path.cmp(&b.file_path));
            v
        };
        assert_eq!(parsed(&after.tracks), parsed(&before.tracks), "parsed rows must be unchanged");
        assert_eq!(after.playlists, before.playlists);
        let strings_after = row_strings(&pdb, id);
        assert_eq!(strings_after[4], "2", "cue counter counts from {:?}", strings_before[4]);
        for (i, s) in strings_after.iter().enumerate() {
            if i != 4 {
                assert_eq!(s, &strings_before[i], "slot {i} changed");
            }
        }
        for (i, tid) in ids.iter().enumerate() {
            if *tid != id {
                assert_eq!(row_strings(&pdb, *tid), others_before[i]);
            }
        }
        // Only the one page differs from the pristine file.
        let now = std::fs::read(&pdb).unwrap();
        assert_eq!(now.len(), pristine.len());
        let changed: Vec<usize> = (0..now.len() / pdbw::PAGE)
            .filter(|p| now[p * pdbw::PAGE..(p + 1) * pdbw::PAGE] != pristine[p * pdbw::PAGE..(p + 1) * pdbw::PAGE])
            .collect();
        assert_eq!(changed.len(), 1, "pages changed: {changed:?}");
        assert_eq!(std::fs::read(pdb.with_extension("pdb.orig")).unwrap(), pristine);

        // A grid write bumps the analysis counter and patches the tempo.
        write_stick_beatgrid(&root, id, &dat, &beats(128.0)).unwrap();
        let s = row_strings(&pdb, id);
        assert_eq!(s[4], "2");
        assert_eq!(s[3], (strings_before[3].parse::<u32>().unwrap_or(0) + 1).to_string());
        assert_eq!(read_export(&pdb).unwrap().tracks[&id].tempo_centi_bpm, 12800);
    }

    /// The page holding track `id`'s live row, by byte offset.
    fn page_of(data: &[u8], id: u32) -> usize {
        use crate::pdb::{table_dir, table_pages};
        let (_, first, last) = table_dir(data).unwrap().find(TYPE_TRACKS).unwrap();
        table_pages(data, pdbw::PAGE, first, last)
            .into_iter()
            .find(|&p| {
                data[p + 0x1B] & 0x40 == 0
                    && page_slots(data, p)
                        .iter()
                        .any(|&(live, row)| live && u32_at(data, row + 0x48) == Some(id))
            })
            .expect("row page")
    }

    /// The export summary's track count (type 19: `u16 0x0280, u16, u32 n`).
    fn summary_count(data: &[u8]) -> Option<u32> {
        use crate::pdb::{page_rows, table_dir, table_pages, u16_at};
        let (_, first, last) = table_dir(data).unwrap().find(TYPE_SUMMARY)?;
        table_pages(data, pdbw::PAGE, first, last)
            .into_iter()
            .flat_map(|p| page_rows(data, pdbw::PAGE, p, TYPE_SUMMARY))
            .find(|&row| u16_at(data, row) == Some(0x0280))
            .and_then(|row| u32_at(data, row + 4))
    }

    #[test]
    fn restore_previous_puts_back_every_kept_database_and_only_those() {
        let dir = std::env::temp_dir().join(format!("ordnung-restore-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let rb = rb_dir(&dir);
        std::fs::create_dir_all(&rb).unwrap();
        std::fs::write(rb.join("export.pdb"), b"new pdb").unwrap();
        std::fs::write(rb.join("export.pdb.prev"), b"old pdb").unwrap();
        std::fs::write(rb.join("exportLibrary.db.prev"), b"old dlp").unwrap();
        std::fs::write(rb.join("exportLibrary.db-wal"), b"stale").unwrap();
        std::fs::write(rb.join("exportExt.pdb"), b"ext stays").unwrap();
        assert!(has_previous(&dir));
        assert_eq!(restore_previous(&dir).unwrap(), 2);
        assert_eq!(std::fs::read(rb.join("export.pdb")).unwrap(), b"old pdb");
        assert_eq!(std::fs::read(rb.join("exportLibrary.db")).unwrap(), b"old dlp");
        assert!(!rb.join("exportLibrary.db-wal").exists());
        assert_eq!(std::fs::read(rb.join("exportExt.pdb")).unwrap(), b"ext stays");
        // The copies stay: restoring is repeatable.
        assert!(has_previous(&dir));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn delete_drops_rows_entries_files_and_counts() {
        use crate::pdb::u16_at;
        let root = temp_root("delete");
        let pdb = root.join("PIONEER/rekordbox/export.pdb");
        std::fs::copy(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/num_rows_export.pdb"),
            &pdb,
        )
        .unwrap();
        let before = read_export(&pdb).unwrap();
        // A track with artwork of its own takes the files along (no two
        // tracks of this fixture share an artwork id); one without has none.
        let lone = *before
            .tracks
            .iter()
            .find(|(_, t)| t.artwork_id != 0 && t.artwork_path.is_some())
            .map(|(id, _)| id)
            .expect("a track with artwork");
        let plain = *before
            .tracks
            .iter()
            .find(|(_, t)| t.artwork_id == 0)
            .map(|(id, _)| id)
            .expect("a track without artwork");
        let keep = *before.tracks.keys().find(|id| **id != lone && **id != plain).unwrap();
        let on_stick = |p: &str| root.join(p.trim_start_matches('/'));
        let rel = |id: u32| before.tracks[&id].file_path.trim_start_matches('/').to_string();
        for id in [lone, plain, keep] {
            let t = &before.tracks[&id];
            let audio = on_stick(&t.file_path);
            std::fs::create_dir_all(audio.parent().unwrap()).unwrap();
            std::fs::write(&audio, b"audio").unwrap();
            let dat = on_stick(t.analyze_path.as_deref().unwrap());
            std::fs::create_dir_all(dat.parent().unwrap()).unwrap();
            std::fs::write(&dat, b"dat").unwrap();
            std::fs::write(dat.with_extension("EXT"), b"ext").unwrap();
            if let Some(art) = t.artwork_path.as_deref().map(on_stick) {
                std::fs::create_dir_all(art.parent().unwrap()).unwrap();
                let n = art.file_name().unwrap().to_str().unwrap()[1..]
                    .trim_end_matches(".jpg")
                    .to_string();
                for name in [format!("a{n}.jpg"), format!("a{n}_m.jpg"), format!("b{n}.jpg"), format!("b{n}_m.jpg")] {
                    std::fs::write(art.with_file_name(name), b"jpg").unwrap();
                }
            }
        }
        // A Device Library Plus beside the pdb, every table a delete touches
        // holding a row for the doomed track.
        let db = root.join("PIONEER/rekordbox/exportLibrary.db");
        {
            let conn = crate::dlp::open(&db, rusqlite::OpenFlags::default()).unwrap();
            conn.execute_batch(
                "CREATE TABLE content(content_id INTEGER PRIMARY KEY, path varchar);
                 CREATE TABLE playlist(playlist_id INTEGER PRIMARY KEY, sequenceNo INTEGER, \
                     name varchar, image_id INTEGER, attribute INTEGER, playlist_id_parent INTEGER);
                 CREATE TABLE playlist_content(playlist_id INTEGER, content_id INTEGER, sequenceNo INTEGER);
                 CREATE TABLE property(deviceName varchar, dbVersion varchar, numberOfContents INTEGER);
                 CREATE TABLE cue(cue_id INTEGER PRIMARY KEY, content_id INTEGER);
                 CREATE TABLE hotCueBankList_cue(hotCueBankList_id INTEGER, cue_id INTEGER, sequenceNo INTEGER);
                 CREATE TABLE history_content(history_id INTEGER, content_id INTEGER, sequenceNo INTEGER);
                 CREATE TABLE myTag_content(myTag_id INTEGER, content_id INTEGER);
                 CREATE TABLE recommendedLike(content_id_1 INTEGER, content_id_2 INTEGER, rating INTEGER);
                 INSERT INTO property VALUES ('X', '1000', 3);
                 INSERT INTO cue VALUES (1, 902);
                 INSERT INTO hotCueBankList_cue VALUES (1, 1, 0);
                 INSERT INTO history_content VALUES (1, 902, 0);
                 INSERT INTO myTag_content VALUES (1, 902);
                 INSERT INTO recommendedLike VALUES (902, 903, 1);",
            )
            .unwrap();
            for (cid, id) in [(901, plain), (902, lone), (903, keep)] {
                conn.execute(
                    "INSERT INTO content VALUES (?1, ?2)",
                    rusqlite::params![cid, before.tracks[&id].file_path],
                )
                .unwrap();
            }
        }
        let pid = edit_stick_playlists(
            &root,
            &PlaylistOp::Create {
                name: "set".into(),
                parent_id: 0,
            },
        )
        .unwrap()
        .new_id
        .unwrap();
        edit_stick_playlists(
            &root,
            &PlaylistOp::AddTracks {
                id: pid,
                rel_paths: vec![rel(lone), rel(plain), rel(keep)],
            },
        )
        .unwrap();
        let pristine = std::fs::read(&pdb).unwrap();
        // A My Tag database with no links has nothing to shed: left byte for byte.
        let ext = root.join("PIONEER/rekordbox/exportExt.pdb");
        std::fs::write(&ext, pdbw::build_export_ext_pdb()).unwrap();

        let n = delete_stick_tracks(&root, &[rel(lone), rel(plain), "Contents/nope.mp3".into()]).unwrap();
        assert_eq!(n, 2);
        assert_eq!(std::fs::read(&ext).unwrap(), pdbw::build_export_ext_pdb());
        assert!(!ext.with_extension("pdb.orig").exists());

        let after = read_stick(&root).unwrap();
        assert_eq!(after.tracks.len(), before.tracks.len() - 2);
        assert!(!after.tracks.contains_key(&lone) && !after.tracks.contains_key(&plain));
        for (id, t) in &after.tracks {
            assert_eq!(t, &before.tracks[id], "surviving rows must not change");
        }
        assert_eq!(after.entries[&pid], vec![keep]);
        let data = std::fs::read(&pdb).unwrap();
        assert_eq!(summary_count(&data), Some(before.tracks.len() as u32 - 2));
        // The row's page carries the has-deletes flag, counts only live rows,
        // and is listed in the tracks table's index page.
        let page = page_of(&pristine, lone);
        assert_eq!(data[page + 0x1B], 0x34);
        let live = page_slots(&data, page).iter().filter(|(p, _)| *p).count();
        assert_eq!(u16_at(&data, page + 0x19), Some(32 * live as u16));
        let sentinel = crate::pdb::table_dir(&data).unwrap().find(TYPE_TRACKS).unwrap().1 as usize * pdbw::PAGE;
        let listed: Vec<u32> = (0..u16_at(&data, sentinel + 0x26).unwrap() as usize)
            .filter_map(|i| u32_at(&data, sentinel + 0x3C + 4 * i))
            .collect();
        assert!(listed.contains(&((page / pdbw::PAGE) as u32 * 8)), "index lists {listed:?}");
        let entries = listed.iter().filter(|e| **e != 0x1FFF_FFF8).count() as u16;
        assert_eq!(u16_at(&data, sentinel + 0x38), Some(entries));

        // Files: the deleted tracks' audio, analysis and artwork are gone;
        // the kept track stays.
        let t = &before.tracks[&lone];
        assert!(!on_stick(&t.file_path).exists());
        let dat = on_stick(t.analyze_path.as_deref().unwrap());
        assert!(!dat.exists() && !dat.with_extension("EXT").exists() && !dat.parent().unwrap().exists());
        let art = on_stick(t.artwork_path.as_deref().unwrap());
        assert!(!art.exists());
        let b = format!("b{}", &art.file_name().unwrap().to_str().unwrap()[1..]);
        assert!(!art.with_file_name(b).exists());
        assert!(!on_stick(&before.tracks[&plain].file_path).exists());
        assert!(on_stick(&before.tracks[&keep].file_path).exists());

        // The DLP shed the rows, counts what is left, and mirrors the playlist.
        let conn = crate::dlp::open_read_only(&db).unwrap();
        let count = |sql: &str| conn.query_row(sql, [], |r| r.get::<_, i64>(0)).unwrap();
        assert_eq!(count("SELECT COUNT(*) FROM content"), 1);
        assert_eq!(count("SELECT numberOfContents FROM property"), 1);
        for table in ["cue", "hotCueBankList_cue", "history_content", "myTag_content", "recommendedLike"] {
            assert_eq!(count(&format!("SELECT COUNT(*) FROM {table}")), 0, "{table}");
        }
        assert_eq!(count("SELECT content_id FROM playlist_content"), 903);
        drop(conn);

        assert!(pdb.with_extension("pdb.orig").is_file());
        assert!(db.with_extension("db.orig").is_file());
        assert_eq!(delete_stick_tracks(&root, &["Contents/nope.mp3".into()]).unwrap(), 0);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn relink_repoints_row_dlp_and_anlz_and_keeps_the_rest() {
        let root = temp_root("relink");
        let pdb = root.join("PIONEER/rekordbox/export.pdb");
        std::fs::copy(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/demo_tracks_export.pdb"),
            &pdb,
        )
        .unwrap();
        let before = read_export(&pdb).unwrap();
        let id = 2;
        let t = before.tracks[&id].clone();
        let was: u32 = row_strings(&pdb, id)[INFO_COUNTER_SLOT].trim().parse().unwrap_or(0);
        // The track's analysis pair, naming the old audio and holding a cue.
        let cue = Cue {
            hot_slot: Some(2),
            position_ms: 3_000,
            loop_end_ms: None,
            label: Some("drop".into()),
            color: None,
        };
        let inp = AnlzInput {
            usb_path: &t.file_path,
            beats: &[],
            duration_ms: 4_100,
            preview: &[],
            bands: &[],
            scroll: &[],
            cues: std::slice::from_ref(&cue),
        };
        let dat = root.join(t.analyze_path.as_deref().unwrap().trim_start_matches('/'));
        std::fs::create_dir_all(dat.parent().unwrap()).unwrap();
        std::fs::write(&dat, crate::anlz::build_dat(&inp)).unwrap();
        std::fs::write(dat.with_extension("EXT"), crate::anlz::build_ext(&inp)).unwrap();
        let cues_before = crate::anlz::read_cues(&dat);
        assert_eq!(cues_before.len(), 1);
        let db = root.join("PIONEER/rekordbox/exportLibrary.db");
        {
            let conn = crate::dlp::open(&db, rusqlite::OpenFlags::default()).unwrap();
            conn.execute_batch(
                "CREATE TABLE content(content_id INTEGER PRIMARY KEY, path varchar, fileName varchar, \
                     fileSize INTEGER, fileType INTEGER, bitrate INTEGER, bitDepth INTEGER, \
                     samplingRate INTEGER, informationUpdateCount INTEGER);",
            )
            .unwrap();
            conn.execute(
                "INSERT INTO content VALUES (902, ?1, 'Demo Track 2.mp3', 1, 1, 320, 16, 44100, NULL)",
                [&t.file_path],
            )
            .unwrap();
        }
        let props = AudioProperties {
            sample_rate_hz: 48_000,
            bit_depth: Some(16),
            channels: 2,
            duration_ms: 4_100,
            bitrate_kbps: None,
        };
        let new_path = "/Contents/Loopmasters/UnknownAlbum/Demo Track 2.aiff";
        relink_converted_track(&root, id, &new_path[1..], Format::Aiff, &props, 12_345).unwrap();

        let after = read_export(&pdb).unwrap();
        let n = &after.tracks[&id];
        assert_eq!(n.file_path, new_path);
        assert_eq!(n.sample_rate_hz, 48_000);
        assert_eq!(n.bitrate_kbps, 1536);
        assert_eq!(
            (&n.title, n.duration_s, n.tempo_centi_bpm, &n.analyze_path, &n.key),
            (&t.title, t.duration_s, t.tempo_centi_bpm, &t.analyze_path, &t.key)
        );
        assert_eq!(after.tracks[&1], before.tracks[&1], "the other row must not change");
        let row = track_row(&pdb, id);
        assert_eq!(row.strings[19], "Demo Track 2.aiff");
        assert_eq!(row.strings[INFO_COUNTER_SLOT], (was + 1).to_string());
        assert_eq!(u32::from_le_bytes(row.header[0x10..0x14].try_into().unwrap()), 12_345);
        assert_eq!(u16::from_le_bytes(row.header[0x52..0x54].try_into().unwrap()), 16);
        assert_eq!(u16::from_le_bytes(row.header[0x5A..0x5C].try_into().unwrap()), 12);

        let conn = crate::dlp::open_read_only(&db).unwrap();
        let got: (String, String, i64, i64, i64, i64, i64, i64) = conn
            .query_row(
                "SELECT path, fileName, fileSize, fileType, bitrate, bitDepth, samplingRate, \
                 informationUpdateCount FROM content WHERE content_id = 902",
                [],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                        r.get(6)?,
                        r.get(7)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(
            got,
            (new_path.into(), "Demo Track 2.aiff".into(), 12_345, 12, 1536, 16, 48_000, 1)
        );
        drop(conn);

        assert_eq!(crate::anlz::read_track_path(&dat).as_deref(), Some(new_path));
        assert_eq!(
            crate::anlz::read_track_path(&dat.with_extension("EXT")).as_deref(),
            Some(new_path)
        );
        assert_eq!(crate::anlz::read_cues(&dat), cues_before, "cues must survive the re-path");
        assert!(dat.with_extension("DAT.orig").is_file());
        let _ = std::fs::remove_dir_all(&root);
    }
}
