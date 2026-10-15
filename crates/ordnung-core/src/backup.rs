//! Catalog backup and restore.
//!
//! Ord has no rekordbox to fall back on: the catalog (tracks, analysis,
//! artwork, playlists, crates, cues, the Discogs caches) lives in one SQLite
//! file, so one zip of it is the whole backup. The audio is not included.
//!
//! The zip holds a *consistent* snapshot of the database: `VACUUM INTO`
//! writes a fresh file with the write-ahead log folded in, which a raw copy
//! of `catalog.db` + `-wal` + `-shm` taken while the app runs cannot promise.
//! Next to it sit `config.toml` (for reference; a restore never touches it)
//! and `restore.sh`, so the backup can be put back by hand when the app
//! itself won't open. Zipping goes through the system `zip`/`unzip`: this is
//! a macOS-only app and both ship with the OS.

use crate::error::{Error, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

/// The catalog file inside a backup zip.
const DB_NAME: &str = "catalog.db";

/// Write a backup of the catalog at `db_path` to `dest_zip`. `config_path`
/// is copied in alongside when it exists.
pub fn backup(db_path: &Path, config_path: Option<&Path>, dest_zip: &Path) -> Result<()> {
    let stage = staging_dir(dest_zip)?;
    let result = (|| {
        let snapshot = stage.join(DB_NAME);
        let conn = rusqlite::Connection::open(db_path)?;
        conn.busy_timeout(std::time::Duration::from_secs(30))?;
        conn.execute("VACUUM INTO ?1", [snapshot.to_string_lossy().as_ref()])?;
        if let Some(cfg) = config_path.filter(|p| p.exists()) {
            std::fs::copy(cfg, stage.join("config.toml")).map_err(|e| io_err(cfg, e))?;
        }
        let script = stage.join("restore.sh");
        std::fs::write(&script, RESTORE_SH).map_err(|e| io_err(&script, e))?;
        let _ = std::fs::remove_file(dest_zip);
        // `-j` stores the files flat, so `restore.sh` sits next to `catalog.db`
        // wherever the zip is unpacked.
        let out = Command::new("/usr/bin/zip")
            .arg("-j")
            .arg("-q")
            .arg(dest_zip)
            .args([&snapshot, &script])
            .args(config_path.filter(|p| p.exists()).map(|_| stage.join("config.toml")))
            .output()
            .map_err(|e| io_err(dest_zip, e))?;
        if !out.status.success() {
            return Err(Error::Invalid(format!(
                "zip failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            )));
        }
        Ok(())
    })();
    let _ = std::fs::remove_dir_all(&stage);
    result
}

/// Put the catalog inside `zip_path` in place of the one at `db_path`. The
/// replaced files (`catalog.db` and its `-wal`/`-shm`) are moved, not
/// deleted, into a `before-restore-<stamp>` folder next to the catalog, and
/// its path is returned. The caller must have closed every connection it
/// holds: an open one keeps reading the moved file.
pub fn restore(zip_path: &Path, db_path: &Path) -> Result<PathBuf> {
    let stage = staging_dir(db_path)?;
    let result = (|| {
        let out = Command::new("/usr/bin/unzip")
            .arg("-o")
            .arg("-q")
            .arg(zip_path)
            .arg(DB_NAME)
            .arg("-d")
            .arg(&stage)
            .output()
            .map_err(|e| io_err(zip_path, e))?;
        let fresh = stage.join(DB_NAME);
        if !out.status.success() || !fresh.exists() {
            return Err(Error::Invalid(format!(
                "{} holds no {DB_NAME}: not an Ord backup",
                zip_path.display()
            )));
        }
        // Refuse a snapshot SQLite itself won't vouch for before touching
        // the live catalog.
        let conn = rusqlite::Connection::open_with_flags(
            &fresh,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )?;
        let check: String = conn.query_row("PRAGMA quick_check", [], |r| r.get(0))?;
        if check != "ok" {
            return Err(Error::Invalid(format!("the backup's catalog is damaged ({check})")));
        }
        drop(conn);

        let parent = db_path.parent().unwrap_or_else(|| Path::new("."));
        let park = parent.join(format!("before-restore-{}", stamp()));
        std::fs::create_dir_all(&park).map_err(|e| io_err(&park, e))?;
        let name = db_path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| DB_NAME.into());
        for suffix in ["", "-wal", "-shm"] {
            let live = parent.join(format!("{name}{suffix}"));
            if live.exists() {
                std::fs::rename(&live, park.join(format!("{name}{suffix}")))
                    .map_err(|e| io_err(&live, e))?;
            }
        }
        std::fs::rename(&fresh, db_path)
            .or_else(|_| std::fs::copy(&fresh, db_path).map(|_| ()))
            .map_err(|e| io_err(db_path, e))?;
        Ok(park)
    })();
    let _ = std::fs::remove_dir_all(&stage);
    result
}

/// A fresh scratch folder next to `near` (same volume, so the final rename
/// is a rename).
fn staging_dir(near: &Path) -> Result<PathBuf> {
    let parent = near.parent().unwrap_or_else(|| Path::new("."));
    let dir = parent.join(format!(".ord-backup-{}-{}", stamp(), std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| io_err(&dir, e))?;
    Ok(dir)
}

fn io_err(path: &Path, source: std::io::Error) -> Error {
    Error::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// `YYYYMMDD-HHMMSS` in UTC: a sortable stamp for file names, without a
/// calendar dependency (the civil-from-days arithmetic below is the
/// standard proleptic-Gregorian conversion).
pub fn stamp() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (days, rem) = (secs / 86_400, secs % 86_400);
    let (h, m, s) = (rem / 3600, rem % 3600 / 60, rem % 60);
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mo <= 2 { y + 1 } else { y };
    format!("{y:04}{mo:02}{d:02}-{h:02}{m:02}{s:02}")
}

const RESTORE_SH: &str = r#"#!/bin/sh
# Put this Ord catalog backup back by hand (for when the app won't open).
#   sh restore.sh              restores into ~/.ordnung
#   ORDNUNG_DIR=/x sh restore.sh   restores into /x
# The catalog currently there is moved, not deleted, into a
# before-restore-<stamp> folder next to it. config.toml is left alone.
set -e
DIR="${ORDNUNG_DIR:-$HOME/.ordnung}"
HERE="$(cd "$(dirname "$0")" && pwd)"
if pgrep -x Ord >/dev/null 2>&1; then echo "Quit Ord first."; exit 1; fi
[ -f "$HERE/catalog.db" ] || { echo "No catalog.db next to this script."; exit 1; }
STAMP="$(date +%Y%m%d-%H%M%S)"
PARK="$DIR/before-restore-$STAMP"
mkdir -p "$PARK"
for f in catalog.db catalog.db-wal catalog.db-shm; do
  [ -e "$DIR/$f" ] && mv "$DIR/$f" "$PARK/$f"
done
cp "$HERE/catalog.db" "$DIR/catalog.db"
echo "Restored the catalog into $DIR. The previous one is in $PARK."
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backup_then_restore_round_trips_and_parks_the_old_catalog() {
        let dir = std::env::temp_dir().join(format!("ord-backup-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("catalog.db");
        {
            let c = rusqlite::Connection::open(&db).unwrap();
            c.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE t(x); INSERT INTO t VALUES (1);")
                .unwrap();
        }
        let cfg = dir.join("config.toml");
        std::fs::write(&cfg, "library_root = \"/x\"\n").unwrap();
        let zip = dir.join("backup.zip");
        backup(&db, Some(&cfg), &zip).unwrap();
        assert!(zip.exists());

        // Change the live catalog after the backup, then restore over it.
        {
            let c = rusqlite::Connection::open(&db).unwrap();
            c.execute_batch("INSERT INTO t VALUES (2);").unwrap();
        }
        let park = restore(&zip, &db).unwrap();
        assert!(park.join("catalog.db").exists(), "old catalog parked");
        let c = rusqlite::Connection::open(&db).unwrap();
        let n: i64 = c.query_row("SELECT count(*) FROM t", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1, "restored catalog is the snapshot");
        assert!(dir.join("config.toml").exists());
        assert!(!dir.read_dir().unwrap().flatten().any(|e| e.file_name().to_string_lossy().starts_with(".ord-backup-")));
        assert!(stamp().len() == 15);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn restore_refuses_a_zip_without_a_catalog() {
        let dir = std::env::temp_dir().join(format!("ord-backup-test2-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let junk = dir.join("junk.txt");
        std::fs::write(&junk, "x").unwrap();
        let zip = dir.join("junk.zip");
        assert!(Command::new("/usr/bin/zip").arg("-j").arg("-q").arg(&zip).arg(&junk).status().unwrap().success());
        let db = dir.join("catalog.db");
        std::fs::write(&db, "live").unwrap();
        assert!(restore(&zip, &db).is_err());
        assert_eq!(std::fs::read_to_string(&db).unwrap(), "live", "live catalog untouched");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
