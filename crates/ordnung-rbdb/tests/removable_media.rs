//! The round trips on a real FAT32 volume: a disk image macOS formats and
//! mounts the way it mounts a stick, so the export meets the msdos driver
//! (names listed in NFD, case folding, 2 s timestamps) instead of APFS.
//! Skips when hdiutil is missing or cannot attach here (another OS, a
//! locked-down runner); it never fails for want of a disk image.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::AtomicBool;

use ordnung_core::model::{AudioProperties, Format, Playlist, Tags, Track};
use ordnung_rbdb::export::{export_usb_with, path_key, ExportMode, ExportOptions};
use ordnung_rbdb::pdb;

fn hdiutil(args: &[&str]) -> Result<(), String> {
    match Command::new("hdiutil").args(args).output() {
        Ok(o) if o.status.success() => Ok(()),
        Ok(o) => Err(String::from_utf8_lossy(&o.stderr).trim().to_string()),
        Err(e) => Err(e.to_string()),
    }
}

/// A 256 MB FAT32 image attached under `root/mnt`; detached and deleted on
/// drop, a panic included, so no run leaves a volume mounted.
struct Volume {
    root: PathBuf,
}

impl Volume {
    fn attach() -> Option<Volume> {
        let volume = Volume { root: std::env::temp_dir().join(format!("ordnung-fat32-test-{}", std::process::id())) };
        let _ = std::fs::remove_dir_all(&volume.root);
        std::fs::create_dir_all(volume.mount()).unwrap();
        let (image, mount) = (volume.root.join("ord.dmg"), volume.mount());
        let (image, mount) = (image.to_str().unwrap(), mount.to_str().unwrap());
        let attached = hdiutil(&["create", "-fs", "MS-DOS FAT32", "-size", "256m", "-volname", "ORDTEST", image])
            .and_then(|_| hdiutil(&["attach", "-nobrowse", "-mountpoint", mount, image]));
        match attached {
            Ok(()) => Some(volume),
            Err(why) => {
                eprintln!("skipped: no FAT32 image here ({why})");
                None
            }
        }
    }

    fn mount(&self) -> PathBuf {
        self.root.join("mnt")
    }
}

impl Drop for Volume {
    fn drop(&mut self) {
        let _ = hdiutil(&["detach", "-force", self.mount().to_str().unwrap()]);
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn audio_file(dir: &Path, name: &str) -> PathBuf {
    let p = dir.join(name);
    std::fs::write(&p, vec![0xA5u8; 5_000]).unwrap();
    p
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
        tags: Tags { title: Some(title.into()), artist: Some("Ord test".into()), ..Default::default() },
        analysis: None,
        cues: Vec::new(),
    }
}

/// The names under `/Contents`, sorted.
fn contents(usb: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(usb.join("Contents"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// `PIONEER/rekordbox` holds the three databases and at most their `.prev`
/// siblings: no `.tmp` from the atomic writes, no `-wal`/`-shm` from SQLite.
fn only_databases(usb: &Path) {
    let rb = usb.join("PIONEER/rekordbox");
    const DBS: [&str; 3] = ["export.pdb", "exportExt.pdb", "exportLibrary.db"];
    for db in DBS {
        assert!(rb.join(db).is_file(), "{db} missing");
    }
    let stray: Vec<String> = std::fs::read_dir(&rb)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| !DBS.iter().any(|db| n == db || *n == format!("{db}.prev")))
        .collect();
    assert!(stray.is_empty(), "stray files beside the databases: {stray:?}");
}

#[test]
fn export_merge_and_replace_on_a_fat32_volume() {
    let Some(volume) = Volume::attach() else { return };
    let usb = volume.mount();
    let src = volume.root.join("src");
    std::fs::create_dir_all(&src).unwrap();
    let cancel = AtomicBool::new(false);
    let merge = ExportOptions { mode: ExportMode::Merge, ..Default::default() };

    // A fresh export reads back and passes the crate's lint.
    let alpha = audio_file(&src, "alpha.mp3");
    let beta = audio_file(&src, "beta.mp3");
    let tracks = vec![track(1, &alpha, "Alpha"), track(2, &beta, "Beta")];
    let set = vec![Playlist {
        id: 1,
        name: "set".into(),
        parent: None,
        is_folder: false,
        track_ids: vec![1, 2],
        icon: None,
        color: None,
    }];
    export_usb_with(&usb, &tracks, &set, ExportOptions::default(), &mut |_| {}, &cancel).expect("fresh export");
    let stick = pdb::read_stick(&usb).unwrap();
    assert_eq!((stick.tracks.len(), stick.playlists.len()), (2, 1));
    pdb::lint(&std::fs::read(usb.join("PIONEER/rekordbox/export.pdb")).unwrap()).expect("lint");
    only_databases(&usb);

    // A merge adding one track finds the stick's rows through the FAT names.
    let gamma = audio_file(&src, "gamma.mp3");
    let added = [track(3, &gamma, "Gamma"), track(1, &alpha, "Alpha")];
    let report = export_usb_with(&usb, &added, &[], merge, &mut |_| {}, &cancel).expect("merge");
    assert!(report.placed.contains(&(1, "/Contents/alpha.mp3".into())), "{:?}", report.placed);
    assert_eq!(pdb::read_stick(&usb).unwrap().tracks.len(), 3, "one new row, no duplicate");
    assert_eq!(contents(&usb), ["alpha.mp3", "beta.mp3", "gamma.mp3"]);
    only_databases(&usb);

    // An NFD source name lands as NFC, and a second merge of the same source
    // finds that file (the driver lists it in NFD) instead of a "(2)" copy.
    let nfd = audio_file(&src, "cafe\u{301}.mp3");
    let nfc = "/Contents/caf\u{e9}.mp3".to_string();
    for _ in 0..2 {
        let report = export_usb_with(&usb, &[track(4, &nfd, "Café")], &[], merge, &mut |_| {}, &cancel).expect("merge nfd");
        assert_eq!(report.placed, [(4, nfc.clone())]);
    }
    let stick = pdb::read_stick(&usb).unwrap();
    assert_eq!(stick.tracks.len(), 4, "the second merge found the first's row");
    assert_eq!(stick.tracks.values().find(|t| t.title == "Café").unwrap().file_path, nfc);
    assert!(usb.join(&nfc[1..]).is_file(), "the NFC path resolves");
    let cafes: Vec<String> = contents(&usb).into_iter().filter(|n| path_key(n).starts_with("caf")).collect();
    assert_eq!(cafes.len(), 1, "{cafes:?}");
    only_databases(&usb);

    // A Replace with alpha alone sweeps the files of the dropped tracks.
    export_usb_with(&usb, &tracks[..1], &[], ExportOptions::default(), &mut |_| {}, &cancel).expect("replace");
    assert_eq!(contents(&usb), ["alpha.mp3"]);
    assert_eq!(pdb::read_stick(&usb).unwrap().tracks.len(), 1);
    only_databases(&usb);
}
