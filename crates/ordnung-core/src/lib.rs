//! Ordnung core: domain model and engines.
//!
//! This crate is a pure library — no UI, no policy, no process I/O decisions.
//! See the `ordnung-architecture` skill for the rules that keep it that way.
//!
//! Phase 0 ships the domain model and a fully-tested key/Camelot module. Engines
//! (`scan`, `analysis`, `tag`, `convert`, `catalog`) arrive in later phases.

pub mod analysis;
pub mod backup;
pub mod bandcamp;
pub mod catalog;
pub mod convert;
pub mod discogs;
pub mod genredb;
pub mod library_index;
pub mod error;
pub mod model;
pub mod scan;
pub mod search;
pub mod stretch;
pub mod tag;
pub mod tools;
pub mod tracklist;
pub mod update;
pub mod usb;

pub use catalog::{
    best_copy_index, AlbumSibling, Catalog, DuplicateGroup, DuplicateKind, MissingArtwork,
    MissingTrack, PlaylistStats, ScannedTrack,
};
pub use error::{Error, Result};
pub use model::key::{vinyl_pitch, Camelot, Key, Mode, PitchClass, VinylShift};
pub use model::{AudioProperties, Format, Playlist, Tags, Track, VinylList, VinylRecord};

/// The `User-Agent` every outbound request carries; Discogs rejects ureq's default.
pub const USER_AGENT: &str = "Ordnung/0.1 +https://kailazy.github.io/Ord/";
