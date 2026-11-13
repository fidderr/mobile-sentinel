//! A generated Android host project, for apps that keep no Gradle project of their own.
//!
//! With `generate = true` in `[android]` of `sentinel.toml`, `build_sentinel` writes a
//! complete Gradle project into `target/sentinel-android/` of the app on every run, then
//! wires it like any other project. The app's repository holds no `android/` folder: only
//! its Rust, `sentinel.toml`, and whatever Java or resources are really its own.
//!
//! ```toml
//! [android]
//! generate = true
//! application_id = "com.example.app"     # required
//! version_code = 1                       # default 1
//! version_name = "0.1.0"                 # default "0.1.0"
//! min_sdk = 31                           # defaults 31, 36, 36
//! target_sdk = 36
//! compile_sdk = 36
//! label = "Example"                      # default: the last part of application_id
//! native_lib = "main"                    # a NativeActivity app: the manifest is generated
//! manifest = "app/AndroidManifest.xml"   # or the app's own manifest, used as it is
//! java = ["app/src"]                     # Java/Kotlin source folders
//! res = ["app/res"]                      # resource folders
//! jni_libs = "build/jniLibs"             # <abi>/lib*.so, as cargo builds them
//! asset_dirs = ["build/assets"]          # asset folders used in place (`assets` copies)
//! legacy_jni_packaging = false           # true: libraries are extracted to disk on install
//! proguard = "app/proguard-rules.pro"    # extra R8 rules for release builds
//! release_debug_signing = false          # true: a release build is signed with the debug key
//! ndk_version = "28.2.13676358"          # default: the folder name of ANDROID_NDK_HOME
//! rust_lib = "main"                      # the library of components whose code is Rust (see `shims`)
//! ```
//!
//! Release builds run R8 (code and resources). The app's own package and every native
//! method are kept, because Rust reaches them by name over JNI. With the NDK version known,
//! the Android Gradle plugin strips the debug symbols of the native libraries.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Where the project goes, relative to the app's folder.
pub const HOST_DIR: &str = "target/sentinel-android";

const AGP_VERSION: &str = "8.7.0";
const KOTLIN_VERSION: &str = "2.0.20";

#[derive(Debug, Clone, PartialEq)]
pub struct HostConfig {
    pub application_id: String,
    pub version_code: u32,
    pub version_name: String,
    pub min_sdk: u32,
    pub target_sdk: u32,
    pub compile_sdk: u32,
    pub label: String,
    pub manifest: Option<PathBuf>,
    pub native_lib: Option<String>,
    pub icon: bool,
    pub java_dirs: Vec<PathBuf>,
    pub res_dirs: Vec<PathBuf>,
    pub asset_dirs: Vec<PathBuf>,
    pub jni_libs: Option<PathBuf>,
    pub legacy_jni_packaging: bool,
    pub proguard: Option<PathBuf>,
    pub release_debug_signing: bool,
    pub ndk_version: Option<String>,
    /// The library of the app's Rust components (see `shims`), unless one names its own.
    pub rust_lib: Option<String>,
}

impl HostConfig {
    /// From the `[android]` keys of `sentinel.toml`; relative paths are taken from `app_root`.
    pub fn from_toml(keys: &HashMap<String, String>, app_root: &Path) -> Result<Self, String> {
        let application_id = keys
            .get("application_id")
            .cloned()
            .ok_or("generate = true needs application_id in [android] of sentinel.toml")?;
        if !is_package_name(&application_id) {
            return Err(format!(
                "application_id \"{application_id}\" is not a package name"
            ));
        }
        let number = |key: &str, default: u32| -> Result<u32, String> {
            match keys.get(key) {
                None => Ok(default),
                Some(v) => v
                    .parse()
                    .map_err(|_| format!("{key} = {v} is not a number")),
            }
        };
        let flag = |key: &str| keys.get(key).is_some_and(|v| v == "true");
        let path = |p: &str| absolute(app_root, p);
        let paths = |key: &str| -> Vec<PathBuf> {
            keys.get(key)
                .map(|v| parse_list(v).iter().map(|p| path(p)).collect())
                .unwrap_or_default()
        };
        let native_lib = keys.get("native_lib").cloned();
        let manifest = keys.get("manifest").map(|p| path(p));
        if manifest.is_none() && native_lib.is_none() {
            return Err("generate = true needs manifest (the app's own) or native_lib (a NativeActivity app)".into());
        }
        let label = keys.get("label").cloned().unwrap_or_else(|| {
            application_id
                .rsplit('.')
                .next()
                .unwrap_or(&application_id)
                .to_string()
        });
        Ok(Self {
            version_code: number("version_code", 1)?,
            version_name: keys
                .get("version_name")
                .cloned()
                .unwrap_or_else(|| "0.1.0".into()),
            min_sdk: number("min_sdk", 31)?,
            target_sdk: number("target_sdk", 36)?,
            compile_sdk: number("compile_sdk", 36)?,
            label,
            manifest,
            native_lib,
            icon: keys.contains_key("icon"),
            java_dirs: paths("java"),
            res_dirs: paths("res"),
            asset_dirs: paths("asset_dirs"),
            jni_libs: keys.get("jni_libs").map(|p| path(p)),
            legacy_jni_packaging: flag("legacy_jni_packaging"),
            proguard: keys.get("proguard").map(|p| path(p)),
            release_debug_signing: flag("release_debug_signing"),
            ndk_version: keys.get("ndk_version").cloned().or_else(ndk_from_env),
            rust_lib: keys.get("rust_lib").cloned(),
            application_id,
        })
    }

    /// The activity the launcher starts, when sentinel.toml names none.
    pub fn default_activity(&self) -> Option<&'static str> {
        self.native_lib
            .as_ref()
            .map(|_| "android.app.NativeActivity")
    }
}

/// Writes the whole project into `out`, replacing what an earlier run wrote there.
/// Module output (`sentinel-build/`) and Gradle's caches are left for speed.
pub fn generate(cfg: &HostConfig, wrapper_dir: &Path, out: &Path) -> Result<(), String> {
    let main = out.join("app/src/main");
    let _ = fs::remove_dir_all(&main);
    let write = |rel: &str, content: &str| -> Result<(), String> {
        let path = out.join(rel);
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        fs::write(&path, content).map_err(|e| format!("{}: {e}", path.display()))
    };
    write("settings.gradle", &render_settings(cfg))?;
    write("build.gradle.kts", &render_root_build())?;
    write("gradle.properties", GRADLE_PROPERTIES)?;
    write("app/build.gradle.kts", &render_app_build(cfg))?;
    write("app/proguard-rules.pro", &render_proguard(cfg))?;
    let manifest = match (&cfg.manifest, &cfg.native_lib) {
        (Some(file), _) => {
            fs::read_to_string(file).map_err(|e| format!("manifest {}: {e}", file.display()))?
        }
        (None, Some(lib)) => render_native_manifest(cfg, lib),
        (None, None) => return Err("no manifest and no native_lib".into()),
    };
    write("app/src/main/AndroidManifest.xml", &manifest)?;
    let components = super::shims::components(&manifest, &cfg.application_id);
    let defined = super::shims::defined_classes(&cfg.java_dirs);
    for (rel, text) in super::shims::render(&components, &defined, cfg.rust_lib.as_deref())? {
        write(
            &format!(
                "app/src/main/java/{}",
                rel.to_string_lossy().replace('\\', "/")
            ),
            &text,
        )?;
    }
    if !cfg
        .res_dirs
        .iter()
        .any(|d| d.join("values/styles.xml").exists())
    {
        write("app/src/main/res/values/styles.xml", STYLES)?;
    }
    for rel in [
        "gradlew",
        "gradlew.bat",
        "gradle/wrapper/gradle-wrapper.jar",
        "gradle/wrapper/gradle-wrapper.properties",
    ] {
        let to = out.join(rel);
        if let Some(dir) = to.parent() {
            fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        fs::copy(wrapper_dir.join(rel), &to).map_err(|e| format!("{rel}: {e}"))?;
    }
    make_executable(&out.join("gradlew"));
    Ok(())
}

pub fn render_settings(cfg: &HostConfig) -> String {
    let name: String = cfg
        .label
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect();
    let name = if name.is_empty() {
        "app".to_string()
    } else {
        name
    };
    format!("rootProject.name = \"{name}\"\ninclude ':app'\n")
}

pub fn render_root_build() -> String {
    format!(
        "buildscript {{\n    repositories {{\n        google()\n        mavenCentral()\n    }}\n    \
         dependencies {{\n        classpath(\"com.android.tools.build:gradle:{AGP_VERSION}\")\n        \
         classpath(\"org.jetbrains.kotlin:kotlin-gradle-plugin:{KOTLIN_VERSION}\")\n    }}\n}}\n\n\
         allprojects {{\n    repositories {{\n        google()\n        mavenCentral()\n    }}\n}}\n"
    )
}

pub fn render_app_build(cfg: &HostConfig) -> String {
    let list = |dirs: &[PathBuf]| {
        dirs.iter()
            .map(|d| format!("\"{}\"", gradle_path(d)))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let mut source = String::new();
    if !cfg.java_dirs.is_empty() {
        source.push_str(&format!(
            "            java.srcDirs({})\n",
            list(&cfg.java_dirs)
        ));
    }
    if !cfg.res_dirs.is_empty() {
        source.push_str(&format!(
            "            res.srcDirs({})\n",
            list(&cfg.res_dirs)
        ));
    }
    if let Some(libs) = &cfg.jni_libs {
        source.push_str(&format!(
            "            jniLibs.srcDirs(\"{}\")\n",
            gradle_path(libs)
        ));
    }
    if !cfg.asset_dirs.is_empty() {
        source.push_str(&format!(
            "            assets.srcDirs({})\n",
            list(&cfg.asset_dirs)
        ));
    }
    let mut rules =
        "getDefaultProguardFile(\"proguard-android-optimize.txt\"), \"proguard-rules.pro\""
            .to_string();
    if let Some(extra) = &cfg.proguard {
        rules.push_str(&format!(", \"{}\"", gradle_path(extra)));
    }
    let signing = if cfg.release_debug_signing {
        "            signingConfig = signingConfigs.getByName(\"debug\")\n"
    } else {
        ""
    };
    let ndk = cfg
        .ndk_version
        .as_ref()
        .map(|v| format!("    ndkVersion = \"{v}\"\n"))
        .unwrap_or_default();
    format!(
        "plugins {{\n    id(\"com.android.application\")\n}}\n\n\
         android {{\n    namespace = \"{id}\"\n    compileSdk = {compile}\n{ndk}    \
         defaultConfig {{\n        applicationId = \"{id}\"\n        minSdk = {min}\n        \
         targetSdk = {target}\n        versionCode = {code}\n        versionName = \"{name}\"\n    }}\n    \
         buildTypes {{\n        getByName(\"debug\") {{\n            isDebuggable = true\n            \
         isMinifyEnabled = false\n        }}\n        getByName(\"release\") {{\n            \
         isMinifyEnabled = true\n            isShrinkResources = true\n            \
         proguardFiles({rules})\n{signing}        }}\n    }}\n    \
         compileOptions {{\n        sourceCompatibility = JavaVersion.VERSION_17\n        \
         targetCompatibility = JavaVersion.VERSION_17\n    }}\n    \
         packaging {{\n        jniLibs.useLegacyPackaging = {legacy}\n    }}\n    \
         sourceSets {{\n        getByName(\"main\") {{\n{source}        }}\n    }}\n}}\n\n\
         dependencies {{\n}}\n",
        id = cfg.application_id,
        compile = cfg.compile_sdk,
        min = cfg.min_sdk,
        target = cfg.target_sdk,
        code = cfg.version_code,
        name = cfg.version_name,
        legacy = cfg.legacy_jni_packaging,
    )
}

pub fn render_proguard(cfg: &HostConfig) -> String {
    format!(
        "# Generated by build_sentinel. Rust reaches these by name over JNI.\n\
         -keep class {id}.** {{ *; }}\n\
         -keepclasseswithmembernames,includedescriptorclasses class * {{ native <methods>; }}\n",
        id = cfg.application_id
    )
}

pub fn render_native_manifest(cfg: &HostConfig, lib: &str) -> String {
    let icon = if cfg.icon {
        "\n        android:icon=\"@mipmap/ic_launcher\""
    } else {
        ""
    };
    format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n\
         <manifest xmlns:android=\"http://schemas.android.com/apk/res/android\">\n    \
         <application\n        android:label=\"{label}\"{icon}\n        android:hasCode=\"true\"\n        \
         android:supportsRtl=\"true\"\n        android:enableOnBackInvokedCallback=\"true\"\n        \
         android:theme=\"@style/AppTheme\">\n        \
         <activity android:name=\"android.app.NativeActivity\" android:exported=\"true\"\n            \
         android:launchMode=\"singleTask\"\n            android:windowSoftInputMode=\"adjustResize\"\n            \
         android:configChanges=\"{changes}\">\n            \
         <meta-data android:name=\"android.app.lib_name\" android:value=\"{lib}\" />\n            \
         <intent-filter>\n                <action android:name=\"android.intent.action.MAIN\" />\n                \
         <category android:name=\"android.intent.category.LAUNCHER\" />\n            </intent-filter>\n        \
         </activity>\n    </application>\n</manifest>\n",
        label = xml_escape(&cfg.label),
        changes = CONFIG_CHANGES,
    )
}

/// A native UI redraws itself on every configuration change, so the activity is never
/// recreated for one.
const CONFIG_CHANGES: &str = "mcc|mnc|locale|touchscreen|keyboard|keyboardHidden|navigation|screenLayout|fontScale|uiMode|orientation|density|screenSize|smallestScreenSize|layoutDirection|colorMode|fontWeightAdjustment";

const STYLES: &str = "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<resources>\n    \
    <style name=\"AppTheme\" parent=\"@android:style/Theme.DeviceDefault.NoActionBar\">\n    \
    </style>\n</resources>\n";

const GRADLE_PROPERTIES: &str = "# Generated by build_sentinel.\n\
    org.gradle.jvmargs=-Xmx2048m -Dfile.encoding=UTF-8\n\
    android.useAndroidX=true\n\
    android.nonTransitiveRClass=true\n\
    android.defaults.buildfeatures.buildconfig=true\n\
    android.nonFinalResIds=false\n\
    kotlin.code.style=official\n";

fn is_package_name(s: &str) -> bool {
    let parts: Vec<&str> = s.split('.').collect();
    parts.len() >= 2
        && parts.iter().all(|p| {
            p.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
                && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        })
}

fn absolute(root: &Path, p: &str) -> PathBuf {
    let path = Path::new(p);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}

/// Forward slashes and no `\\?\`: what Gradle accepts on Windows too.
fn gradle_path(p: &Path) -> String {
    let s = p.to_string_lossy().replace('\\', "/");
    s.strip_prefix("//?/").map(str::to_string).unwrap_or(s)
}

fn parse_list(s: &str) -> Vec<String> {
    s.trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .split(',')
        .map(|item| item.trim().trim_matches('"').trim().to_string())
        .filter(|item| !item.is_empty())
        .collect()
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn ndk_from_env() -> Option<String> {
    let home = std::env::var("ANDROID_NDK_HOME")
        .or_else(|_| std::env::var("ANDROID_NDK_ROOT"))
        .ok()?;
    let name = Path::new(home.trim_end_matches(['/', '\\']))
        .file_name()?
        .to_string_lossy()
        .to_string();
    (name.split('.').count() >= 3 && name.split('.').all(|p| p.parse::<u32>().is_ok()))
        .then_some(name)
}

#[cfg(unix)]
fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(meta) = fs::metadata(path) {
        let mut perm = meta.permissions();
        perm.set_mode(0o755);
        let _ = fs::set_permissions(path, perm);
    }
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn native() -> HostConfig {
        HostConfig::from_toml(
            &keys(&[
                ("application_id", "com.example.app"),
                ("native_lib", "main"),
                ("ndk_version", "28.2.13676358"),
            ]),
            Path::new("/work/app"),
        )
        .unwrap()
    }

    #[test]
    fn needs_an_application_id_and_a_manifest_or_native_lib() {
        let root = Path::new("/w");
        assert!(
            HostConfig::from_toml(&keys(&[("native_lib", "main")]), root)
                .unwrap_err()
                .contains("application_id")
        );
        assert!(
            HostConfig::from_toml(&keys(&[("application_id", "com.x.y")]), root)
                .unwrap_err()
                .contains("native_lib")
        );
        assert!(HostConfig::from_toml(
            &keys(&[("application_id", "not a package"), ("native_lib", "m")]),
            root
        )
        .is_err());
        assert!(HostConfig::from_toml(
            &keys(&[
                ("application_id", "com.x.y"),
                ("native_lib", "m"),
                ("min_sdk", "abc")
            ]),
            root
        )
        .unwrap_err()
        .contains("min_sdk"));
    }

    #[test]
    fn defaults_and_paths_from_the_app_folder() {
        let cfg = HostConfig::from_toml(
            &keys(&[
                ("application_id", "dev.example.thing"),
                ("manifest", "app/AndroidManifest.xml"),
                ("java", "[\"app/src\", \"build/gen\"]"),
                ("jni_libs", "build/jniLibs"),
                ("legacy_jni_packaging", "true"),
            ]),
            Path::new("/work/app"),
        )
        .unwrap();
        assert_eq!(cfg.label, "thing");
        assert_eq!((cfg.min_sdk, cfg.target_sdk, cfg.version_code), (31, 36, 1));
        assert_eq!(
            cfg.java_dirs,
            vec![
                PathBuf::from("/work/app/app/src"),
                PathBuf::from("/work/app/build/gen")
            ]
        );
        assert_eq!(
            cfg.manifest,
            Some(PathBuf::from("/work/app/app/AndroidManifest.xml"))
        );
        assert!(cfg.legacy_jni_packaging);
        assert_eq!(cfg.default_activity(), None);
        assert_eq!(
            native().default_activity(),
            Some("android.app.NativeActivity")
        );
    }

    #[test]
    fn app_build_carries_ids_sources_r8_and_the_ndk() {
        let mut cfg = native();
        cfg.java_dirs = vec![PathBuf::from(r"C:\w\app\src")];
        cfg.jni_libs = Some(PathBuf::from(r"C:\w\build\jniLibs"));
        cfg.release_debug_signing = true;
        let build = render_app_build(&cfg);
        assert!(build.contains("namespace = \"com.example.app\""));
        assert!(build.contains("applicationId = \"com.example.app\""));
        assert!(build.contains("java.srcDirs(\"C:/w/app/src\")"));
        assert!(build.contains("jniLibs.srcDirs(\"C:/w/build/jniLibs\")"));
        assert!(!build.contains("res.srcDirs"));
        assert!(build.contains("isMinifyEnabled = true"));
        assert!(build.contains("isShrinkResources = true"));
        assert!(build.contains("ndkVersion = \"28.2.13676358\""));
        assert!(build.contains("jniLibs.useLegacyPackaging = false"));
        assert!(build.contains("signingConfig = signingConfigs.getByName(\"debug\")"));
        // The module wiring finds its place: build_sentinel inserts after this line.
        assert!(build.contains("dependencies {"));
        assert!(build.contains("minSdk = 31"));
    }

    #[test]
    fn keep_rules_cover_the_app_package_and_native_methods() {
        let rules = render_proguard(&native());
        assert!(rules.contains("-keep class com.example.app.** { *; }"));
        assert!(rules.contains("native <methods>"));
    }

    #[test]
    fn native_manifest_launches_the_named_library() {
        let mut cfg = native();
        cfg.label = "A & B".into();
        let m = render_native_manifest(&cfg, "main");
        assert!(m.contains("android:name=\"android.app.NativeActivity\""));
        assert!(m.contains("android:value=\"main\""));
        assert!(m.contains("android.intent.category.LAUNCHER"));
        assert!(m.contains("android:label=\"A &amp; B\""));
        assert!(!m.contains("ic_launcher"));
        cfg.icon = true;
        assert!(render_native_manifest(&cfg, "main").contains("@mipmap/ic_launcher"));
    }

    #[test]
    fn generate_writes_a_complete_project_and_replaces_an_old_one() {
        let dir = tempfile::TempDir::new().unwrap();
        let wrapper = dir.path().join("wrapper");
        fs::create_dir_all(wrapper.join("gradle/wrapper")).unwrap();
        for f in [
            "gradlew",
            "gradlew.bat",
            "gradle/wrapper/gradle-wrapper.jar",
            "gradle/wrapper/gradle-wrapper.properties",
        ] {
            fs::write(wrapper.join(f), f).unwrap();
        }
        let out = dir.path().join("host");
        let stale = out.join("app/src/main/res/values/old.xml");
        fs::create_dir_all(stale.parent().unwrap()).unwrap();
        fs::write(&stale, "x").unwrap();

        generate(&native(), &wrapper, &out).unwrap();
        for f in [
            "settings.gradle",
            "build.gradle.kts",
            "gradle.properties",
            "app/build.gradle.kts",
            "app/proguard-rules.pro",
            "app/src/main/AndroidManifest.xml",
            "app/src/main/res/values/styles.xml",
            "gradlew",
            "gradle/wrapper/gradle-wrapper.jar",
        ] {
            assert!(out.join(f).exists(), "{f} missing");
        }
        assert!(!stale.exists());
        assert!(fs::read_to_string(out.join("settings.gradle"))
            .unwrap()
            .contains("include ':app'"));

        // The app's own manifest is used as it is; its own styles replace the generated ones.
        let own = dir.path().join("AndroidManifest.xml");
        fs::write(&own, "<manifest>own</manifest>").unwrap();
        let res = dir.path().join("res");
        fs::create_dir_all(res.join("values")).unwrap();
        fs::write(res.join("values/styles.xml"), "<resources/>").unwrap();
        let mut cfg = native();
        cfg.manifest = Some(own);
        cfg.res_dirs = vec![res];
        generate(&cfg, &wrapper, &out).unwrap();
        assert_eq!(
            fs::read_to_string(out.join("app/src/main/AndroidManifest.xml")).unwrap(),
            "<manifest>own</manifest>"
        );
        assert!(!out.join("app/src/main/res/values/styles.xml").exists());
    }

    #[test]
    fn generate_writes_shims_for_the_apps_rust_components() {
        let dir = tempfile::TempDir::new().unwrap();
        let wrapper = dir.path().join("wrapper");
        fs::create_dir_all(wrapper.join("gradle/wrapper")).unwrap();
        for f in [
            "gradlew",
            "gradlew.bat",
            "gradle/wrapper/gradle-wrapper.jar",
            "gradle/wrapper/gradle-wrapper.properties",
        ] {
            fs::write(wrapper.join(f), f).unwrap();
        }
        let own = dir.path().join("AndroidManifest.xml");
        fs::write(
            &own,
            "<manifest><application><service android:name=\".Worker\" /></application></manifest>",
        )
        .unwrap();
        let mut cfg = native();
        cfg.manifest = Some(own);
        cfg.rust_lib = Some("main".into());
        let out = dir.path().join("host");
        generate(&cfg, &wrapper, &out).unwrap();
        let java = out.join("app/src/main/java");
        assert!(fs::read_to_string(java.join("com/example/app/Worker.java"))
            .unwrap()
            .contains("RustService"));
        assert!(java.join("com/mobilesentinel/rust/Lib_main.java").exists());
        cfg.rust_lib = None;
        assert!(generate(&cfg, &wrapper, &out)
            .unwrap_err()
            .contains("rust_lib"));
    }

    #[test]
    fn lists_and_ndk_names() {
        assert_eq!(parse_list("[\"a\", \"b c\" ,]"), vec!["a", "b c"]);
        assert_eq!(parse_list("single"), vec!["single"]);
        assert_eq!(gradle_path(Path::new(r"\\?\C:\x\y")), "C:/x/y");
    }
}
