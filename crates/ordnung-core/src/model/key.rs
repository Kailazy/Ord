//! Musical key: canonical storage + Camelot / Open Key / classical rendering.
//!
//! Camelot is the display contract (see the `audio-analysis` skill). Keys are
//! stored canonically as `(PitchClass, Mode)` and rendered on demand. The Camelot
//! number and Open Key number are derived, never stored.

/// Pitch class, C = 0 .. B = 11 (chromatic, sharps preferred internally).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PitchClass(pub u8);

impl PitchClass {
    pub fn new(semitone: u8) -> Self {
        PitchClass(semitone % 12)
    }

    /// Classical name (sharp spelling).
    pub fn name(self) -> &'static str {
        const NAMES: [&str; 12] = [
            "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
        ];
        NAMES[(self.0 % 12) as usize]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Major,
    Minor,
}

/// A musical key. Canonical: pitch class of the tonic + mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Key {
    pub tonic: PitchClass,
    pub mode: Mode,
}

/// A Camelot wheel position: number 1..=12 and side (A = minor, B = major).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Camelot {
    pub number: u8,
    pub major: bool,
}

/// A turntable's pitch fader, ±8%, as a fraction. Deliberately not the app
/// player's `PITCH_RANGE_PCT` (ordnung-gui `audio.rs`): widening that fader
/// must not change which records count as mixable on a deck.
const VINYL_PITCH_RANGE: f32 = 0.08;
/// How far off a semitone a pitched record may land and still count as in key.
const VINYL_DETUNE_CENTS: f32 = 30.0;

/// How far a record is pitched, relative to the playing deck, to beatmatch it,
/// and by how many semitones that moves it.
#[derive(Debug, Clone, Copy)]
pub struct VinylShift {
    pub pitch_pct: f32,
    pub semitones: i32,
}

/// The pitch, in percent, a turntable needs to beatmatch a record tagged
/// `bpm` to a deck running at `playing_bpm`, trying straight, double and
/// half time. `None` when no fold lands within the fader's reach.
pub fn vinyl_pitch(bpm: f32, playing_bpm: f32) -> Option<f32> {
    // Also rejects NaN; an infinite BPM fails the range check below.
    if !(bpm > 0.0 && playing_bpm > 0.0) {
        return None;
    }
    [1.0, 2.0, 0.5]
        .into_iter()
        .map(|k| playing_bpm / (k * bpm))
        .find(|r| (r - 1.0).abs() <= VINYL_PITCH_RANGE)
        .map(|rho| (rho - 1.0) * 100.0)
}

impl Camelot {
    /// "8B", "5A", etc.
    pub fn label(self) -> String {
        format!("{}{}", self.number, if self.major { 'B' } else { 'A' })
    }

    /// Harmonic-mixing compatibility between two wheel positions: same number
    /// (same key or relative major/minor) or one step around the wheel on the
    /// same side.
    pub fn compatible_with(self, other: Camelot) -> bool {
        if self.number == other.number {
            return true;
        }
        if self.major == other.major {
            let diff = (self.number as i8 - other.number as i8).rem_euclid(12);
            return diff == 1 || diff == 11;
        }
        false
    }

    /// Shift by `semitones` (any sign): +1 st = +7 wheel positions, side kept.
    pub fn transpose(self, semitones: i32) -> Camelot {
        let number = (self.number as i32 - 1 + 7 * semitones).rem_euclid(12) as u8 + 1;
        Camelot { number, major: self.major }
    }

    /// Vinyl counterpart of `compatible_with`: `self` at `bpm`, beatmatched on a
    /// turntable to `playing` at `playing_bpm`. `Some` iff it lands in key.
    pub fn vinyl_compatible_with(
        self,
        bpm: f32,
        playing: Camelot,
        playing_bpm: f32,
    ) -> Option<VinylShift> {
        let pitch_pct = vinyl_pitch(bpm, playing_bpm)?;
        let d = 12.0 * (1.0 + pitch_pct / 100.0).log2();
        let n = d.round();
        if (100.0 * (d - n)).abs() > VINYL_DETUNE_CENTS {
            return None;
        }
        let n = n as i32;
        self.transpose(n)
            .compatible_with(playing)
            .then_some(VinylShift { pitch_pct, semitones: n })
    }
}

impl Key {
    pub fn new(tonic: PitchClass, mode: Mode) -> Self {
        Key { tonic, mode }
    }

    /// Camelot number for a *major* pitch class: stepping up a fifth (+7 semitones)
    /// advances the number by one; C major = 8B.
    fn major_number(pc: u8) -> u8 {
        // fifths-from-C = pc * 7 (mod 12), since 7 is its own inverse mod 12.
        ((7 + (pc % 12) * 7) % 12) + 1
    }

    /// Camelot wheel position.
    pub fn camelot(self) -> Camelot {
        match self.mode {
            Mode::Major => Camelot {
                number: Self::major_number(self.tonic.0),
                major: true,
            },
            // A minor key shares its Camelot number with its relative major,
            // whose tonic is a minor third (+3 semitones) up.
            Mode::Minor => Camelot {
                number: Self::major_number((self.tonic.0 + 3) % 12),
                major: false,
            },
        }
    }

    /// Default display: Camelot label.
    pub fn display(self) -> String {
        self.camelot().label()
    }

    /// Harmonic-mixing compatibility: same key, ±1 on the wheel, or relative
    /// major/minor (same number, opposite side). See [`Camelot::compatible_with`].
    pub fn compatible_with(self, other: Key) -> bool {
        self.camelot().compatible_with(other.camelot())
    }

    /// Parse a key from the notations found in the wild — classical names
    /// ("Am", "F♯m", "Bb", "C minor", "Dmaj"), Camelot labels ("8A") and Open
    /// Key labels ("1d", "10m") — into the canonical form. This is how key
    /// strings from *outside* (file tags, a rekordbox export's Keys table)
    /// enter the model; unparseable input is `None`, never a guess.
    pub fn parse(s: &str) -> Option<Key> {
        let s = s.trim();
        if s.is_empty() {
            return None;
        }
        // Wheel forms: digits then a side letter — Camelot A/B or Open Key
        // d/m. Tried first because a note-letter parse would misread "8A"
        // starting from its digit anyway. ('m' can only be a wheel side here,
        // not a minor suffix, because the prefix must be all digits.)
        let last = s.chars().last()?;
        if matches!(last, 'A' | 'B' | 'a' | 'b' | 'd' | 'D' | 'm' | 'M')
            && !s[..s.len() - 1].is_empty()
            && s[..s.len() - 1].chars().all(|c| c.is_ascii_digit())
        {
            let number: u8 = s[..s.len() - 1].parse().ok()?;
            if !(1..=12).contains(&number) {
                return None;
            }
            // Open Key is the same wheel rotated so Camelot 8 reads as 1; fold
            // it onto the Camelot number first.
            let (number, major) = match last.to_ascii_uppercase() {
                'B' => (number, true),
                'A' => (number, false),
                'D' => ((number + 6) % 12 + 1, true),
                _ => ((number + 6) % 12 + 1, false),
            };
            // Invert `major_number` (multiplying by 7 is its own inverse mod
            // 12): the major tonic for this wheel number, and the relative
            // minor a minor third below it.
            let major_pc = (7 * (number as i16 - 8)).rem_euclid(12) as u8;
            return Some(if major {
                Key::new(PitchClass::new(major_pc), Mode::Major)
            } else {
                Key::new(PitchClass::new((major_pc + 9) % 12), Mode::Minor)
            });
        }
        // Classical form: note letter, optional accidental, optional mode word.
        let mut chars = s.chars();
        let letter = chars.next()?.to_ascii_uppercase();
        let base = match letter {
            'C' => 0u8,
            'D' => 2,
            'E' => 4,
            'F' => 5,
            'G' => 7,
            'A' => 9,
            'B' => 11,
            _ => return None,
        };
        let rest: String = chars.collect();
        let (accidental, rest) = match rest.chars().next() {
            Some('#') | Some('♯') => (1i8, &rest[rest.chars().next()?.len_utf8()..]),
            Some('b') | Some('♭') => (-1i8, &rest[rest.chars().next()?.len_utf8()..]),
            _ => (0i8, rest.as_str()),
        };
        let pc = PitchClass::new((base as i8 + accidental).rem_euclid(12) as u8);
        let mode = match rest
            .trim()
            .trim_start_matches(['-', ' '])
            .to_ascii_lowercase()
            .as_str()
        {
            "" | "maj" | "major" => Mode::Major,
            "m" | "min" | "minor" => Mode::Minor,
            _ => return None,
        };
        Some(Key::new(pc, mode))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(name: u8, mode: Mode) -> Key {
        Key::new(PitchClass::new(name), mode)
    }

    /// `parse` accepts the notations key strings actually arrive in — file
    /// tags and rekordbox Keys-table names — and lands them on the canonical
    /// form; junk stays `None` rather than becoming a wrong key.
    #[test]
    fn parse_reads_classical_and_camelot_notations() {
        assert_eq!(Key::parse("Am"), Some(key(9, Mode::Minor)));
        assert_eq!(Key::parse("C"), Some(key(0, Mode::Major)));
        assert_eq!(Key::parse("F#m"), Some(key(6, Mode::Minor)));
        assert_eq!(Key::parse("Bb"), Some(key(10, Mode::Major)));
        assert_eq!(Key::parse("B"), Some(key(11, Mode::Major)));
        assert_eq!(Key::parse("E♭m"), Some(key(3, Mode::Minor)));
        assert_eq!(Key::parse("C minor"), Some(key(0, Mode::Minor)));
        assert_eq!(Key::parse("Dmaj"), Some(key(2, Mode::Major)));
        // Camelot and Open Key labels round-trip through their own renderers.
        assert_eq!(Key::parse("8A"), Some(key(9, Mode::Minor)));
        assert_eq!(Key::parse("8B"), Some(key(0, Mode::Major)));
        assert_eq!(Key::parse("11a"), Some(key(6, Mode::Minor)));
        assert_eq!(Key::parse("1d"), Some(key(0, Mode::Major)));
        assert_eq!(Key::parse("10m"), Some(key(0, Mode::Minor)));
        for pc in 0..12 {
            for mode in [Mode::Major, Mode::Minor] {
                let k = key(pc, mode);
                assert_eq!(Key::parse(&k.camelot().label()), Some(k));
            }
        }
        assert_eq!(Key::parse(""), None);
        assert_eq!(Key::parse("13A"), None);
        assert_eq!(Key::parse("0B"), None);
        assert_eq!(Key::parse("Hm"), None);
        assert_eq!(Key::parse("Applause"), None);
    }

    #[test]
    fn camelot_reference_points() {
        // C major = 8B, A minor = 8A (relative), G major = 9B, E minor = 9A.
        assert_eq!(key(0, Mode::Major).camelot().label(), "8B");
        assert_eq!(key(9, Mode::Minor).camelot().label(), "8A");
        assert_eq!(key(7, Mode::Major).camelot().label(), "9B");
        assert_eq!(key(4, Mode::Minor).camelot().label(), "9A");
        // A major = 11B, F# minor = 11A.
        assert_eq!(key(9, Mode::Major).camelot().label(), "11B");
        assert_eq!(key(6, Mode::Minor).camelot().label(), "11A");
    }

    #[test]
    fn camelot_numbers_cover_one_to_twelve() {
        let mut seen = std::collections::BTreeSet::new();
        for pc in 0..12 {
            seen.insert(key(pc, Mode::Major).camelot().number);
        }
        assert_eq!(
            seen.len(),
            12,
            "all 12 Camelot numbers present for major keys"
        );
        assert!(seen.iter().all(|&n| (1..=12).contains(&n)));
    }

    #[test]
    fn harmonic_compatibility() {
        let am = key(9, Mode::Minor); // 8A
        assert!(am.compatible_with(key(0, Mode::Major))); // relative major C (8B)
        assert!(am.compatible_with(key(4, Mode::Minor))); // 9A, +1
        assert!(am.compatible_with(key(2, Mode::Minor))); // 7A, -1
        assert!(!am.compatible_with(key(6, Mode::Minor))); // 11A, not adjacent
    }

    #[test]
    fn transpose_walks_the_wheel() {
        let c = |number: u8, major: bool| Camelot { number, major };
        for number in 1..=12 {
            for major in [false, true] {
                let k = c(number, major);
                assert_eq!(k.transpose(12), k);
                assert_eq!(k.transpose(-12), k);
                // +7 st is one step up the wheel, 12 wrapping to 1.
                assert_eq!(k.transpose(7), c(number % 12 + 1, major));
            }
        }
        assert_eq!(c(8, true).transpose(1), c(3, true)); // 8B + 1 st wraps to 3B
    }

    #[test]
    fn vinyl_compatibility_follows_the_pitch_fader() {
        let c = |number: u8, major: bool| Camelot { number, major };
        let a = |number: u8| c(number, false);
        // (candidate, its bpm, playing, its bpm) -> (semitones, pitch %)
        let shift = |cand: Camelot, bpm: f32, playing: Camelot, playing_bpm: f32| {
            cand.vinyl_compatible_with(bpm, playing, playing_bpm)
                .map(|s| (s.semitones, s.pitch_pct))
        };
        // Pitching 117 up to 124 is a semitone (d = +1.006): the paper match
        // 7A moves off, the paper miss 1A lands on 8A.
        assert_eq!(shift(a(7), 117.0, a(8), 124.0), None);
        let (n, pct) = shift(a(1), 117.0, a(8), 124.0).unwrap();
        assert_eq!(n, 1);
        assert!((pct - 5.98).abs() < 0.01, "{pct}");
        // Slowing 124 down to 117 is a semitone the other way.
        let (n, pct) = shift(a(3), 124.0, a(8), 117.0).unwrap();
        assert_eq!(n, -1);
        assert!((pct + 5.645).abs() < 0.01, "{pct}");
        // 1A pitched up to 8A is the relative of 8B.
        assert_eq!(shift(a(1), 117.0, c(8, true), 124.0).map(|s| s.0), Some(1));
        // Same tempo, double and half time: no shift.
        assert_eq!(shift(a(9), 124.0, a(8), 124.0), Some((0, 0.0)));
        assert_eq!(shift(a(8), 85.0, a(8), 170.0).map(|s| s.0), Some(0));
        assert_eq!(shift(a(8), 170.0, a(8), 85.0).map(|s| s.0), Some(0));
        // +12.7% is past the fader.
        assert_eq!(shift(a(8), 110.0, a(8), 124.0), None);
        // 29 cents sharp or flat of the semitone is still in key, 31 is not.
        for bpm in [118.007, 122.027] {
            assert_eq!(shift(a(8), bpm, a(8), 120.0).map(|s| s.0), Some(0), "{bpm}");
        }
        for bpm in [117.870, 122.168] {
            assert_eq!(shift(a(8), bpm, a(8), 120.0), None, "{bpm}");
        }
        // A BPM that is not a positive finite number, on either deck, never matches.
        for bad in [0.0, -120.0, f32::NAN, f32::INFINITY] {
            assert_eq!(shift(a(8), bad, a(8), 124.0), None, "{bad}");
            assert_eq!(shift(a(8), 124.0, a(8), bad), None, "{bad}");
        }
        assert_eq!(shift(a(8), -117.0, a(8), -124.0), None);
        // The fader alone, for the player: straight, folded, out of reach, bad.
        assert!((vinyl_pitch(117.0, 124.0).unwrap() - 5.98).abs() < 0.01);
        assert_eq!(vinyl_pitch(85.0, 170.0), Some(0.0));
        assert_eq!(vinyl_pitch(110.0, 124.0), None);
        assert_eq!(vinyl_pitch(f32::NAN, 124.0), None);
    }

    #[test]
    fn camelot_compatibility_wraps_and_respects_side() {
        let c = |number: u8, major: bool| Camelot { number, major };
        assert!(c(12, false).compatible_with(c(1, false))); // wheel wraps 12 -> 1
        assert!(c(1, false).compatible_with(c(12, false)));
        assert!(c(8, false).compatible_with(c(8, true))); // relative major/minor
        assert!(!c(8, false).compatible_with(c(9, true))); // +1 across sides
        assert!(!c(3, true).compatible_with(c(5, true))); // two steps away
    }
}
