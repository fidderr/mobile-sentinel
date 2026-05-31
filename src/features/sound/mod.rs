//! Sound Library — bundled + custom + system-default sound resolution.
//!
//! A standalone capability (the `sound-library` feature): it knows nothing
//! about recipes. The library is constructed with a [`SoundBackend`] for
//! system-default URI lookup; on Android this is implemented by the JNI
//! layer, in tests by a mock. The recipe layer adapts its own context sound
//! type onto this library's [`SoundId`] at its own boundary.

pub mod library;
pub mod sound_id;

pub use library::{SoundBackend, SoundEntry, SoundError, SoundLibrary};
pub use sound_id::SoundId;
