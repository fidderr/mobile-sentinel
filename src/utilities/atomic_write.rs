//! Crash-safe, reader-safe file replacement shared by the on-disk stores.
//!
//! A file is written to a uniquely named sibling temp file, flushed to disk,
//! then renamed over the destination. Because the rename is atomic, a reader
//! (including the `:sentinel` process) observes either the complete previous
//! contents or the complete new contents, never a partial write. The temp name
//! embeds the process id and a per-process counter so concurrent writers never
//! clobber each other's temp file.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Suffix every temp file ends with; stores skip files carrying it.
pub(crate) const TEMP_SUFFIX: &str = ".tmp";

fn temp_path_for(dest: &Path) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut name = dest
        .file_name()
        .map(|s| s.to_os_string())
        .unwrap_or_default();
    name.push(format!(".{}.{}{}", std::process::id(), n, TEMP_SUFFIX));
    dest.with_file_name(name)
}

/// Atomically replace `dest` with `bytes`, creating parent directories.
///
/// On any failure the temp file is removed and `dest` is left untouched.
pub(crate) fn write_atomic(dest: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = dest.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    let tmp = temp_path_for(dest);
    let result = (|| {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        fs::rename(&tmp, dest)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tempfile::TempDir;

    #[test]
    fn writes_and_replaces() {
        let dir = TempDir::new().unwrap();
        let p = dir.path().join("a.json");
        write_atomic(&p, b"one").unwrap();
        write_atomic(&p, b"two").unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"two");
        let leftovers: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().ends_with(TEMP_SUFFIX))
            .collect();
        assert!(leftovers.is_empty());
    }

    #[test]
    fn creates_missing_parent_dirs() {
        let dir = TempDir::new().unwrap();
        let p = dir.path().join("x/y/z.json");
        write_atomic(&p, b"ok").unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"ok");
    }

    /// A reader hammering the file while writers replace it must only ever
    /// see one of the complete payloads, never a truncated/partial one.
    #[test]
    fn readers_never_observe_partial_content() {
        let dir = TempDir::new().unwrap();
        let p = Arc::new(dir.path().join("job.json"));
        let small = vec![b'a'; 16];
        let big = vec![b'b'; 256 * 1024];
        write_atomic(&p, &small).unwrap();

        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let writers: Vec<_> = [small.clone(), big.clone()]
            .into_iter()
            .map(|payload| {
                let p = p.clone();
                let stop = stop.clone();
                std::thread::spawn(move || {
                    while !stop.load(Ordering::Relaxed) {
                        write_atomic(&p, &payload).unwrap();
                    }
                })
            })
            .collect();

        for _ in 0..2000 {
            match fs::read(&*p) {
                Ok(got) => assert!(got == small || got == big, "partial read: {} bytes", got.len()),
                // On Windows a rename can briefly deny a concurrent open.
                Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {}
                Err(e) => panic!("unexpected read error: {e}"),
            }
        }
        stop.store(true, Ordering::Relaxed);
        for w in writers {
            w.join().unwrap();
        }
    }
}
