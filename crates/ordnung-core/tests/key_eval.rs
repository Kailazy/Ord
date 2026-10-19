//! Key-detection accuracy regression test against rekordbox ground truth.
//!
//! Runs the *production* `key::detect` over the 79-track labelled sample set
//! (`common::GROUND_TRUTH`) and grades each call against the rekordbox Camelot the
//! user transcribed (KEY_CHECK.md).
//! Asserts the exact/compatible rates stay at or above the calibrated baseline, so a
//! future chroma/profile change can't silently regress key accuracy.
//!
//! Run: cargo test -p ordnung-core --test key_eval --release -- --ignored --nocapture

mod common;

use common::GROUND_TRUTH;
use ordnung_core::analysis::{decode_mono_capped, dsp, key};
use ordnung_core::model::key::{Camelot, Key, Mode};
use std::path::PathBuf;

#[derive(PartialEq, Clone, Copy)]
enum Grade {
    Exact,
    Rel, // relative major/minor (same Camelot number)
    Adj, // adjacent number, same side
    X,
}

fn grade(got: Camelot, want: Camelot) -> Grade {
    if got.number == want.number && got.major == want.major {
        Grade::Exact
    } else if got.number == want.number {
        Grade::Rel
    } else if got.major == want.major {
        let d = (got.number as i8 - want.number as i8).rem_euclid(12);
        if d == 1 || d == 11 {
            Grade::Adj
        } else {
            Grade::X
        }
    } else {
        Grade::X
    }
}

/// Calibrated floors on analyzer v9 (see KEY_CHECK.md). Assert >= so accuracy can
/// improve freely but a regression below the established baseline fails the build.
const MIN_EXACT: usize = 27;
const MIN_COMPATIBLE: usize = 40;

#[test]
#[ignore = "decodes the sample set; run with --ignored --nocapture"]
fn key_accuracy_regression() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/seeker-sample");
    if !dir.is_dir() {
        eprintln!("skip: no sample dir");
        return;
    }

    let (mut exact, mut rel, mut adj, mut x, mut none, mut minor, mut total) =
        (0, 0, 0, 0, 0, 0, 0);
    let mut rows: Vec<(char, &str, String, &str)> = Vec::new();

    for &(needle, gt, _) in GROUND_TRUTH {
        let path = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .find(|p| p.file_name().map_or(false, |n| n.to_string_lossy().contains(needle)));
        let Some(path) = path else {
            panic!("no file matches ground-truth needle {needle:?}");
        };
        let Ok(audio) = decode_mono_capped(&path, Some(150 * 48_000)) else {
            eprintln!("decode failed: {needle:?}");
            continue;
        };
        let spec = dsp::spectrogram(&audio.samples, audio.sample_rate);
        total += 1;
        let want = Key::parse(gt).unwrap().camelot();
        let (label, g) = match key::detect(&spec) {
            Some(k) => {
                if k.mode == Mode::Minor {
                    minor += 1;
                }
                (k.camelot().label(), grade(k.camelot(), want))
            }
            None => {
                none += 1;
                ("-".to_string(), Grade::X)
            }
        };
        match g {
            Grade::Exact => exact += 1,
            Grade::Rel => rel += 1,
            Grade::Adj => adj += 1,
            Grade::X => x += 1,
        }
        let mark = match g {
            Grade::Exact => 'E',
            Grade::Rel => 'r',
            Grade::Adj => 'a',
            Grade::X => 'X',
        };
        rows.push((mark, needle, label, gt));
    }

    let compat = exact + rel + adj;
    println!("\n--- per-track (E exact · r relative · a adjacent · X miss) ---");
    for (mark, needle, got, gt) in &rows {
        println!("  {} {:<28} {:>4} -> {:>3}", mark, needle, got, gt);
    }
    println!(
        "\nexact {}/{} ({}%)  compatible {} ({}%)  [E{} r{} a{} X{} none{}]  minor {}/{}",
        exact, total, 100 * exact / total, compat, 100 * compat / total,
        exact, rel, adj, x, none, minor, total,
    );

    assert!(
        exact >= MIN_EXACT,
        "key exact regressed: {exact} < {MIN_EXACT} (floor); see KEY_CHECK.md"
    );
    assert!(
        compat >= MIN_COMPATIBLE,
        "key compatibility regressed: {compat} < {MIN_COMPATIBLE} (floor)"
    );
}
