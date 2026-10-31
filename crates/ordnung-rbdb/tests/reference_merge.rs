//! A merge onto a stick rekordbox 7 wrote, replayed on an APFS clone of the
//! local reference stick (`testdata/rekordbox-reference/stop126`, gitignored:
//! it is a DJ's library). Skips when the reference is not on this machine.
//! What it proves that the synthetic round trips cannot: the stick's own
//! rows, My Tags, cues and dates come through a merge unchanged, the real
//! Device Library Plus schema takes `update_library`'s rows, and a track the
//! stick nests under `/Contents/<Artist>/<Album>/` is found by name.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use ordnung_core::model::{AudioProperties, Format, Tags, Track};
use ordnung_rbdb::export::{export_usb_with, ExportMode, ExportOptions};
use ordnung_rbdb::pdb;

fn reference() -> Option<PathBuf> {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/rekordbox-reference/stop126");
    p.join("PIONEER/rekordbox/exportLibrary.db").is_file().then_some(p)
}

fn track(id: u64, path: &Path, title: &str) -> Track {
    Track {
        id,
        source_path: path.to_string_lossy().into_owned(),
        format: Format::Mp3,
        properties: Some(AudioProperties {
            sample_rate_hz: 44_100,
            bit_depth: Some(16),
            channels: 2,
            duration_ms: 90_000,
            bitrate_kbps: Some(320),
        }),
        tags: Tags {
            title: Some(title.into()),
            artist: Some("Ord test".into()),
            ..Default::default()
        },
        analysis: None,
        cues: Vec::new(),
    }
}

#[test]
fn merge_onto_a_rekordbox_stick_keeps_its_rows_tags_and_cues() {
    let Some(reference) = reference() else {
        eprintln!("skipped: no reference stick at testdata/rekordbox-reference/stop126");
        return;
    };
    let root = std::env::temp_dir().join(format!("ordnung-refmerge-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let usb = root.join("usb");
    // An APFS clone of the whole stick: instant, and the reference stays as it is.
    let cloned = std::process::Command::new("cp")
        .args(["-Rc"])
        .arg(&reference)
        .arg(&usb)
        .status()
        .unwrap()
        .success();
    assert!(cloned, "cp -Rc of the reference stick");
    let rb = usb.join("PIONEER/rekordbox");
    let dlp = rb.join("exportLibrary.db");

    let before = pdb::read_stick(&usb).unwrap();
    let dlp_before = dlp_facts(&dlp);
    // One of the stick's own tracks, re-selected by file name from a
    // "library" copy of it, and one new track.
    let (nested_id, nested) = before
        .tracks
        .iter()
        .find(|(_, t)| t.file_path.matches('/').count() > 2)
        .map(|(id, t)| (*id, t.file_path.clone()))
        .expect("rekordbox nests its audio");
    let dat = usb.join(before.tracks[&nested_id].analyze_path.as_deref().unwrap().trim_start_matches('/'));
    let dat_before = std::fs::read(&dat).unwrap();
    let src = root.join("src");
    std::fs::create_dir_all(&src).unwrap();
    let same = src.join(nested.rsplit('/').next().unwrap());
    std::fs::copy(usb.join(nested.trim_start_matches('/')), &same).unwrap();
    let fresh = src.join("ord test track.mp3");
    std::fs::write(&fresh, vec![0x11u8; 30_000]).unwrap();
    let tracks = vec![track(1, &same, "Same"), track(2, &fresh, "Fresh")];

    let merge = ExportOptions { mode: ExportMode::Merge, ..Default::default() };
    let cancel = AtomicBool::new(false);
    let report = export_usb_with(&usb, &tracks, &[], merge, &mut |_| {}, &cancel).expect("merge onto the reference stick");
    assert_eq!(report.placed[0], (1, nested.clone()), "the re-selected track stays on its path");

    let after = pdb::read_stick(&usb).unwrap();
    assert_eq!(after.tracks.len(), before.tracks.len() + 1, "one new row, no duplicate");
    assert!(after.playlists == before.playlists, "the stick's playlists stay");
    // Every row the stick had reads back the same, the re-selected one
    // included: an Add never rewrites what the stick holds for a track.
    for (id, was) in &before.tracks {
        let now = after.tracks.get(id).expect("row kept");
        assert_eq!(view(now), view(was), "row {id} changed");
    }
    assert_eq!(std::fs::read(&dat).unwrap(), dat_before, "the stick's analysis file for a re-selected track stays");
    pdb::lint(&std::fs::read(rb.join("export.pdb")).unwrap()).expect("lint");
    let dlp_after = dlp_facts(&dlp);
    assert_eq!(dlp_after.my_tags, dlp_before.my_tags, "My Tag links");
    assert_eq!(dlp_after.cues_by_content, dlp_before.cues_by_content, "cues per track");
    assert_eq!(dlp_after.created, dlp_before.created, "property.createdDate");
    assert_eq!(dlp_after.contents, dlp_before.contents + 1);
    assert!(
        dlp_after.master_db_ids.iter().all(|m| dlp_before.master_db_ids.contains(m)),
        "no second library id appears in the DLP"
    );
    // Update from library is the explicit rewrite: the same track with
    // `refresh_existing` replaces the stick's analysis file and row.
    let refresh = ExportOptions { refresh_existing: true, ..merge };
    export_usb_with(&usb, &tracks[..1], &[], refresh, &mut |_| {}, &cancel).expect("update onto the reference stick");
    assert_ne!(std::fs::read(&dat).unwrap(), dat_before, "Update rewrites the analysis file");
    pdb::lint(&std::fs::read(rb.join("export.pdb")).unwrap()).expect("lint after update");
    let _ = std::fs::remove_dir_all(&root);
}

/// Every field of a row a player shows or rekordbox compares, as text;
/// the artwork path is resolved through the artwork table, not a row field.
fn view(t: &pdb::RbTrack) -> String {
    format!(
        "{:?}",
        (
            (&t.file_path, t.tempo_centi_bpm, &t.key, &t.title, &t.artist, &t.album, &t.genre, &t.label),
            (t.artwork_id, &t.comment, t.duration_s, t.bitrate_kbps, t.sample_rate_hz, t.year),
            (t.rating, t.color_id, t.track_number, t.disc_number, t.master_db_id, t.master_content_id),
            (&t.date_added, &t.remixer, &t.composer, &t.original_artist, &t.analyze_path),
        )
    )
}

struct DlpFacts {
    contents: i64,
    my_tags: i64,
    cues_by_content: BTreeMap<i64, i64>,
    created: String,
    master_db_ids: Vec<i64>,
}

fn dlp_facts(db: &Path) -> DlpFacts {
    let conn = rusqlite::Connection::open_with_flags(db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    conn.execute_batch(
        "PRAGMA key = 'r8gddnr4k847830ar6cqzbkk0el6qytmb3trbbx805jm74vez64i5o8fnrqryqls'; PRAGMA cipher_compatibility = 4;",
    )
    .unwrap();
    let one = |sql: &str| conn.query_row(sql, [], |r| r.get::<_, i64>(0)).unwrap();
    let mut stmt = conn.prepare("SELECT content_id, count(*) FROM cue GROUP BY content_id").unwrap();
    let cues_by_content = stmt
        .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))
        .unwrap()
        .flatten()
        .collect();
    let mut stmt = conn.prepare("SELECT DISTINCT masterDbId FROM content").unwrap();
    let master_db_ids = stmt.query_map([], |r| r.get::<_, i64>(0)).unwrap().flatten().collect();
    let created = conn
        .query_row("SELECT createdDate FROM property", [], |r| r.get::<_, String>(0))
        .unwrap_or_default();
    DlpFacts {
        contents: one("SELECT count(*) FROM content"),
        my_tags: one("SELECT count(*) FROM myTag_content"),
        cues_by_content,
        created,
        master_db_ids,
    }
}
