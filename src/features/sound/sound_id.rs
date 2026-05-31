//! Stable sound identifiers.
//!
//! A sound is referenced by [`SoundId`], never by file path. Resolution to a
//! playable URI happens via the [`crate::sound::library::SoundLibrary`]. This
//! type is fully standalone — it knows nothing about recipes; the recipe
//! layer maps its own context sound type to/from `SoundId` at its boundary.

use serde::{Deserialize, Serialize};

/// Stable identifier for a configured sound.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum SoundId {
    /// Bundled APK asset under the consumer's sounds directory.
    /// Identifier is the file stem (e.g. `"happy"`).
    Bundled(String),
    /// User-imported sound, identified by an opaque token (typically UUID).
    Custom(String),
    /// Platform default alarm sound — resolved via the sound backend.
    SystemDefault,
    /// Silent — vibration only.
    Silent,
}

impl SoundId {
    /// True for `Silent` — used to skip audio paths.
    pub fn is_silent(&self) -> bool {
        matches!(self, SoundId::Silent)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_silent_only_for_silent_variant() {
        assert!(SoundId::Silent.is_silent());
        assert!(!SoundId::SystemDefault.is_silent());
        assert!(!SoundId::Bundled("a".into()).is_silent());
        assert!(!SoundId::Custom("u".into()).is_silent());
    }

    #[test]
    fn json_format_is_tagged() {
        let json = serde_json::to_string(&SoundId::Bundled("happy".into())).unwrap();
        assert!(json.contains("\"kind\":\"bundled\""));
        assert!(json.contains("\"value\":\"happy\""));
    }
}
