//! APK/IPA asset extraction to filesystem.

use std::fs;
use std::path::{Path, PathBuf};

/// Extract bundled assets from the app package to the filesystem.
/// On Android, reads from APK assets via JNI AssetManager.
/// On other platforms, copies from a source directory.
pub struct AssetExtractor {
    target_base: PathBuf,
}

impl AssetExtractor {
    /// Create an extractor that writes to the given base directory.
    pub fn new(target_base: PathBuf) -> Self {
        Self { target_base }
    }

    /// Extract all files from an asset subdirectory to a target subdirectory.
    /// Only copies files that don't already exist (idempotent).
    /// Returns the number of files extracted.
    #[cfg(target_os = "android")]
    pub fn extract(&self, asset_subdir: &str, target_subdir: &str) -> usize {
        let dest_dir = self.target_base.join(target_subdir);
        let _ = fs::create_dir_all(&dest_dir);

        let Ok((vm, activity)) = crate::platform::android::context::get_activity() else {
            eprintln!("[mobile-sentinel] AssetExtractor: get_activity failed");
            return 0;
        };
        let Ok(mut env) = vm.attach_current_thread() else {
            eprintln!("[mobile-sentinel] AssetExtractor: attach failed");
            return 0;
        };

        use jni::objects::JValue;

        // Get AssetManager
        let asset_manager = match env.call_method(
            activity.as_obj(),
            "getAssets",
            "()Landroid/content/res/AssetManager;",
            &[],
        ) {
            Ok(v) => match v.l() {
                Ok(obj) => obj,
                Err(_) => return 0,
            },
            Err(_) => {
                let _ = env.exception_clear();
                return 0;
            }
        };

        // List files
        let path_str = match env.new_string(asset_subdir) {
            Ok(s) => s,
            Err(_) => return 0,
        };
        let file_list = match env.call_method(
            &asset_manager,
            "list",
            "(Ljava/lang/String;)[Ljava/lang/String;",
            &[JValue::Object(&path_str)],
        ) {
            Ok(v) => match v.l() {
                Ok(obj) => obj,
                Err(_) => return 0,
            },
            Err(_) => {
                let _ = env.exception_clear();
                return 0;
            }
        };

        let array = jni::objects::JObjectArray::from(file_list);
        let length = env.get_array_length(&array).unwrap_or(0);
        let mut count = 0;

        for i in 0..length {
            let Ok(element) = env.get_object_array_element(&array, i) else {
                continue;
            };
            let jstr = jni::objects::JString::from(element);
            let Ok(filename) = env.get_string(&jstr) else {
                continue;
            };
            let filename: String = filename.into();

            let dest_path = dest_dir.join(&filename);
            if dest_path.exists() {
                continue;
            }

            let asset_path = format!("{}/{}", asset_subdir, filename);
            let Ok(asset_jstr) = env.new_string(&asset_path) else {
                continue;
            };

            let input_stream = match env.call_method(
                &asset_manager,
                "open",
                "(Ljava/lang/String;)Ljava/io/InputStream;",
                &[JValue::Object(&asset_jstr)],
            ) {
                Ok(v) => match v.l() {
                    Ok(obj) => obj,
                    Err(_) => continue,
                },
                Err(_) => {
                    let _ = env.exception_clear();
                    continue;
                }
            };

            // Read bytes
            let mut buffer = Vec::new();
            let Ok(buf_array) = env.new_byte_array(8192) else {
                continue;
            };
            loop {
                let bytes_read = match env.call_method(
                    &input_stream,
                    "read",
                    "([B)I",
                    &[JValue::Object(&buf_array)],
                ) {
                    Ok(v) => v.i().unwrap_or(-1),
                    Err(_) => {
                        let _ = env.exception_clear();
                        break;
                    }
                };
                if bytes_read <= 0 {
                    break;
                }
                let mut tmp = vec![0i8; bytes_read as usize];
                let _ = env.get_byte_array_region(&buf_array, 0, &mut tmp);
                buffer.extend(tmp.iter().map(|&b| b as u8));
            }
            let _ = env.call_method(&input_stream, "close", "()V", &[]);
            let _ = env.exception_clear();

            if fs::write(&dest_path, &buffer).is_ok() {
                count += 1;
            }
        }
        count
    }

    /// Non-Android: copy from a source directory on the filesystem.
    #[cfg(not(target_os = "android"))]
    pub fn extract(&self, _asset_subdir: &str, target_subdir: &str) -> usize {
        let dest_dir = self.target_base.join(target_subdir);
        let _ = fs::create_dir_all(&dest_dir);
        0 // No-op on non-Android; consumer can use extract_from_dir instead
    }

    /// Copy files from a filesystem source directory (for desktop/testing).
    pub fn extract_from_dir(&self, source_dir: &Path, target_subdir: &str) -> usize {
        let dest_dir = self.target_base.join(target_subdir);
        let _ = fs::create_dir_all(&dest_dir);
        let mut count = 0;
        if let Ok(entries) = fs::read_dir(source_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    let dest = dest_dir.join(entry.file_name());
                    if !dest.exists() && fs::copy(&path, &dest).is_ok() {
                        count += 1;
                    }
                }
            }
        }
        count
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn extract_from_dir_copies_files() {
        let source = tempdir().unwrap();
        let target = tempdir().unwrap();

        // Create source files
        std::fs::write(source.path().join("file1.txt"), "content1").unwrap();
        std::fs::write(source.path().join("file2.txt"), "content2").unwrap();

        let extractor = AssetExtractor::new(target.path().to_path_buf());
        let count = extractor.extract_from_dir(source.path(), "subdir");
        assert_eq!(count, 2);

        // Verify files exist in target
        let dest = target.path().join("subdir");
        assert_eq!(
            std::fs::read_to_string(dest.join("file1.txt")).unwrap(),
            "content1"
        );
        assert_eq!(
            std::fs::read_to_string(dest.join("file2.txt")).unwrap(),
            "content2"
        );
    }

    #[test]
    fn extract_from_dir_is_idempotent() {
        let source = tempdir().unwrap();
        let target = tempdir().unwrap();

        std::fs::write(source.path().join("file.txt"), "original").unwrap();

        let extractor = AssetExtractor::new(target.path().to_path_buf());
        let count1 = extractor.extract_from_dir(source.path(), "sub");
        assert_eq!(count1, 1);

        // Modify source — should NOT overwrite existing
        std::fs::write(source.path().join("file.txt"), "modified").unwrap();
        let count2 = extractor.extract_from_dir(source.path(), "sub");
        assert_eq!(count2, 0); // nothing new extracted

        // Original content preserved
        let content = std::fs::read_to_string(target.path().join("sub/file.txt")).unwrap();
        assert_eq!(content, "original");
    }

    #[test]
    fn extract_from_dir_empty_source() {
        let source = tempdir().unwrap();
        let target = tempdir().unwrap();
        let extractor = AssetExtractor::new(target.path().to_path_buf());
        let count = extractor.extract_from_dir(source.path(), "empty");
        assert_eq!(count, 0);
    }

    #[test]
    fn extract_from_dir_nonexistent_source() {
        let target = tempdir().unwrap();
        let extractor = AssetExtractor::new(target.path().to_path_buf());
        let count = extractor.extract_from_dir(Path::new("/nonexistent/path"), "sub");
        assert_eq!(count, 0);
    }
}
