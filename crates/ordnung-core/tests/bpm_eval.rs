//! BPM-detection accuracy regression test against rekordbox ground truth.
//!
//! Runs the *production* `tempo::detect` over the same 79-track labelled sample set
//! as `key_eval` (`common::GROUND_TRUTH`), grading each detected BPM against the
//! rekordbox value the user transcribed (the `BPM↣rb` column of KEY_CHECK.md). Asserts the within-2-BPM and
//! modulo-octave rates stay at or above the calibrated baseline, so re-enabling and
//! later tuning the tempo path can't silently regress it.
//!
//! Grading mirrors KEY_CHECK.md's BPM flag:
//!   ok  — within `BPM_TOL` of rekordbox
//!   8ve — within tolerance of half or double rekordbox (octave error)
//!   X   — otherwise
//!
//! This covers *tempo*; first-downbeat placement gets its own ground truth and
//! grading once dedicated downbeat detection lands (it needs rekordbox anchor ms,
//! not in KEY_CHECK.md). The grid is spot-checked for consistency below.
//!
//! Run: cargo test -p ordnung-core --test bpm_eval --release -- --ignored --nocapture

mod common;

use common::GROUND_TRUTH;
use ordnung_core::analysis::{decode_mono_capped, downbeat, dsp, tempo};
use std::path::PathBuf;

/// Tolerance for an "ok" match (rekordbox rounds to whole BPM; our lock is <0.5 off
/// on steady material, so 2 BPM absorbs rounding + genuine near-misses like KEY_CHECK).
const BPM_TOL: f32 = 2.0;

#[derive(PartialEq, Clone, Copy)]
enum Grade {
    Ok,
    Octave,
    Miss,
}

fn grade(got: f32, want: f32) -> Grade {
    if (got - want).abs() <= BPM_TOL {
        Grade::Ok
    } else if (got - want * 2.0).abs() <= BPM_TOL || (got - want * 0.5).abs() <= BPM_TOL {
        Grade::Octave
    } else {
        Grade::Miss
    }
}

/// Calibrated floors. v16 (re-enabled tempo path) measured 72 within 2 BPM / 73
/// modulo octave here; v18 (metrical correction, `tempo::correct_metrical`) lifted it
/// to 75 / 76 by recovering the 3:2 & 5:4 slow folds. Floors sit a few tracks below
/// the v18 baseline (float/decoder cushion); assert `>=` so accuracy can improve
/// freely but a regression fails the build.
const MIN_OK: usize = 72;
const MIN_MODULO_OCTAVE: usize = 74;

#[test]
#[ignore = "decodes the sample set; run with --ignored --nocapture"]
fn bpm_accuracy_regression() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/seeker-sample");
    if !dir.is_dir() {
        eprintln!("skip: no sample dir");
        return;
    }

    let (mut ok, mut octave, mut miss, mut total) = (0, 0, 0, 0);
    let mut rows: Vec<(char, &str, f32, f32)> = Vec::new();

    for &(needle, _, want) in GROUND_TRUTH {
        let path = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .find(|p| p.file_name().map_or(false, |n| n.to_string_lossy().contains(needle)));
        let Some(path) = path else {
            panic!("no file matches ground-truth needle {needle:?}");
        };
        // Match analyze_file's window: the first 150 s of the decode.
        let Ok(audio) = decode_mono_capped(&path, Some(150 * 48_000)) else {
            eprintln!("decode failed: {needle:?}");
            continue;
        };
        let spec = dsp::spectrogram(&audio.samples, audio.sample_rate);
        let got = tempo::detect(&spec).bpm;
        total += 1;
        let g = grade(got, want);
        match g {
            Grade::Ok => ok += 1,
            Grade::Octave => octave += 1,
            Grade::Miss => miss += 1,
        }
        let mark = match g {
            Grade::Ok => 'o',
            Grade::Octave => '8',
            Grade::Miss => 'X',
        };
        rows.push((mark, needle, got, want));
    }

    let modulo_octave = ok + octave;
    println!("\n--- per-track (o within 2 BPM · 8 half/double · X miss) ---");
    for (mark, needle, got, want) in &rows {
        println!("  {} {:<28} {:>6.1} -> {:>5.0}", mark, needle, got, want);
    }
    println!(
        "\nwithin 2 BPM {}/{} ({}%)  modulo octave {} ({}%)  [o{} 8{} X{}]",
        ok,
        total,
        100 * ok / total,
        modulo_octave,
        100 * modulo_octave / total,
        ok,
        octave,
        miss,
    );

    assert!(
        ok >= MIN_OK,
        "BPM within-2 regressed: {ok} < {MIN_OK} (floor); see KEY_CHECK.md"
    );
    assert!(
        modulo_octave >= MIN_MODULO_OCTAVE,
        "BPM modulo-octave regressed: {modulo_octave} < {MIN_MODULO_OCTAVE} (floor)"
    );
}

/// Diagnostic (not a pass/fail gate): print the detected bar phase for a spread of
/// real tracks, so a regression that collapses `detect_phase` to a constant is
/// visible by eye. Real downbeat *accuracy* needs rekordbox anchor ground truth
/// (not in KEY_CHECK.md) — this only sanity-checks that the cue responds to audio.
#[test]
#[ignore = "decodes sample tracks; run with --ignored --nocapture"]
fn downbeat_phase_probe() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/seeker-sample");
    if !dir.is_dir() {
        eprintln!("skip: no sample dir");
        return;
    }
    let mut counts = [0usize; 4];
    println!("\n--- detected bar phase (0 = first detected beat is the downbeat) ---");
    for &(needle, _, _) in GROUND_TRUTH {
        let path = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .find(|p| p.file_name().map_or(false, |n| n.to_string_lossy().contains(needle)));
        let Some(path) = path else { continue };
        let Ok(audio) = decode_mono_capped(&path, Some(150 * 48_000)) else { continue };
        let spec = dsp::spectrogram(&audio.samples, audio.sample_rate);
        let t = tempo::detect(&spec);
        if t.bpm <= 0.0 {
            continue;
        }
        let phase = downbeat::detect_phase(&spec, t.bpm, t.beat_offset_ms);
        counts[phase as usize] += 1;
        println!("  phase {} {:<28} ({:.0} bpm)", phase, needle, t.bpm);
    }
    println!("\nphase distribution: {counts:?}  (a healthy detector is not all one bucket)");
}

/// End-to-end wiring check: `analyze_file` (decode → tempo → grid) emits a BPM and a
/// populated, evenly-spaced static grid that spans the track. Guards the v16 wiring
/// against the v8 regression where the grid was left anchorless.
#[test]
#[ignore = "decodes a sample track; run with --ignored --nocapture"]
fn analyze_file_emits_static_grid() {
    use ordnung_core::analysis::analyze_file;

    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/seeker-sample");
    if !dir.is_dir() {
        eprintln!("skip: no sample dir");
        return;
    }
    // A steady 4/4 track with a confident lock (rekordbox 130, detected 130).
    let path = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| p.file_name().map_or(false, |n| n.to_string_lossy().contains("02. Lava")))
        .expect("Lava sample present");

    let a = analyze_file(&path).expect("analyze");
    let bpm = a.bpm.expect("bpm emitted");
    assert!((bpm - 130.0).abs() < 2.0, "bpm {bpm}");
    assert!(a.beatgrid.beats.len() > 100, "grid populated: {}", a.beatgrid.beats.len());

    // Consecutive beats are one tempo period apart, all tagged the global BPM.
    let period_ms = 60_000.0 / bpm as f64;
    for w in a.beatgrid.beats.windows(2) {
        let gap = w[1].position_ms as f64 - w[0].position_ms as f64;
        assert!((gap - period_ms).abs() <= 1.0, "gap {gap} vs {period_ms}");
        assert!((w[0].bpm - bpm).abs() < 1e-6);
    }
    // Bar numbers cycle 1..=4.
    assert!(a.beatgrid.beats.iter().all(|b| (1..=4).contains(&b.number)));
    println!(
        "analyze_file: bpm {:.1}, {} beats, last @ {} ms",
        bpm,
        a.beatgrid.beats.len(),
        a.beatgrid.beats.last().unwrap().position_ms
    );
}
