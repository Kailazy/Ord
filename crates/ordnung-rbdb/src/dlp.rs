//! Device Library Plus — the `exportLibrary.db` half of a modern export.
//!
//! rekordbox 6/7 exports write a second database next to `export.pdb`: a
//! SQLCipher-encrypted SQLite file that newer players (OPUS-QUAD, OMNIS-DUO,
//! XDJ-AZ, CDJ-3000 firmware) read instead of the DeviceSQL one. Crucially,
//! such exports leave `export.pdb`'s playlist-tree and playlist-entry tables
//! **empty** — the playlists exist only here. So a stick exported by
//! rekordbox 7 shows its full library through [`crate::pdb`] but zero
//! playlists, until this module fills them in (see [`crate::pdb::read_stick`]).
//!
//! The encryption key is a static constant rekordbox embeds (obfuscated) in
//! its own binary, identical for every export, and publicly documented by the
//! reverse-engineering community — see
//! <https://gist.github.com/0xdevalias/b803476793b56f7c45e6361799168eb0>.
//! Reading it here only ever decrypts the user's own export, on their own
//! stick, read-only.
//!
//! Same defensive posture as the pdb reader: a corrupt or unreadable database
//! returns an error; a malformed row is skipped, never a panic.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use crate::pdb::{RbPlaylist, ReadError};

/// The static Device Library Plus key (64 ASCII bytes, passed as a SQLCipher
/// passphrase with v4 compatibility). Distinct from the `master.db` key.
pub(crate) const DLP_KEY: &str =
    "r8gddnr4k847830ar6cqzbkk0el6qytmb3trbbx805jm74vez64i5o8fnrqryqls";

/// The playlists of one `exportLibrary.db`, in terms the pdb side can join
/// on: tree nodes plus each playlist's member *file paths* in play order
/// (paths as stored, `/Contents/...`, absolute from the volume root).
#[derive(Debug, Default, Clone)]
pub struct DlpPlaylists {
    /// Every playlist/folder node, sorted by sort order.
    pub playlists: Vec<RbPlaylist>,
    /// Playlist id → member file paths in playlist order.
    pub entries_by_path: HashMap<u32, Vec<String>>,
}

/// Open an `exportLibrary.db` with the Device Library Plus key.
pub(crate) fn open(
    db_path: &Path,
    flags: rusqlite::OpenFlags,
) -> Result<rusqlite::Connection, ReadError> {
    let conn = rusqlite::Connection::open_with_flags(db_path, flags)?;
    conn.execute_batch(&format!(
        "PRAGMA key = '{DLP_KEY}'; PRAGMA cipher_compatibility = 4;"
    ))?;
    Ok(conn)
}

/// A stick's `exportLibrary.db` opened for reading, through a copy on local
/// disk. rekordbox leaves the database in WAL mode, and SQLite creates the
/// `-wal` and `-shm` sidecars beside any WAL-mode database it opens, even
/// read-only: read in place, plugging a stick in would write two files onto
/// it. The copy (with the stick's WAL, when it has one) goes with the
/// connection: fields drop in order, the connection closes first.
pub(crate) struct ReadOnlyDlp {
    conn: rusqlite::Connection,
    _scratch: Scratch,
}

impl std::ops::Deref for ReadOnlyDlp {
    type Target = rusqlite::Connection;
    fn deref(&self) -> &rusqlite::Connection {
        &self.conn
    }
}

/// A scratch database path removed, sidecars included, when dropped.
struct Scratch(std::path::PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        remove_scratch(&self.0);
    }
}

/// Open an `exportLibrary.db` read-only with the Device Library Plus key.
pub(crate) fn open_read_only(db_path: &Path) -> Result<ReadOnlyDlp, ReadError> {
    let scratch = Scratch(scratch_db_path("ordnung-dlp-read"));
    scratch_copy(db_path, &scratch.0).map_err(ReadError::io(db_path))?;
    let conn = open(&scratch.0, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    Ok(ReadOnlyDlp {
        conn,
        _scratch: scratch,
    })
}

/// What a `content` row says about which track it is, for checking the
/// Device Library Plus against `export.pdb` before either lands: the two
/// must agree on these or rekordbox refuses the stick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ContentIdentity {
    pub path: String,
    pub master_db_id: u32,
    pub master_content_id: u32,
    pub track_number: u32,
}

/// Every `content` row of `db_path`, keyed by content id (the pdb track id).
pub(crate) fn content_identity(db_path: &Path) -> Result<BTreeMap<u32, ContentIdentity>, ReadError> {
    let conn = open_read_only(db_path)?;
    let mut stmt =
        conn.prepare("SELECT content_id, path, masterDbId, masterContentId, trackNo FROM content")?;
    let rows = stmt.query_map([], |r| {
        let int = |i: usize| r.get::<_, i64>(i).unwrap_or(0) as u32;
        Ok((
            int(0),
            ContentIdentity {
                path: r.get(1)?,
                master_db_id: int(2),
                master_content_id: int(3),
                track_number: int(4),
            },
        ))
    })?;
    Ok(rows.flatten().collect())
}

/// The stick's creation date as its `property` row has it (`YYYY-MM-DD`),
/// for the pdb summary row of a merge to repeat; `None` without a readable
/// library.
pub(crate) fn created_date(db_path: &Path) -> Option<String> {
    let conn = open_read_only(db_path).ok()?;
    conn.query_row("SELECT createdDate FROM property", [], |r| r.get::<_, String>(0))
        .ok()
        .filter(|d| !d.is_empty())
}

/// Open `db_path` (an `exportLibrary.db`) read-only and pull the playlist
/// tree and memberships out of it.
pub fn read_playlists(db_path: &Path) -> Result<DlpPlaylists, ReadError> {
    let conn = open_read_only(db_path)?;

    let mut out = DlpPlaylists::default();

    // The playlist tree. `attribute` follows the desktop database's
    // convention: 0 = playlist, 1 = folder (4, a smart playlist, can't occur
    // on an export — rekordbox freezes them into plain lists).
    let mut stmt = conn
        .prepare(
            "SELECT playlist_id, sequenceNo, name, attribute, playlist_id_parent \
             FROM playlist",
        )?;
    let rows = stmt
        .query_map([], |r| {
            Ok(RbPlaylist {
                id: r.get::<_, i64>(0)? as u32,
                sort_order: r.get::<_, i64>(1).unwrap_or(0) as u32,
                name: r.get::<_, String>(2)?,
                is_folder: r.get::<_, i64>(3).unwrap_or(0) == 1,
                parent_id: r.get::<_, i64>(4).unwrap_or(0) as u32,
            })
        })?;
    for row in rows.flatten() {
        out.playlists.push(row);
    }
    out.playlists.sort_by_key(|p| p.sort_order);

    // Memberships, joined to each member's file path: content ids equal the
    // pdb's track ids on every stick seen, but the path is the key the
    // reader matches on, so a stick whose ids ever differ still resolves.
    let mut stmt = conn
        .prepare(
            "SELECT pc.playlist_id, c.path \
             FROM playlist_content pc \
             JOIN content c ON c.content_id = pc.content_id \
             ORDER BY pc.playlist_id, pc.sequenceNo",
        )?;
    let rows = stmt
        .query_map([], |r| {
            Ok((r.get::<_, i64>(0)? as u32, r.get::<_, String>(1)?))
        })?;
    for (playlist, path) in rows.flatten() {
        out.entries_by_path.entry(playlist).or_default().push(path);
    }
    // Empty playlists still get an entry so the tree mirrors the player.
    for p in &out.playlists {
        if !p.is_folder {
            out.entries_by_path.entry(p.id).or_default();
        }
    }

    Ok(out)
}

// ---------------------------------------------------------------------------
// Write side (Phase 5)
// ---------------------------------------------------------------------------

/// Full 22-table Device Library Plus schema, as rekordbox 7.2.2 writes it
/// (column list captured from the EYEBAGS golden reference — see
/// `docs/rekordbox-export-structure.md` §4).
const DLP_SCHEMA: &str = "
CREATE TABLE album(album_id INTEGER PRIMARY KEY, name varchar, artist_id INTEGER,
    image_id INTEGER, isComplation INTEGER, nameForSearch varchar);
CREATE TABLE artist(artist_id INTEGER PRIMARY KEY, name varchar, nameForSearch varchar);
CREATE TABLE category(category_id INTEGER PRIMARY KEY, menuItem_id INTEGER,
    sequenceNo INTEGER, isVisible INTEGER);
CREATE TABLE color(color_id INTEGER PRIMARY KEY, name varchar);
CREATE TABLE content(content_id INTEGER PRIMARY KEY, title varchar, titleForSearch varchar,
    subtitle varchar, bpmx100 INTEGER, length INTEGER, trackNo INTEGER, discNo INTEGER,
    artist_id_artist INTEGER, artist_id_remixer INTEGER, artist_id_originalArtist INTEGER,
    artist_id_composer INTEGER, artist_id_lyricist INTEGER, album_id INTEGER,
    genre_id INTEGER, label_id INTEGER, key_id INTEGER, color_id INTEGER, image_id INTEGER,
    djComment varchar, rating INTEGER, releaseYear INTEGER, releaseDate varchar,
    dateCreated varchar, dateAdded varchar, path varchar, fileName varchar,
    fileSize INTEGER, fileType INTEGER, bitrate INTEGER, bitDepth INTEGER,
    samplingRate INTEGER, isrc varchar, djPlayCount INTEGER, isHotCueAutoLoadOn INTEGER,
    isKuvoDeliverStatusOn INTEGER, kuvoDeliveryComment varchar, masterDbId INTEGER,
    masterContentId INTEGER, analysisDataFilePath varchar, analysedBits INTEGER,
    contentLink INTEGER, hasModified INTEGER, cueUpdateCount INTEGER,
    analysisDataUpdateCount INTEGER, informationUpdateCount INTEGER);
CREATE TABLE cue(cue_id INTEGER PRIMARY KEY, content_id INTEGER, kind INTEGER,
    colorTableIndex INTEGER, cueComment varchar, isActiveLoop INTEGER,
    beatLoopNumerator INTEGER, beatLoopDenominator INTEGER, inUsec INTEGER, outUsec INTEGER,
    in150FramePerSec INTEGER, out150FramePerSec INTEGER, inMpegFrameNumber INTEGER,
    outMpegFrameNumber INTEGER, inMpegAbs INTEGER, outMpegAbs INTEGER,
    inDecodingStartFramePosition INTEGER, outDecodingStartFramePosition INTEGER,
    inFileOffsetInBlock INTEGER, OutFileOffsetInBlock INTEGER,
    inNumberOfSampleInBlock INTEGER, outNumberOfSampleInBlock INTEGER);
CREATE TABLE genre(genre_id INTEGER PRIMARY KEY, name varchar);
CREATE TABLE history(history_id INTEGER PRIMARY KEY, sequenceNo INTEGER, name varchar,
    attribute INTEGER, history_id_parent INTEGER);
CREATE TABLE history_content(history_id INTEGER, content_id INTEGER, sequenceNo INTEGER);
CREATE TABLE hotCueBankList(hotCueBankList_id INTEGER PRIMARY KEY, sequenceNo INTEGER,
    name varchar, image_id INTEGER, attribute INTEGER, hotCueBankList_id_parent INTEGER);
CREATE TABLE hotCueBankList_cue(hotCueBankList_id INTEGER, cue_id INTEGER, sequenceNo INTEGER);
CREATE TABLE image(image_id INTEGER PRIMARY KEY, path varchar);
CREATE TABLE key(key_id INTEGER PRIMARY KEY, name varchar);
CREATE TABLE label(label_id INTEGER PRIMARY KEY, name varchar);
CREATE TABLE menuItem(menuItem_id INTEGER PRIMARY KEY, kind INTEGER, name varchar);
CREATE TABLE myTag(myTag_id INTEGER PRIMARY KEY, sequenceNo INTEGER, name varchar,
    attribute INTEGER, myTag_id_parent INTEGER);
CREATE TABLE myTag_content(myTag_id INTEGER, content_id INTEGER);
CREATE TABLE playlist(playlist_id INTEGER PRIMARY KEY, sequenceNo INTEGER, name varchar,
    image_id INTEGER, attribute INTEGER, playlist_id_parent INTEGER);
CREATE TABLE playlist_content(playlist_id INTEGER, content_id INTEGER, sequenceNo INTEGER);
CREATE TABLE property(deviceName varchar, dbVersion varchar, numberOfContents INTEGER,
    createdDate varchar, backGroundColorType INTEGER, myTagMasterDBID INTEGER);
CREATE TABLE recommendedLike(content_id_1 INTEGER, content_id_2 INTEGER, rating INTEGER,
    createdDate INTEGER);
CREATE TABLE sort(sort_id INTEGER PRIMARY KEY, menuItem_id INTEGER, sequenceNo INTEGER,
    isVisible INTEGER, isSelectedAsSubColumn INTEGER);
";

/// Player browse-menu definitions, verbatim from the golden reference. The
/// `\u{FFFA}`/`\u{FFFB}` wrappers (interlinear annotation anchors) are part of
/// the names as rekordbox stores them.
const MENU_ITEMS: &[(i64, i64, &str)] = &[
    (1, 128, "GENRE"), (2, 129, "ARTIST"), (3, 130, "ALBUM"), (4, 131, "TRACK"),
    (5, 133, "BPM"), (6, 134, "RATING"), (7, 135, "YEAR"), (8, 136, "REMIXER"),
    (9, 137, "LABEL"), (10, 138, "ORIGINAL ARTIST"), (11, 139, "KEY"), (12, 141, "CUE"),
    (13, 142, "COLOR"), (14, 146, "TIME"), (15, 147, "BITRATE"), (16, 148, "FILE NAME"),
    (17, 132, "PLAYLIST"), (18, 152, "HOT CUE BANK"), (19, 149, "HISTORY"),
    (20, 145, "SEARCH"), (21, 150, "COMMENTS"), (22, 140, "DATE ADDED"),
    (23, 151, "DJ PLAY COUNT"), (24, 144, "FOLDER"), (25, 161, "DEFAULT"),
    (26, 162, "ALPHABET"), (27, 170, "MATCHING"),
];

const CATEGORIES: &[(i64, i64, i64, i64)] = &[
    (1, 1, 0, 0), (2, 2, 1, 1), (3, 3, 2, 1), (4, 4, 3, 1), (5, 17, 5, 1),
    (6, 5, 0, 0), (7, 6, 0, 0), (8, 7, 0, 0), (9, 8, 0, 0), (10, 9, 0, 0),
    (11, 10, 0, 0), (12, 11, 4, 1), (15, 13, 0, 0), (17, 24, 9, 1), (18, 20, 7, 1),
    (19, 14, 0, 0), (20, 15, 0, 0), (21, 16, 0, 0), (22, 19, 6, 1), (23, 18, 0, 0),
    (26, 27, 8, 1), (27, 22, 10, 1),
];

const SORTS: &[(i64, i64, i64, i64, i64)] = &[
    (0, 25, 1, 1, 0), (1, 26, 2, 1, 0), (2, 2, 3, 1, 0), (3, 3, 4, 1, 0),
    (4, 5, 5, 1, 0), (5, 6, 6, 1, 0), (6, 1, 0, 0, 0), (7, 21, 0, 0, 0),
    (8, 14, 0, 0, 0), (9, 8, 0, 0, 0), (10, 9, 0, 0, 0), (11, 10, 0, 0, 0),
    (12, 11, 7, 1, 0), (13, 15, 0, 0, 0), (15, 13, 0, 0, 0), (16, 23, 0, 0, 0),
    (17, 22, 0, 0, 0),
];



/// Put a finished local database on the stick. Stale journal sidecars from
/// a prior write go first — a leftover -wal/-shm beside a fresh database
/// makes SQLite try (and, on FAT, fail) to recover. The database itself is
/// replaced atomically, never removed first, so an interrupted placement
/// leaves the previous complete file rather than nothing at all.
pub(crate) fn place_library(tmp: &Path, db_path: &Path) -> std::io::Result<()> {
    for suffix in ["-wal", "-shm"] {
        let _ = std::fs::remove_file(crate::export::sibling(db_path, suffix));
    }
    crate::export::place_atomic(tmp, db_path).map(|_| ())
}

/// Create and populate a complete Device Library Plus database at `db_path`,
/// mirroring the same resolved library as the `export.pdb`, fully
/// checkpointed in rollback-journal mode so read-only players never need to
/// recover a WAL. `db_path` must be on local disk ([`scratch_db_path`]):
/// SQLite cannot write in place on macOS's msdos (FAT32) driver — the second
/// transaction dies with "attempt to write a readonly database", leaving a
/// one-table stub that players reject as a corrupted device library. The
/// finished file lands on the stick through [`place_library`].
pub(crate) fn build_library(
    db_path: &Path,
    t: &crate::pdbw::PdbTables,
    device_name: &str,
) -> Result<(), ReadError> {
    let conn = open(db_path, rusqlite::OpenFlags::default())?;
    conn.execute_batch(DLP_SCHEMA)?;

    conn.execute_batch("BEGIN")?;
    {
        for (table, list) in named_tables(t) {
            for (id, name) in list {
                insert_named(&conn, table, *id as i64, name)?;
            }
        }
        for (i, name) in crate::pdbw::COLORS.iter().enumerate() {
            conn.execute(
                "INSERT INTO color VALUES (?1, ?2)",
                rusqlite::params![i as i64 + 1, name],
            )?;
        }
        // Artwork mirrors: same ids as the pdb artwork table, but the DLP
        // references the b-named copies of the same JPEGs.
        for (id, _a_path) in &t.artwork {
            conn.execute(
                "INSERT INTO image VALUES (?1, ?2)",
                rusqlite::params![*id as i64, crate::artwork::dlp_path(*id)],
            )?;
        }
        for (id, kind, name) in MENU_ITEMS {
            conn.execute(
                "INSERT INTO menuItem VALUES (?1, ?2, ?3)",
                rusqlite::params![id, kind, format!("\u{FFFA}{name}\u{FFFB}")],
            )?;
        }
        for (id, mi, seq, vis) in CATEGORIES {
            conn.execute(
                "INSERT INTO category VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![id, mi, seq, vis],
            )?;
        }
        for (id, mi, seq, vis, sub) in SORTS {
            conn.execute(
                "INSERT INTO sort VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![id, mi, seq, vis, sub],
            )?;
        }
        insert_content(&conn, &t.tracks, t.master_db_id)?;
        insert_playlists(&conn, t)?;
        conn.execute(
            "INSERT INTO property VALUES (?1, '1000', ?2, ?3, 0, ?4)",
            rusqlite::params![
                device_name,
                t.tracks.len() as i64,
                t.created_date,
                t.master_db_id as i64,
            ],
        )?;
    }
    conn.execute_batch("COMMIT")?;
    Ok(())
}

/// Bring a local copy of a stick's own Device Library Plus up to `t` for a
/// merge. The rows `resolved` names (new and re-selected tracks) are
/// replaced, their artist, album, genre, label and key interned into the
/// stick's own tables by name; new artwork is added; the playlist tables
/// are rewritten from `t`; `property.numberOfContents` is recounted.
/// Everything else the stick carries stays as rekordbox wrote it: the
/// carried tracks' rows, My Tags, cues, history, and the property row's
/// name, dates and tag id, so rekordbox still knows the stick as its own.
pub(crate) fn update_library(
    db_path: &Path,
    t: &crate::pdbw::PdbTables,
    resolved: &[u32],
) -> Result<(), ReadError> {
    let conn = open(db_path, rusqlite::OpenFlags::default())?;
    conn.execute_batch("BEGIN")?;
    {
        fn name_of(list: &[(u32, String)], id: u32) -> Option<&str> {
            list.iter().find(|(i, _)| *i == id).map(|(_, n)| n.as_str())
        }
        let mut rows = Vec::new();
        for tr in t.tracks.iter().filter(|tr| resolved.contains(&tr.id)) {
            // Its cue rows go with it: the library's cues replace them in
            // the ANLZ files, and a DLP cue the ANLZ no longer has is stale.
            conn.execute(
                "DELETE FROM hotCueBankList_cue WHERE cue_id IN \
                 (SELECT cue_id FROM cue WHERE content_id = ?1)",
                [tr.id as i64],
            )?;
            conn.execute("DELETE FROM cue WHERE content_id = ?1", [tr.id as i64])?;
            conn.execute("DELETE FROM content WHERE content_id = ?1", [tr.id as i64])?;
            let mut row = tr.clone();
            row.artist_id = intern(&conn, "artist", name_of(&t.artists, tr.artist_id))?;
            row.composer_id = intern(&conn, "artist", name_of(&t.artists, tr.composer_id))?;
            row.remixer_id = intern(&conn, "artist", name_of(&t.artists, tr.remixer_id))?;
            row.original_artist_id =
                intern(&conn, "artist", name_of(&t.artists, tr.original_artist_id))?;
            row.album_id = intern(&conn, "album", name_of(&t.albums, tr.album_id))?;
            row.genre_id = intern(&conn, "genre", name_of(&t.genres, tr.genre_id))?;
            row.label_id = intern(&conn, "label", name_of(&t.labels, tr.label_id))?;
            row.key_id = intern(&conn, "key", name_of(&t.keys, tr.key_id))?;
            rows.push(row);
        }
        insert_content(&conn, &rows, t.master_db_id)?;
        for (id, _) in &t.artwork {
            conn.execute(
                "INSERT OR IGNORE INTO image VALUES (?1, ?2)",
                rusqlite::params![*id as i64, crate::artwork::dlp_path(*id)],
            )?;
        }
        conn.execute("DELETE FROM playlist_content", [])?;
        conn.execute("DELETE FROM playlist", [])?;
        insert_playlists(&conn, t)?;
        conn.execute(
            "UPDATE property SET numberOfContents = (SELECT COUNT(*) FROM content)",
            [],
        )?;
    }
    conn.execute_batch("COMMIT")?;
    checkpoint(&conn)
}

/// Fold the WAL into the main file: the copy may have come with the stick's
/// WAL ([`scratch_copy`]), and only the main file is placed back.
pub(crate) fn checkpoint(conn: &rusqlite::Connection) -> Result<(), ReadError> {
    conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")?;
    Ok(())
}

/// Copy a stick's database to `tmp` on local disk to edit it — SQLite cannot
/// write in place on macOS's msdos driver — with the stick's WAL beside it
/// when there is one, so rows a yanked stick left only in the WAL come along.
pub(crate) fn scratch_copy(db_path: &Path, tmp: &Path) -> std::io::Result<()> {
    remove_scratch(tmp);
    std::fs::copy(db_path, tmp)?;
    let wal = crate::export::sibling(db_path, "-wal");
    if wal.is_file() {
        std::fs::copy(&wal, crate::export::sibling(tmp, "-wal"))?;
    }
    Ok(())
}

/// Remove a scratch database and its journal sidecars.
pub(crate) fn remove_scratch(tmp: &Path) {
    let _ = std::fs::remove_file(tmp);
    for suffix in ["-wal", "-shm"] {
        let _ = std::fs::remove_file(crate::export::sibling(tmp, suffix));
    }
}

/// The id `name` has in one of the DLP's interned tables (`artist`, `album`,
/// `genre`, `label`, `key`), inserted after the table's highest when new; 0
/// for no name.
fn intern(conn: &rusqlite::Connection, table: &str, name: Option<&str>) -> rusqlite::Result<u32> {
    use rusqlite::OptionalExtension;
    let Some(name) = name else { return Ok(0) };
    let have: Option<i64> = conn
        .query_row(&format!("SELECT {table}_id FROM {table} WHERE name = ?1"), [name], |r| r.get(0))
        .optional()?;
    if let Some(id) = have {
        return Ok(id as u32);
    }
    let id: i64 =
        conn.query_row(&format!("SELECT COALESCE(MAX({table}_id), 0) + 1 FROM {table}"), [], |r| r.get(0))?;
    insert_named(conn, table, id, name)?;
    Ok(id as u32)
}

/// The five interned name tables and the rows `t` holds for each.
fn named_tables(t: &crate::pdbw::PdbTables) -> [(&'static str, &Vec<(u32, String)>); 5] {
    [
        ("artist", &t.artists),
        ("album", &t.albums),
        ("genre", &t.genres),
        ("label", &t.labels),
        ("key", &t.keys),
    ]
}

/// One row of an interned name table, in the shape rekordbox gives it.
fn insert_named(conn: &rusqlite::Connection, table: &str, id: i64, name: &str) -> rusqlite::Result<usize> {
    match table {
        "artist" => conn.execute(
            "INSERT INTO artist VALUES (?1, ?2, ?3)",
            rusqlite::params![id, name, name.to_lowercase()],
        ),
        "album" => conn.execute(
            "INSERT INTO album VALUES (?1, ?2, NULL, NULL, 0, ?3)",
            rusqlite::params![id, name, name.to_lowercase()],
        ),
        _ => conn.execute(&format!("INSERT INTO {table} VALUES (?1, ?2)"), rusqlite::params![id, name]),
    }
}

/// The `content` rows for `tracks`, every row stamped with `master_db_id`.
fn insert_content(
    conn: &rusqlite::Connection,
    tracks: &[crate::pdbw::TrackRow],
    master_db_id: u32,
) -> rusqlite::Result<()> {
    let mut content = conn.prepare(
        "INSERT INTO content VALUES (?1, ?2, ?3, NULL, ?4, ?5, ?6, ?7, \
         ?8, ?29, ?30, ?31, 0, ?9, ?10, ?11, ?12, 0, ?28, \
         ?13, ?14, ?15, NULL, ?16, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, \
         0, 1, 1, NULL, ?25, ?26, ?27, 41, 788224, 0, NULL, 1, 1)",
    )?;
    for tr in tracks {
        content.execute(rusqlite::params![
            tr.id as i64,
            tr.title,
            // rekordbox 7 leaves titleForSearch empty on every row of
            // its exports (94 of 94 reference tracks); nothing reads it.
            "",
            tr.tempo_centi_bpm as i64,
            tr.duration_s as i64,
            tr.track_number as i64,
            tr.disc_number as i64,
            zero_null(tr.artist_id),
            zero_null(tr.album_id),
            zero_null(tr.genre_id),
            zero_null(tr.label_id),
            zero_null(tr.key_id),
            tr.comment,
            tr.rating as i64,
            tr.year as i64, // rekordbox writes 0, not NULL, for no year
            tr.date_added,
            tr.file_path,
            tr.filename,
            tr.file_size as i64,
            tr.file_type as i64,
            tr.bitrate_kbps as i64,
            tr.sample_depth as i64,
            tr.sample_rate_hz as i64,
            tr.isrc,
            master_db_id as i64,
            tr.master_content_id as i64,
            tr.analyze_path,
            zero_null(tr.artwork_id),
            zero_null(tr.remixer_id),
            zero_null(tr.original_artist_id),
            zero_null(tr.composer_id),
        ])?;
    }
    Ok(())
}

/// The playlist tree and memberships of `t`, into empty playlist tables.
fn insert_playlists(conn: &rusqlite::Connection, t: &crate::pdbw::PdbTables) -> rusqlite::Result<()> {
    for p in &t.playlists {
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
    for (idx, track, playlist) in &t.playlist_entries {
        conn.execute(
            "INSERT INTO playlist_content VALUES (?1, ?2, ?3)",
            rusqlite::params![*playlist as i64, *track as i64, *idx as i64],
        )?;
    }
    Ok(())
}

/// rekordbox leaves absent interned refs NULL rather than 0.
fn zero_null(v: u32) -> Option<i64> {
    (v != 0).then_some(v as i64)
}

/// A collision-free scratch path on local disk for building a database that
/// will be copied onto the stick whole (process id alone isn't unique enough:
/// parallel test threads share one process).
pub(crate) fn scratch_db_path(prefix: &str) -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    std::env::temp_dir().join(format!(
        "{prefix}-{}-{}.db",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reading a stick's library must not write to the stick: a WAL-mode
    /// database (rekordbox's own mode) read in place grows `-wal` and `-shm`
    /// sidecars on every open, even a read-only one.
    #[test]
    fn reading_a_wal_mode_library_leaves_no_sidecars_beside_it() {
        let dir = std::env::temp_dir().join(format!("ordnung-dlp-read-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("exportLibrary.db");
        let t = crate::pdbw::PdbTables {
            created_date: "2026-09-04".into(),
            master_db_id: 7,
            ..Default::default()
        };
        build_library(&db, &t, "STICK").unwrap();
        open(&db, rusqlite::OpenFlags::default())
            .unwrap()
            .execute_batch("PRAGMA journal_mode = WAL")
            .unwrap();
        for suffix in ["-wal", "-shm"] {
            let _ = std::fs::remove_file(crate::export::sibling(&db, suffix));
        }
        read_playlists(&db).unwrap();
        content_identity(&db).unwrap();
        let left: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(left, vec!["exportLibrary.db".to_string()], "sidecars on the stick");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The full production writer must persist the complete 22-table schema
    /// and its rows — a partially written exportLibrary.db is exactly what a
    /// player rejects as "Device library is corrupted".
    #[test]
    fn merge_updates_the_sticks_own_library_and_keeps_its_tags() {
        let dir = std::env::temp_dir().join(format!("ordnung-dlp-merge-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("exportLibrary.db");
        let row = |id: u32, title: &str, artist_id: u32| crate::pdbw::TrackRow {
            id,
            title: title.into(),
            filename: format!("{id}.mp3"),
            file_path: format!("/Contents/{id}.mp3"),
            artist_id,
            ..Default::default()
        };
        let mut t = crate::pdbw::PdbTables {
            tracks: vec![row(1, "One", 1), row(2, "Two", 1)],
            artists: vec![(1, "A".into())],
            created_date: "2026-09-04".into(),
            master_db_id: 7,
            ..Default::default()
        };
        build_library(&db, &t, "STICK").unwrap();
        // What rekordbox would add on the stick: a My Tag on track 1.
        open(&db, rusqlite::OpenFlags::default())
            .unwrap()
            .execute_batch(
                "INSERT INTO myTag VALUES (5, 1, 'Peak', 0, 0); INSERT INTO myTag_content VALUES (5, 1); \
                 INSERT INTO cue (cue_id, content_id) VALUES (9, 1); INSERT INTO cue (cue_id, content_id) VALUES (10, 2);",
            )
            .unwrap();
        // The merge: 1 re-selected under artist B, 3 new, 2 carried; one playlist.
        t.tracks = vec![row(1, "One", 2), row(3, "Three", 1), row(2, "Two", 1)];
        t.artists = vec![(1, "A".into()), (2, "B".into())];
        t.playlists = vec![RbPlaylist { id: 1, parent_id: 0, sort_order: 1, is_folder: false, name: "set".into() }];
        t.playlist_entries = vec![(0, 3, 1), (1, 1, 1)];
        update_library(&db, &t, &[1, 3]).unwrap();

        let conn = open_read_only(&db).unwrap();
        let count = |sql: &str| conn.query_row(sql, [], |r| r.get::<_, i64>(0)).unwrap();
        assert_eq!(count("SELECT count(*) FROM content"), 3);
        assert_eq!(count("SELECT count(*) FROM myTag_content WHERE myTag_id = 5 AND content_id = 1"), 1);
        // The re-selected row's cues are the library's now; the carried row keeps its own.
        assert_eq!(count("SELECT count(*) FROM cue WHERE content_id = 1"), 0);
        assert_eq!(count("SELECT count(*) FROM cue WHERE content_id = 2"), 1);
        assert_eq!(count("SELECT numberOfContents FROM property"), 3);
        assert_eq!(count("SELECT deviceName = 'STICK' FROM property"), 1);
        // B interned beside A in the stick's own table; row 1 points at it.
        let b = count("SELECT artist_id FROM artist WHERE name = 'B'");
        assert_eq!(count("SELECT count(*) FROM artist"), 2);
        assert_eq!(count("SELECT artist_id_artist FROM content WHERE content_id = 1"), b);
        assert_eq!(count("SELECT masterDbId FROM content WHERE content_id = 3"), 7);
        assert_eq!(count("SELECT count(*) FROM playlist_content WHERE playlist_id = 1"), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn library_persists_every_table() {
        let dir = std::env::temp_dir().join(format!("ordnung-dlpw-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("exportLibrary.db");

        let tables = crate::pdbw::PdbTables {
            tracks: vec![crate::pdbw::TrackRow {
                id: 1,
                title: "One".into(),
                filename: "one.mp3".into(),
                file_path: "/Contents/one.mp3".into(),
                artwork_id: 1,
                ..Default::default()
            }],
            genres: vec![(1, "House".into())],
            artists: vec![(1, "A".into())],
            albums: vec![(1, "B".into())],
            labels: vec![],
            keys: vec![(1, "8A".into())],
            artwork: vec![(1, "/PIONEER/Artwork/00001/a1.jpg".into())],
            playlists: vec![RbPlaylist {
                id: 1,
                parent_id: 0,
                sort_order: 1,
                is_folder: false,
                name: "set".into(),
            }],
            playlist_entries: vec![(1, 1, 1)],
            created_date: "2026-09-04".into(),
            master_db_id: 1,
        };
        build_library(&db, &tables, "TEST").expect("write");

        let conn = open_read_only(&db).unwrap();
        let n_tables: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n_tables, 22, "full DLP schema must be present");
        for (table, want) in [
            ("content", 1i64),
            ("playlist", 1),
            ("playlist_content", 1),
            ("menuItem", 27),
            ("category", 22),
            ("sort", 17),
            ("property", 1),
            ("image", 1),
        ] {
            let n: i64 = conn
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
                .unwrap();
            assert_eq!(n, want, "{table} row count");
        }
        // Artwork lands as the b-named copy, and the track references it.
        let (img_path, content_img): (String, i64) = conn
            .query_row(
                "SELECT i.path, c.image_id FROM content c JOIN image i ON i.image_id = c.image_id",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(img_path, "/PIONEER/Artwork/00001/b1.jpg");
        assert_eq!(content_img, 1);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Round-trip through a real SQLCipher database: write a miniature
    /// Device Library Plus export with the production key, read it back
    /// through the production reader. Proves the key handling, the schema
    /// queries and the folder/ordering semantics in one go.
    #[test]
    fn reads_playlists_out_of_an_encrypted_device_library() {
        let dir = std::env::temp_dir().join(format!("ordnung-dlp-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("exportLibrary.db");

        {
            let conn = rusqlite::Connection::open(&db).unwrap();
            conn.execute_batch(&format!(
                "PRAGMA key = '{DLP_KEY}'; PRAGMA cipher_compatibility = 4;
                 CREATE TABLE playlist(playlist_id integer primary key, sequenceNo integer, \
                     name varchar, image_id integer, attribute integer, playlist_id_parent integer);
                 CREATE TABLE playlist_content(playlist_id integer, content_id integer, \
                     sequenceNo integer);
                 CREATE TABLE content(content_id integer primary key, path varchar);
                 INSERT INTO playlist VALUES (1, 1, 'CRATES', NULL, 1, 0);
                 INSERT INTO playlist VALUES (2, 2, 'warmup', NULL, 0, 1);
                 INSERT INTO playlist VALUES (3, 3, 'empty one', NULL, 0, 0);
                 INSERT INTO content VALUES (10, '/Contents/A/a.mp3');
                 INSERT INTO content VALUES (11, '/Contents/B/b.mp3');
                 -- Deliberately inserted out of order; sequenceNo decides.
                 INSERT INTO playlist_content VALUES (2, 11, 2);
                 INSERT INTO playlist_content VALUES (2, 10, 1);"
            ))
            .unwrap();
        }

        let got = read_playlists(&db).expect("read back");
        assert_eq!(got.playlists.len(), 3);
        let folder = &got.playlists[0];
        assert!(folder.is_folder);
        assert_eq!(folder.name, "CRATES");
        let leaf = got.playlists.iter().find(|p| p.id == 2).unwrap();
        assert_eq!(leaf.parent_id, 1);
        assert!(!leaf.is_folder);
        assert_eq!(
            got.entries_by_path[&2],
            vec!["/Contents/A/a.mp3".to_string(), "/Contents/B/b.mp3".into()]
        );
        // Present-but-empty, so the sidebar mirrors the player.
        assert_eq!(got.entries_by_path[&3], Vec::<String>::new());

        // Without the key the file must be unreadable — i.e. it really is
        // encrypted, not a plain SQLite file with a wishful pragma.
        let plain = rusqlite::Connection::open_with_flags(
            &db,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        assert!(plain
            .query_row("SELECT count(*) FROM sqlite_master", [], |r| r.get::<_, i64>(0))
            .is_err());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
