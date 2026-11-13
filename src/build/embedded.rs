//! The capability list inside the app's native library.
//!
//! `build.rs` records the Cargo features mobile-sentinel was compiled with, and this module keeps
//! that list as a string in the library itself. `build_sentinel` reads it back from the `.so` it
//! is about to package, so the manifest and the Gradle modules always match the library, whatever
//! `CARGO_TARGET_DIR` is and however many older builds lie in `target/`.
//!
//! Only the reading side is here (host tooling). The list itself is kept in the library by
//! `crate::capability_info`, which every build has.

use std::fs;
use std::path::Path;

pub use crate::capability_info::MARKER;

/// The capability ids embedded in a library's bytes: `None` when it holds no list (a library
/// built with an older mobile-sentinel), `Some(vec![])` when it was built with none.
pub fn parse(bytes: &[u8]) -> Option<Vec<String>> {
    let marker = MARKER.as_bytes();
    let mut from = 0;
    while let Some(at) = bytes[from..]
        .windows(marker.len())
        .position(|w| w == marker)
    {
        let start = from + at + marker.len();
        let tail = &bytes[start..];
        let end = tail.iter().position(|b| {
            !(b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'_' || *b == b',')
        })?;
        if tail[end] == b']' {
            let list = std::str::from_utf8(&tail[..end]).ok()?;
            let mut ids: Vec<String> = list
                .split(',')
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect();
            ids.sort();
            ids.dedup();
            return Some(ids);
        }
        from = start;
    }
    None
}

/// The union of the lists embedded in every `.so` below `dir` (one per ABI), `None` when no
/// library there holds one.
pub fn scan_dir(dir: &Path) -> Option<Vec<String>> {
    let mut found: Option<Vec<String>> = None;
    collect(dir, 0, &mut found);
    found
}

fn collect(dir: &Path, depth: usize, found: &mut Option<Vec<String>>) {
    if depth > 4 {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, depth + 1, found);
        } else if path.extension().is_some_and(|e| e == "so") {
            if let Some(ids) = fs::read(&path).ok().and_then(|b| parse(&b)) {
                let all = found.get_or_insert_with(Vec::new);
                all.extend(ids);
                all.sort();
                all.dedup();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability_info::MOBILE_SENTINEL_CAPABILITIES_INFO;

    #[test]
    fn the_embedded_list_starts_with_the_marker_and_is_what_build_rs_recorded() {
        assert!(MOBILE_SENTINEL_CAPABILITIES_INFO.starts_with(MARKER));
        let from_binary = parse(MOBILE_SENTINEL_CAPABILITIES_INFO.as_bytes()).unwrap();
        let recorded = env!("MOBILE_SENTINEL_CAPABILITY_LIST");
        let mut expected: Vec<String> = recorded
            .split(',')
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect();
        expected.sort();
        assert_eq!(from_binary, expected);
    }

    #[test]
    fn parse_finds_the_list_among_other_bytes_and_skips_look_alikes() {
        let mut lib = vec![0u8, 1, 2, 3];
        lib.extend_from_slice(MARKER.as_bytes()); // a stray copy, never closed by `]`
        lib.extend_from_slice(b"\0junk");
        lib.extend_from_slice(b"\x7fELF");
        lib.extend_from_slice(MARKER.as_bytes());
        lib.extend_from_slice(b"scanner,audio,scanner]\0more");
        assert_eq!(
            parse(&lib),
            Some(vec!["audio".to_string(), "scanner".to_string()])
        );
    }

    #[test]
    fn parse_tells_none_from_an_empty_list() {
        assert_eq!(parse(b"no list in here"), None);
        assert_eq!(parse(b""), None);
        let mut lib = MARKER.as_bytes().to_vec();
        lib.extend_from_slice(b"]");
        assert_eq!(parse(&lib), Some(Vec::new()));
        // Cut off before the closing bracket: not a list.
        assert_eq!(parse(MARKER.as_bytes()), None);
    }

    #[test]
    fn scan_dir_joins_the_libraries_of_every_abi_and_ignores_the_rest() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        assert_eq!(scan_dir(root), None);
        fs::create_dir_all(root.join("arm64-v8a")).unwrap();
        fs::create_dir_all(root.join("x86_64")).unwrap();
        fs::write(
            root.join("arm64-v8a/libmain.so"),
            b"xx mobile-sentinel-capabilities:v1[audio]",
        )
        .unwrap();
        fs::write(
            root.join("x86_64/libmain.so"),
            b"mobile-sentinel-capabilities:v1[audio,share]",
        )
        .unwrap();
        fs::write(
            root.join("x86_64/notes.txt"),
            b"mobile-sentinel-capabilities:v1[battery]",
        )
        .unwrap();
        assert_eq!(
            scan_dir(root),
            Some(vec!["audio".to_string(), "share".to_string()])
        );
    }
}
