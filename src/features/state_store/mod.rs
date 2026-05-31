//! Generic durable per-instance state store.
//!
//! [`StateStore<T>`] is an atomic, crash-safe, per-id JSON store that any
//! consumer can use to persist their own instance state — not just recipes.
//! It is the reusable primitive the recipe layer's `ContextStore` is built
//! on (`ContextStore = StateStore<ContextRecord>`), and it is exposed
//! directly behind the `state-store` Cargo feature so consumers can store
//! arbitrary `Stateful` records of their own type.
//!
//! # Guarantees
//!
//! | Property | How |
//! |---|---|
//! | Layout | one JSON file per instance: `<root>/<id>.json` |
//! | Atomicity | write to `<id>.json.tmp`, then `rename` (POSIX / Win32 atomic) |
//! | Concurrency | per-id `Mutex` serialises writers; reads are lock-free |
//! | Revision | every write bumps `revision = previous + 1` from disk state |
//! | Resilience | unparseable files are logged + skipped, never propagated |

mod revision;

pub use revision::Revision;

/// Public alias for the store error type (distinct from the recipe
/// `context::StoreError` re-export which aliases the same type).
pub use self::StoreError as StateStoreError;

use std::collections::HashMap;
use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};

use serde::de::DeserializeOwned;
use serde::Serialize;
use thiserror::Error;

use crate::types::InstanceId;

/// A record that can live in a [`StateStore`]: it carries its own instance
/// id and a monotonic revision the store manages.
///
/// Consumers implement this for their own record type to use [`StateStore`]
/// with arbitrary payloads. The recipe layer implements it for
/// `ContextRecord`.
pub trait Stateful: Serialize + DeserializeOwned + Clone {
    /// The instance id this record belongs to (the file key).
    fn instance_id(&self) -> &InstanceId;
    /// The record's current revision.
    fn revision(&self) -> Revision;
    /// Return the record with its revision replaced. Called by the store on
    /// every write to enforce the monotonic-revision invariant.
    fn with_revision(self, revision: Revision) -> Self;
}

/// Errors returned by [`StateStore`] operations.
#[derive(Debug, Error)]
pub enum StoreError {
    /// The requested instance is not present in the store.
    #[error("state not found for instance {0}")]
    NotFound(InstanceId),
    /// File-system failure during read/write/rename/list.
    #[error("io error during {operation}: {source}")]
    Io {
        operation: &'static str,
        #[source]
        source: std::io::Error,
    },
    /// JSON (de)serialisation failure.
    #[error("serde error during {operation}: {source}")]
    Serde {
        operation: &'static str,
        #[source]
        source: serde_json::Error,
    },
    /// The persisted record's id does not match the file name.
    #[error("instance id mismatch: file={file}, record={record}")]
    IdMismatch { file: String, record: String },
    /// Attempted to lock an instance whose mutex was poisoned. Indicates a
    /// panic occurred inside a previous write. Treat as fatal.
    #[error("instance lock poisoned for {0}")]
    LockPoisoned(InstanceId),
}

impl StoreError {
    fn io(operation: &'static str, source: std::io::Error) -> Self {
        Self::Io { operation, source }
    }

    fn serde(operation: &'static str, source: serde_json::Error) -> Self {
        Self::Serde { operation, source }
    }
}

/// Atomic, per-id JSON store generic over a [`Stateful`] record type `T`.
///
/// Construct with [`StateStore::new`] passing the directory that should
/// contain the per-id JSON files. The directory is created on first write
/// if it does not already exist.
pub struct StateStore<T: Stateful> {
    root: PathBuf,
    /// Per-instance writer mutexes. Acquired by `write` / `delete` for the
    /// duration of the file rename. Reads do not touch this map.
    writer_locks: RwLock<HashMap<InstanceId, Arc<Mutex<()>>>>,
    _marker: std::marker::PhantomData<fn() -> T>,
}

impl<T: Stateful> StateStore<T> {
    /// Create a store rooted at `root`. The directory is created lazily on
    /// first write.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            writer_locks: RwLock::new(HashMap::new()),
            _marker: std::marker::PhantomData,
        }
    }

    /// The on-disk directory this store writes into.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Read the current record for `id`. Returns [`StoreError::NotFound`]
    /// if no record exists. Reads observe either the pre-write or
    /// post-rename state — never a partial write.
    pub fn load(&self, id: &InstanceId) -> Result<T, StoreError> {
        let path = self.record_path(id);
        let bytes = match fs::read(&path) {
            Ok(b) => b,
            Err(e) if e.kind() == ErrorKind::NotFound => {
                return Err(StoreError::NotFound(id.clone()))
            }
            Err(e) => return Err(StoreError::io("load.read", e)),
        };
        let record: T =
            serde_json::from_slice(&bytes).map_err(|e| StoreError::serde("load.parse", e))?;
        if record.instance_id() != id {
            return Err(StoreError::IdMismatch {
                file: id.0.clone(),
                record: record.instance_id().0.clone(),
            });
        }
        Ok(record)
    }

    /// True if a record exists for `id`.
    pub fn exists(&self, id: &InstanceId) -> bool {
        self.record_path(id).exists()
    }

    /// Atomically write `record`. The persisted revision is set to
    /// `previous.revision.next()` (or [`Revision::INITIAL`] on first
    /// write), regardless of what the caller passed.
    ///
    /// On success returns the persisted record (with the bumped revision).
    pub fn write(&self, record: T) -> Result<T, StoreError> {
        let id = record.instance_id().clone();
        let lock = self.acquire_lock(&id);
        let _guard = lock
            .lock()
            .map_err(|_| StoreError::LockPoisoned(id.clone()))?;

        // Compute the new revision based on what's currently on disk so
        // concurrent processes can't accidentally roll the counter back.
        let next_revision = match self.load(&id) {
            Ok(prev) => prev.revision().next(),
            Err(StoreError::NotFound(_)) => Revision::INITIAL,
            Err(e) => return Err(e),
        };
        let record = record.with_revision(next_revision);

        self.ensure_root()?;

        let final_path = self.record_path(&id);
        let temp_path = self.temp_path(&id);

        let bytes = serde_json::to_vec_pretty(&record)
            .map_err(|e| StoreError::serde("write.serialize", e))?;

        {
            let mut f =
                fs::File::create(&temp_path).map_err(|e| StoreError::io("write.create_temp", e))?;
            f.write_all(&bytes)
                .map_err(|e| StoreError::io("write.write_temp", e))?;
            let _ = f.sync_all();
        }

        fs::rename(&temp_path, &final_path).map_err(|e| StoreError::io("write.rename", e))?;

        Ok(record)
    }

    /// Remove the record for `id`. Idempotent (Ok even if absent).
    pub fn delete(&self, id: &InstanceId) -> Result<(), StoreError> {
        let lock = self.acquire_lock(id);
        let _guard = lock
            .lock()
            .map_err(|_| StoreError::LockPoisoned(id.clone()))?;

        let path = self.record_path(id);
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
            Err(e) => Err(StoreError::io("delete", e)),
        }
    }

    /// Enumerate all persisted records. Files that fail to parse are
    /// skipped with a structured warning logged via `log::warn!`.
    pub fn enumerate_all(&self) -> Result<Vec<T>, StoreError> {
        let dir = match fs::read_dir(&self.root) {
            Ok(d) => d,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(vec![]),
            Err(e) => return Err(StoreError::io("enumerate.read_dir", e)),
        };

        let mut out = Vec::new();
        for entry in dir {
            let entry = entry.map_err(|e| StoreError::io("enumerate.entry", e))?;
            let path = entry.path();
            let file_name = match path.file_name().and_then(|s| s.to_str()) {
                Some(n) => n,
                None => continue,
            };
            if !file_name.ends_with(".json") || file_name.ends_with(".json.tmp") {
                continue;
            }
            let bytes = match fs::read(&path) {
                Ok(b) => b,
                Err(_) => continue,
            };
            match serde_json::from_slice::<T>(&bytes) {
                Ok(record) => out.push(record),
                Err(e) => {
                    log::warn!(
                        "[StateStore] skipping unparseable record {}: {}",
                        file_name,
                        e
                    );
                }
            }
        }
        Ok(out)
    }

    /// Number of records currently persisted.
    pub fn count(&self) -> Result<usize, StoreError> {
        Ok(self.enumerate_all()?.len())
    }

    // -----------------------------------------------------------------
    // Internals
    // -----------------------------------------------------------------

    fn record_path(&self, id: &InstanceId) -> PathBuf {
        self.root.join(format!("{}.json", id.0))
    }

    fn temp_path(&self, id: &InstanceId) -> PathBuf {
        self.root.join(format!("{}.json.tmp", id.0))
    }

    fn ensure_root(&self) -> Result<(), StoreError> {
        fs::create_dir_all(&self.root).map_err(|e| StoreError::io("ensure_root", e))
    }

    fn acquire_lock(&self, id: &InstanceId) -> Arc<Mutex<()>> {
        if let Some(existing) = self.writer_locks.read().unwrap().get(id) {
            return existing.clone();
        }
        let mut table = self.writer_locks.write().unwrap();
        table
            .entry(id.clone())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    use tempfile::TempDir;

    // A minimal consumer-defined record proving the store is generic.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct Note {
        id: InstanceId,
        revision: Revision,
        text: String,
    }

    impl Stateful for Note {
        fn instance_id(&self) -> &InstanceId {
            &self.id
        }
        fn revision(&self) -> Revision {
            self.revision
        }
        fn with_revision(mut self, revision: Revision) -> Self {
            self.revision = revision;
            self
        }
    }

    fn note(id: &str, text: &str) -> Note {
        Note {
            id: InstanceId::new(id),
            revision: Revision::INITIAL,
            text: text.into(),
        }
    }

    #[test]
    fn write_then_load_round_trips_generic_payload() {
        let dir = TempDir::new().unwrap();
        let store: StateStore<Note> = StateStore::new(dir.path());
        store.write(note("a", "hello")).unwrap();
        let loaded = store.load(&InstanceId::new("a")).unwrap();
        assert_eq!(loaded.text, "hello");
    }

    #[test]
    fn revision_increments_per_write() {
        let dir = TempDir::new().unwrap();
        let store: StateStore<Note> = StateStore::new(dir.path());
        assert_eq!(store.write(note("a", "1")).unwrap().revision, Revision(1));
        assert_eq!(store.write(note("a", "2")).unwrap().revision, Revision(2));
        assert_eq!(store.write(note("a", "3")).unwrap().revision, Revision(3));
    }

    #[test]
    fn load_missing_is_not_found() {
        let dir = TempDir::new().unwrap();
        let store: StateStore<Note> = StateStore::new(dir.path());
        assert!(matches!(
            store.load(&InstanceId::new("x")),
            Err(StoreError::NotFound(_))
        ));
    }

    #[test]
    fn delete_is_idempotent() {
        let dir = TempDir::new().unwrap();
        let store: StateStore<Note> = StateStore::new(dir.path());
        store.write(note("a", "1")).unwrap();
        store.delete(&InstanceId::new("a")).unwrap();
        store.delete(&InstanceId::new("a")).unwrap();
        assert!(!store.exists(&InstanceId::new("a")));
    }

    #[test]
    fn enumerate_skips_unparseable() {
        let dir = TempDir::new().unwrap();
        let store: StateStore<Note> = StateStore::new(dir.path());
        store.write(note("good", "ok")).unwrap();
        fs::write(dir.path().join("bad.json"), b"not json").unwrap();
        assert_eq!(store.enumerate_all().unwrap().len(), 1);
    }
}
