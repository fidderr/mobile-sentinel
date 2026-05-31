//! Sound Library — bundled enumeration, custom import, and resolution
//! to playable URIs.
//!
//! A [`SoundLibrary`] is constructed with a [`SoundBackend`] (real on
//! Android, mock in tests) and two filesystem roots:
//!
//! - `bundled_dir` — APK assets extracted by [`crate::AssetExtractor`];
//!   files there are addressed by stem (e.g. `happy` → `<bundled_dir>/happy.mp3`).
//! - `custom_dir` — user-imported sounds, written by [`SoundLibrary::import`].
//!
//! At fire time a Recipe calls [`SoundLibrary::resolve`] with a [`SoundId`];
//! the library returns a `file://`-style URI string, falling back to the
//! system default sound (with a structured warning) if the referenced
//! file is missing.

use std::fs;
use std::path::{Path, PathBuf};

use thiserror::Error;
use uuid::Uuid;

use super::sound_id::SoundId;

/// Backend contract for talking to the platform from the Sound Library.
/// On Android this is implemented by the JNI layer; in tests by a mock.
pub trait SoundBackend: Send + Sync {
    /// Return the system default alarm/notification URI as a string.
    fn system_default_uri(&self) -> String;
}

/// Errors returned by [`SoundLibrary`] operations.
#[derive(Debug, Error)]
pub enum SoundError {
    #[error("io error during {operation}: {source}")]
    Io {
        operation: &'static str,
        #[source]
        source: std::io::Error,
    },
    #[error("source path not readable: {0}")]
    SourceNotReadable(PathBuf),
    #[error("imported file is empty")]
    EmptyImport,
    #[error("file extension '{0}' is not supported (expected mp3/m4a/ogg/wav)")]
    UnsupportedExtension(String),
}

impl SoundError {
    fn io(operation: &'static str, source: std::io::Error) -> Self {
        Self::Io { operation, source }
    }
}

/// Allowed bundled / custom sound file extensions (lower-case).
const ALLOWED_EXTENSIONS: &[&str] = &["mp3", "m4a", "ogg", "wav"];

/// Sound Library.
pub struct SoundLibrary<B: SoundBackend> {
    bundled_dir: PathBuf,
    custom_dir: PathBuf,
    backend: B,
}

/// Minimal record describing a bundled or custom sound entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoundEntry {
    pub id: SoundId,
    /// Suggested display label (file stem; consumer may override).
    pub label: String,
    /// Path on disk.
    pub path: PathBuf,
}

impl<B: SoundBackend> SoundLibrary<B> {
    /// Construct a library rooted at the supplied directories.
    pub fn new(
        bundled_dir: impl Into<PathBuf>,
        custom_dir: impl Into<PathBuf>,
        backend: B,
    ) -> Self {
        Self {
            bundled_dir: bundled_dir.into(),
            custom_dir: custom_dir.into(),
            backend,
        }
    }

    pub fn bundled_dir(&self) -> &Path {
        &self.bundled_dir
    }

    pub fn custom_dir(&self) -> &Path {
        &self.custom_dir
    }

    /// Enumerate every bundled sound. Returns the entries sorted by label.
    pub fn enumerate_bundled(&self) -> Result<Vec<SoundEntry>, SoundError> {
        Self::enumerate_dir(&self.bundled_dir, |stem| SoundId::Bundled(stem.to_owned()))
    }

    /// Enumerate every imported custom sound. Returns the entries sorted
    /// by label.
    pub fn enumerate_custom(&self) -> Result<Vec<SoundEntry>, SoundError> {
        Self::enumerate_dir(&self.custom_dir, |stem| SoundId::Custom(stem.to_owned()))
    }

    /// Resolve a [`SoundId`] to a playable URI.
    ///
    /// Falls back to the system default URI with a structured warning if
    /// the referenced file is missing. Returns `"silent://"` for
    /// [`SoundId::Silent`] so Recipe handlers can branch consistently.
    pub fn resolve(&self, id: &SoundId) -> String {
        match id {
            SoundId::Silent => "silent://".to_owned(),
            SoundId::SystemDefault => self.backend.system_default_uri(),
            SoundId::Bundled(stem) => {
                if let Some(path) = self.find_with_supported_ext(&self.bundled_dir, stem) {
                    file_uri(&path)
                } else {
                    log::warn!(
                        "[SoundLibrary] bundled sound '{}' missing — falling back to system default",
                        stem
                    );
                    self.backend.system_default_uri()
                }
            }
            SoundId::Custom(token) => {
                if let Some(path) = self.find_with_supported_ext(&self.custom_dir, token) {
                    file_uri(&path)
                } else {
                    log::warn!(
                        "[SoundLibrary] custom sound '{}' missing — falling back to system default",
                        token
                    );
                    self.backend.system_default_uri()
                }
            }
        }
    }

    /// Import a sound from `source_path` into the custom directory and
    /// return the new [`SoundId::Custom`] token. The token is a freshly
    /// generated UUID; consumers may attach their own display label.
    ///
    /// Validates: source exists, source is non-empty, extension is in
    /// [`ALLOWED_EXTENSIONS`]. Copies bytes to
    /// `<custom_dir>/<uuid>.<ext>`.
    pub fn import(&self, source_path: &Path) -> Result<SoundId, SoundError> {
        let bytes = fs::read(source_path)
            .map_err(|_| SoundError::SourceNotReadable(source_path.to_owned()))?;
        if bytes.is_empty() {
            return Err(SoundError::EmptyImport);
        }
        let ext = source_path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_lowercase())
            .unwrap_or_default();
        if !ALLOWED_EXTENSIONS.contains(&ext.as_str()) {
            return Err(SoundError::UnsupportedExtension(ext));
        }
        fs::create_dir_all(&self.custom_dir).map_err(|e| SoundError::io("import.mkdir", e))?;
        let token = Uuid::new_v4().to_string();
        let dest = self.custom_dir.join(format!("{token}.{ext}"));
        fs::write(&dest, &bytes).map_err(|e| SoundError::io("import.write", e))?;
        Ok(SoundId::Custom(token))
    }

    /// Remove a custom sound by its token. Idempotent.
    pub fn delete_custom(&self, token: &str) -> Result<(), SoundError> {
        if let Some(path) = self.find_with_supported_ext(&self.custom_dir, token) {
            match fs::remove_file(&path) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(SoundError::io("delete_custom", e)),
            }
        } else {
            Ok(())
        }
    }

    // -----------------------------------------------------------------
    // Internals
    // -----------------------------------------------------------------

    fn enumerate_dir<F>(dir: &Path, mk_id: F) -> Result<Vec<SoundEntry>, SoundError>
    where
        F: Fn(&str) -> SoundId,
    {
        let entries_iter = match fs::read_dir(dir) {
            Ok(it) => it,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
            Err(e) => return Err(SoundError::io("enumerate.read_dir", e)),
        };
        let mut entries = Vec::new();
        for entry in entries_iter {
            let entry = entry.map_err(|e| SoundError::io("enumerate.entry", e))?;
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.to_lowercase())
                .unwrap_or_default();
            if !ALLOWED_EXTENSIONS.contains(&ext.as_str()) {
                continue;
            }
            let stem = match path.file_stem().and_then(|s| s.to_str()) {
                Some(s) => s.to_owned(),
                None => continue,
            };
            entries.push(SoundEntry {
                id: mk_id(&stem),
                label: stem,
                path,
            });
        }
        entries.sort_by(|a, b| a.label.cmp(&b.label));
        Ok(entries)
    }

    fn find_with_supported_ext(&self, dir: &Path, stem: &str) -> Option<PathBuf> {
        // First try the token as a full filename (custom sounds include extension)
        let direct = dir.join(stem);
        if direct.exists() {
            return Some(direct);
        }
        // Fall back to stem + supported extensions
        for ext in ALLOWED_EXTENSIONS {
            let candidate = dir.join(format!("{stem}.{ext}"));
            if candidate.exists() {
                return Some(candidate);
            }
        }
        None
    }
}

/// Convert a filesystem path to a `file://` URI string. The path is
/// rendered with forward slashes for cross-platform consistency.
fn file_uri(path: &Path) -> String {
    let s = path.to_string_lossy().replace('\\', "/");
    if s.starts_with('/') {
        format!("file://{s}")
    } else {
        format!("file:///{s}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    struct MockBackend;
    impl SoundBackend for MockBackend {
        fn system_default_uri(&self) -> String {
            "system://default-alarm".to_owned()
        }
    }

    fn lib(dir: &Path) -> SoundLibrary<MockBackend> {
        let bundled = dir.join("bundled");
        let custom = dir.join("custom");
        fs::create_dir_all(&bundled).unwrap();
        fs::create_dir_all(&custom).unwrap();
        SoundLibrary::new(bundled, custom, MockBackend)
    }

    #[test]
    fn silent_resolves_to_silent_uri() {
        let dir = TempDir::new().unwrap();
        let l = lib(dir.path());
        assert_eq!(l.resolve(&SoundId::Silent), "silent://");
    }

    #[test]
    fn system_default_resolves_via_backend() {
        let dir = TempDir::new().unwrap();
        let l = lib(dir.path());
        assert_eq!(l.resolve(&SoundId::SystemDefault), "system://default-alarm");
    }

    #[test]
    fn bundled_resolves_to_file_uri_when_present() {
        let dir = TempDir::new().unwrap();
        let l = lib(dir.path());
        let path = l.bundled_dir().join("happy.mp3");
        fs::write(&path, b"not real audio").unwrap();
        let uri = l.resolve(&SoundId::Bundled("happy".into()));
        assert!(uri.starts_with("file://"));
        assert!(uri.contains("happy.mp3"));
    }

    #[test]
    fn missing_bundled_falls_back_to_system_default() {
        let dir = TempDir::new().unwrap();
        let l = lib(dir.path());
        let uri = l.resolve(&SoundId::Bundled("ghost".into()));
        assert_eq!(uri, "system://default-alarm");
    }

    #[test]
    fn missing_custom_falls_back_to_system_default() {
        let dir = TempDir::new().unwrap();
        let l = lib(dir.path());
        let uri = l.resolve(&SoundId::Custom("uuid-1".into()));
        assert_eq!(uri, "system://default-alarm");
    }

    #[test]
    fn enumerate_bundled_lists_supported_files_only() {
        let dir = TempDir::new().unwrap();
        let l = lib(dir.path());
        // Supported extensions:
        for stem in ["happy", "lofi", "samba"] {
            fs::write(l.bundled_dir().join(format!("{stem}.mp3")), b"x").unwrap();
        }
        // Unsupported — must be filtered out:
        fs::write(l.bundled_dir().join("readme.txt"), b"hi").unwrap();
        let mut entries = l.enumerate_bundled().unwrap();
        entries.sort_by(|a, b| a.label.cmp(&b.label));
        let labels: Vec<_> = entries.iter().map(|e| e.label.clone()).collect();
        assert_eq!(labels, vec!["happy", "lofi", "samba"]);
        for entry in entries {
            assert!(matches!(entry.id, SoundId::Bundled(_)));
        }
    }

    #[test]
    fn enumerate_returns_empty_when_dir_missing() {
        let dir = TempDir::new().unwrap();
        let bundled = dir.path().join("does-not-exist");
        let custom = dir.path().join("also-not-real");
        let l = SoundLibrary::new(bundled, custom, MockBackend);
        assert!(l.enumerate_bundled().unwrap().is_empty());
        assert!(l.enumerate_custom().unwrap().is_empty());
    }

    #[test]
    fn import_creates_unique_token_and_returns_custom_id() {
        let dir = TempDir::new().unwrap();
        let l = lib(dir.path());
        let src = dir.path().join("input.mp3");
        fs::write(&src, b"01234567").unwrap();

        let id1 = l.import(&src).unwrap();
        let id2 = l.import(&src).unwrap();
        assert_ne!(id1, id2);
        match (&id1, &id2) {
            (SoundId::Custom(a), SoundId::Custom(b)) => {
                assert_ne!(a, b);
                let uri1 = l.resolve(&id1);
                let uri2 = l.resolve(&id2);
                assert!(uri1.contains(a));
                assert!(uri2.contains(b));
                assert!(uri1.contains(".mp3"));
            }
            _ => panic!("expected Custom variant"),
        }
    }

    #[test]
    fn import_rejects_unsupported_extension() {
        let dir = TempDir::new().unwrap();
        let l = lib(dir.path());
        let src = dir.path().join("input.txt");
        fs::write(&src, b"hello").unwrap();
        assert!(matches!(
            l.import(&src),
            Err(SoundError::UnsupportedExtension(_))
        ));
    }

    #[test]
    fn import_rejects_empty_file() {
        let dir = TempDir::new().unwrap();
        let l = lib(dir.path());
        let src = dir.path().join("empty.mp3");
        fs::write(&src, b"").unwrap();
        assert!(matches!(l.import(&src), Err(SoundError::EmptyImport)));
    }

    #[test]
    fn import_then_resolve_round_trips() {
        let dir = TempDir::new().unwrap();
        let l = lib(dir.path());
        let src = dir.path().join("song.ogg");
        fs::write(&src, b"data").unwrap();
        let id = l.import(&src).unwrap();
        let uri = l.resolve(&id);
        assert!(uri.starts_with("file://"));
        assert!(uri.contains(".ogg"));
    }

    #[test]
    fn delete_custom_removes_file_and_is_idempotent() {
        let dir = TempDir::new().unwrap();
        let l = lib(dir.path());
        let src = dir.path().join("song.mp3");
        fs::write(&src, b"data").unwrap();
        let id = l.import(&src).unwrap();
        let token = match &id {
            SoundId::Custom(t) => t.clone(),
            _ => panic!(),
        };
        l.delete_custom(&token).unwrap();
        // Calling again is a no-op.
        l.delete_custom(&token).unwrap();
        // Now resolve falls back.
        assert_eq!(l.resolve(&id), "system://default-alarm");
    }
}
