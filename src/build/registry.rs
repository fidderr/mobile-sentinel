//! Capability registry — the source of truth for "what a consumer opts
//! into," used at build time to assemble a minimal Android manifest.
//!
//! # Why this exists
//!
//! mobile-sentinel's `android/` Gradle module compiles every Kotlin
//! component into the AAR, but a component is only *visible to Android*
//! (and to Google Play's manifest scan) if it is **declared** in the
//! merged manifest. The library manifest therefore declares only a
//! **baseline** that every alarm-shaped consumer needs. Everything that
//! is optional — and especially everything that is policy-sensitive
//! (Accessibility service, Device-admin receiver) — is declared **only
//! when the consumer enables its Cargo feature**:
//!
//! ```toml
//! # consumer Cargo.toml
//! mobile-sentinel = { version = "*", features = ["overlay", "camera", "sensors"] }
//! ```
//!
//! At build time `build_sentinel` calls [`assemble`] with the declared
//! list (derived from the consumer's enabled Cargo features) and injects
//! exactly those permissions + components into the consumer's manifest. A
//! capability the consumer does not enable never enters the shipped APK's
//! manifest, so a static scan never sees it.
//!
//! Capabilities are declared as **Cargo features** on the mobile-sentinel
//! dependency (the single, compile-enforced source of truth). build.rs maps
//! each enabled feature to a capability id here; the former manifest-only
//! capability `battery` is now a regular Cargo feature with a real door.
//!
//! # Scope
//!
//! This registry drives two build-time mechanisms:
//!
//! 1. **Manifest assembly** ([`assemble`]) — gates permissions + declared
//!    components. This is what Google Play's manifest scan keys on, so it
//!    is the part that removes policy exposure. Always applied.
//! 2. **Gradle module selection** ([`enabled_modules`]) — names the
//!    per-capability Gradle modules (`android/caps/<id>` → `:sentinel-<id>`)
//!    to include in the consumer build. `build_sentinel` wires `:sentinel-core`
//!    plus only the ENABLED capability modules, so a disabled capability's
//!    Kotlin and external Gradle dependencies are never compiled (structural
//!    trimming — no source exclusion, nothing optional).
//!
//! Cargo features tree-shake the native Rust `.so` (a disabled feature's code
//! is never compiled in); this registry does the equivalent for the Android
//! manifest + Kotlin/Gradle side. One enabled-feature set drives all three.

use once_cell::sync::Lazy;
use std::collections::{BTreeMap, BTreeSet};

// The shared capability-id list, included verbatim (the same file `build.rs`
// includes). Brings `CAPABILITY_IDS` and `FIRING_BUNDLE_IDS` into scope so the
// id list lives in exactly one place for both build-script and runtime.
include!("capability_ids.rs");

/// An Android `<uses-permission>` a capability requires.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Permission {
    /// Fully-qualified permission string, e.g. `"android.permission.CAMERA"`.
    pub name: &'static str,
    /// Optional `android:maxSdkVersion` — declare only up to this API.
    pub max_sdk: Option<u32>,
}

impl Permission {
    const fn new(name: &'static str) -> Self {
        Self {
            name,
            max_sdk: None,
        }
    }

    /// Render this permission as a manifest line.
    fn render(&self) -> String {
        match self.max_sdk {
            Some(max) => format!(
                "    <uses-permission android:name=\"{}\" android:maxSdkVersion=\"{}\" />",
                self.name, max
            ),
            None => format!("    <uses-permission android:name=\"{}\" />", self.name),
        }
    }
}

/// An Android `<uses-feature>` a capability declares (always `required=false`
/// so the app stays installable on devices lacking the hardware).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Feature {
    /// Feature name, e.g. `"android.hardware.camera"`.
    pub name: &'static str,
}

impl Feature {
    fn render(&self) -> String {
        format!(
            "    <uses-feature android:name=\"{}\" android:required=\"false\" />",
            self.name
        )
    }
}

/// A single opt-in capability: the permissions, hardware features, and
/// `<application>`-level component XML it contributes to the consumer's
/// manifest when declared.
#[derive(Debug, Clone, Copy)]
pub struct Capability {
    /// Stable lower-snake-case id used in `sentinel.toml`.
    pub id: &'static str,
    /// Human-readable purpose (printed by `build_sentinel`).
    pub description: &'static str,
    /// `<uses-permission>` entries this capability requires.
    pub permissions: &'static [Permission],
    /// `<uses-feature>` entries this capability declares.
    pub features: &'static [Feature],
    /// `<application>`-child component XML (services / receivers /
    /// activities). Class names are fully-qualified to `com.mobilesentinel`
    /// so they resolve correctly when injected into a consumer manifest
    /// whose package differs from the library's.
    pub components: &'static str,
    /// Whether Google Play review treats this capability as sensitive.
    /// `build_sentinel` prints a prominent warning when it is declared so
    /// the developer makes an informed choice.
    pub policy_sensitive: bool,
    /// Kotlin source file names that ONLY this capability needs. They live
    /// in this capability's Gradle module (`android/caps/<id>`), which is
    /// compiled into the consumer build only when the capability is enabled.
    /// A non-empty list is also what marks the capability as having a Gradle
    /// module (see [`enabled_modules`]).
    pub kotlin_sources: &'static [&'static str],
}

const NO_PERMS: &[Permission] = &[];
const NO_FEATURES: &[Feature] = &[];

/// All optional capabilities mobile-sentinel knows how to assemble.
///
/// Anything NOT in this table and NOT part of the baseline manifest is
/// unknown and reported back to the developer rather than silently
/// ignored.
pub const CAPABILITIES: &[Capability] = &[
    Capability {
        id: "overlay",
        description: "Force the firing UI to the foreground while the screen is on/unlocked (SYSTEM_ALERT_WINDOW).",
        permissions: &[Permission::new("android.permission.SYSTEM_ALERT_WINDOW")],
        features: NO_FEATURES,
        components: "",
        policy_sensitive: false,
        kotlin_sources: &["SentinelOverlayPrimitives.kt"],
    },
    Capability {
        id: "battery",
        description: "Request exemption from battery optimization for reliable firing on aggressive OEMs.",
        permissions: &[Permission::new(
            "android.permission.REQUEST_IGNORE_BATTERY_OPTIMIZATIONS",
        )],
        features: NO_FEATURES,
        components: "",
        policy_sensitive: false,
        kotlin_sources: &["SentinelBatteryPrimitives.kt"],
    },
    Capability {
        id: "torch",
        description: "Torch / flashlight (camera flash LED).",
        permissions: NO_PERMS,
        features: NO_FEATURES,
        components: "",
        policy_sensitive: false,
        kotlin_sources: &["SentinelTorchPrimitives.kt"],
    },
    Capability {
        id: "display",
        description: "Screen brightness control + keep-screen-on.",
        permissions: NO_PERMS,
        features: NO_FEATURES,
        components: "",
        policy_sensitive: false,
        kotlin_sources: &["SentinelDisplayPrimitives.kt"],
    },
    Capability {
        id: "screen_pin",
        description: "System screen-pinning (lock-task).",
        permissions: NO_PERMS,
        features: NO_FEATURES,
        components: "",
        policy_sensitive: false,
        kotlin_sources: &["SentinelScreenPinPrimitives.kt"],
    },
    Capability {
        id: "foregrounding",
        description: "Finish the app activity (close after dismiss/snooze).",
        permissions: NO_PERMS,
        features: NO_FEATURES,
        components: "",
        policy_sensitive: false,
        kotlin_sources: &["SentinelForegroundingPrimitives.kt"],
    },
    Capability {
        id: "scanner",
        description: "Barcode / QR scanning (CAMERA + ZXing + SentinelScannerActivity).",
        permissions: &[Permission::new("android.permission.CAMERA")],
        features: &[
            Feature {
                name: "android.hardware.camera",
            },
            Feature {
                name: "android.hardware.camera.autofocus",
            },
        ],
        // The scanner uses a TRANSLUCENT theme on purpose. A `singleInstance`
        // host (the lock-screen/firing path sets that) lives in a separate
        // task; an opaque scanner would drive the host to `onStop`, tearing
        // down its render surface — a WebView host then returns frozen. A
        // translucent window keeps the host in `onPause` (surface alive), so
        // the UI stays live. The scanner paints its own opaque backdrop, so it
        // still looks like a normal solid full-screen scanner.
        components: r#"        <activity
            android:name="com.mobilesentinel.SentinelScannerActivity"
            android:theme="@android:style/Theme.Translucent.NoTitleBar"
            android:screenOrientation="portrait"
            android:showWhenLocked="true"
            android:turnScreenOn="true" />"#,
        policy_sensitive: false,
        kotlin_sources: &["SentinelScannerActivity.kt", "SentinelScannerHelper.kt"],
    },
    Capability {
        id: "camera",
        description: "Take a photo via the system camera app (ACTION_IMAGE_CAPTURE + FileProvider).",
        // ACTION_IMAGE_CAPTURE itself needs no permission; declaring the
        // CAMERA permission would actually force the app to also hold it at
        // runtime, so we deliberately do NOT request it for the delegated
        // capture flow. The hardware feature is advertised as not-required.
        permissions: NO_PERMS,
        features: &[Feature {
            name: "android.hardware.camera",
        }],
        // A transparent proxy activity (delegates to the system camera) plus a
        // FileProvider so the captured full-res JPEG can be handed back. The
        // provider authority is "<package>.sentinelfileprovider"; the
        // `${applicationId}` placeholder is resolved by the manifest merger so
        // it stays correct in any consumer package. The referenced
        // `@xml/sentinel_camera_paths` ships in this module's resources.
        components: r#"        <activity
            android:name="com.mobilesentinel.SentinelCameraActivity"
            android:theme="@android:style/Theme.Translucent.NoTitleBar"
            android:excludeFromRecents="true" />
        <provider
            android:name="androidx.core.content.FileProvider"
            android:authorities="${applicationId}.sentinelfileprovider"
            android:exported="false"
            android:grantUriPermissions="true">
            <meta-data
                android:name="android.support.FILE_PROVIDER_PATHS"
                android:resource="@xml/sentinel_camera_paths" />
        </provider>"#,
        policy_sensitive: false,
        kotlin_sources: &["SentinelCameraActivity.kt", "SentinelCameraHelper.kt"],
    },
    Capability {
        id: "sensors",
        description: "Accelerometer shake counter + hardware step counter (ACTIVITY_RECOGNITION + high-rate sensors).",
        permissions: &[
            Permission::new("android.permission.ACTIVITY_RECOGNITION"),
            Permission::new("android.permission.HIGH_SAMPLING_RATE_SENSORS"),
        ],
        features: NO_FEATURES,
        components: "",
        policy_sensitive: false,
        kotlin_sources: &["SentinelSensorHelper.kt"],
    },
    Capability {
        id: "audio",
        description: "Non-firing audio preview (play/stop/volume of arbitrary URIs).",
        permissions: NO_PERMS,
        features: NO_FEATURES,
        components: "",
        policy_sensitive: false,
        kotlin_sources: &["SentinelAudioPreviewPrimitives.kt"],
    },
    Capability {
        id: "haptics",
        description: "Vibration / haptic feedback (VIBRATE).",
        permissions: &[Permission::new("android.permission.VIBRATE")],
        features: NO_FEATURES,
        components: "",
        policy_sensitive: false,
        kotlin_sources: &["SentinelHapticPrimitives.kt"],
    },
    Capability {
        id: "permissions",
        description: "Runtime permission status/request + app-settings deep link.",
        permissions: NO_PERMS,
        features: NO_FEATURES,
        components: "",
        policy_sensitive: false,
        kotlin_sources: &["SentinelPermissionPrimitives.kt"],
    },
    Capability {
        id: "media_picker",
        description: "Native file picker for importing custom sounds (SentinelFilePickerActivity).",
        permissions: NO_PERMS,
        features: NO_FEATURES,
        components: r#"        <activity
            android:name="com.mobilesentinel.SentinelFilePickerActivity"
            android:theme="@android:style/Theme.Translucent.NoTitleBar"
            android:excludeFromRecents="true" />"#,
        policy_sensitive: false,
        kotlin_sources: &["SentinelFilePickerHelper.kt"],
    },
    Capability {
        id: "biometric",
        description: "Biometric (fingerprint/face) authentication prompt (BiometricPrompt).",
        permissions: NO_PERMS,
        features: NO_FEATURES,
        components: "",
        policy_sensitive: false,
        kotlin_sources: &[
            "SentinelBiometricHelper.kt",
            "SentinelBiometricsPrimitives.kt",
        ],
    },
    Capability {
        id: "accessibility",
        description: "Ultra-protection: relaunch the firing activity if the user reaches Settings/power-menu while a kiosk session is active (SentinelAccessibilityService).",
        permissions: NO_PERMS,
        features: NO_FEATURES,
        components: r#"        <service
            android:name="com.mobilesentinel.SentinelAccessibilityService"
            android:enabled="true"
            android:exported="true"
            android:permission="android.permission.BIND_ACCESSIBILITY_SERVICE">
            <intent-filter>
                <action android:name="android.accessibilityservice.AccessibilityService" />
            </intent-filter>
            <meta-data
                android:name="android.accessibilityservice"
                android:resource="@xml/sentinel_accessibility_config" />
        </service>"#,
        policy_sensitive: true,
        kotlin_sources: &["SentinelAccessibilityService.kt"],
    },
    Capability {
        id: "device_admin",
        description: "Device-admin force-lock support (SentinelAdminReceiver).",
        permissions: NO_PERMS,
        features: NO_FEATURES,
        components: r#"        <receiver
            android:name="com.mobilesentinel.SentinelAdminReceiver"
            android:permission="android.permission.BIND_DEVICE_ADMIN"
            android:exported="true">
            <meta-data
                android:name="android.app.device_admin"
                android:resource="@xml/sentinel_device_admin" />
            <intent-filter>
                <action android:name="android.app.action.DEVICE_ADMIN_ENABLED" />
            </intent-filter>
        </receiver>"#,
        policy_sensitive: true,
        kotlin_sources: &[
            "SentinelAdminReceiver.kt",
            "SentinelDeviceAdminPrimitives.kt",
        ],
    },
    Capability {
        id: "clipboard",
        description: "Clipboard get/set.",
        permissions: NO_PERMS,
        features: NO_FEATURES,
        components: "",
        policy_sensitive: false,
        kotlin_sources: &["SentinelClipboardPrimitives.kt"],
    },
    Capability {
        id: "share",
        description: "System share sheet.",
        permissions: NO_PERMS,
        features: NO_FEATURES,
        components: "",
        policy_sensitive: false,
        kotlin_sources: &["SentinelSharePrimitives.kt"],
    },
    Capability {
        id: "secure_storage",
        description: "Encrypted key/value storage.",
        permissions: NO_PERMS,
        features: NO_FEATURES,
        components: "",
        policy_sensitive: false,
        kotlin_sources: &["SentinelSecureStoragePrimitives.kt"],
    },
    Capability {
        id: "sms",
        description: "Send SMS text messages.",
        permissions: &[Permission::new("android.permission.SEND_SMS")],
        features: NO_FEATURES,
        components: "",
        policy_sensitive: true,
        kotlin_sources: &["SentinelSmsPrimitives.kt"],
    },
    Capability {
        id: "phone",
        description: "Dial numbers / query call state.",
        permissions: &[Permission::new("android.permission.CALL_PHONE")],
        features: NO_FEATURES,
        components: "",
        policy_sensitive: true,
        kotlin_sources: &["SentinelPhonePrimitives.kt"],
    },
    Capability {
        id: "network",
        description: "Network connectivity status.",
        permissions: &[Permission::new("android.permission.ACCESS_NETWORK_STATE")],
        features: NO_FEATURES,
        components: "",
        policy_sensitive: false,
        kotlin_sources: &["SentinelNetworkPrimitives.kt"],
    },
    Capability {
        id: "location",
        description: "Current device location.",
        permissions: &[
            Permission::new("android.permission.ACCESS_FINE_LOCATION"),
            Permission::new("android.permission.ACCESS_COARSE_LOCATION"),
        ],
        features: NO_FEATURES,
        components: "",
        policy_sensitive: true,
        kotlin_sources: &["SentinelLocationPrimitives.kt"],
    },
    Capability {
        id: "maps",
        description: "Geocoding / reverse-geocoding.",
        permissions: NO_PERMS,
        features: NO_FEATURES,
        components: "",
        policy_sensitive: false,
        kotlin_sources: &["SentinelMapsPrimitives.kt"],
    },
    Capability {
        id: "contacts",
        description: "Read device contacts.",
        permissions: &[Permission::new("android.permission.READ_CONTACTS")],
        features: NO_FEATURES,
        components: "",
        policy_sensitive: true,
        kotlin_sources: &["SentinelContactsPrimitives.kt"],
    },
    Capability {
        id: "calendar",
        description: "Read/create/delete calendar events.",
        permissions: &[
            Permission::new("android.permission.READ_CALENDAR"),
            Permission::new("android.permission.WRITE_CALENDAR"),
        ],
        features: NO_FEATURES,
        components: "",
        policy_sensitive: true,
        kotlin_sources: &["SentinelCalendarPrimitives.kt"],
    },
    Capability {
        id: "dismiss_guard",
        description: "Block back/swipe dismissal during firing.",
        permissions: NO_PERMS,
        features: NO_FEATURES,
        components: "",
        policy_sensitive: false,
        kotlin_sources: &["SentinelDismissGuardPrimitives.kt"],
    },
    Capability {
        id: "notifications",
        description: "General notifications (post/update/cancel).",
        permissions: NO_PERMS,
        features: NO_FEATURES,
        components: "",
        policy_sensitive: false,
        kotlin_sources: &["SentinelNotificationPrimitives.kt"],
    },
    Capability {
        id: "file_system",
        description: "Bundled-asset copy/list helpers.",
        permissions: NO_PERMS,
        features: NO_FEATURES,
        components: "",
        policy_sensitive: false,
        kotlin_sources: &["SentinelFileSystemPrimitives.kt"],
    },
    // ---- Firing sub-features -------------------------------------------
    // The firing surface, split into independent per-feature modules so a
    // consumer ships only the surfaces (and Kotlin) it actually uses. Each is
    // its own `:sentinel-<id>` Gradle module under `android/caps/<id>`, just
    // like every leaf capability — there is no shared "alarm runtime" bundle.
    Capability {
        id: "foreground_service",
        description: "Alarm foreground service so the process survives a swipe-kill while firing.",
        permissions: &[
            Permission::new("android.permission.FOREGROUND_SERVICE"),
            Permission::new("android.permission.FOREGROUND_SERVICE_MEDIA_PLAYBACK"),
            Permission::new("android.permission.POST_NOTIFICATIONS"),
        ],
        features: NO_FEATURES,
        components: "",
        policy_sensitive: false,
        kotlin_sources: &[
            "SentinelForegroundService.kt",
            "SentinelForegroundServicePrimitives.kt",
        ],
    },
    Capability {
        id: "firing_audio",
        description: "Alarm audio playback during firing.",
        permissions: NO_PERMS,
        features: NO_FEATURES,
        components: "",
        policy_sensitive: false,
        kotlin_sources: &["SentinelFiringAudioPrimitives.kt"],
    },
    Capability {
        id: "firing_vibration",
        description: "Alarm vibration during firing (looping waveform alongside or instead of audio).",
        permissions: &[Permission::new("android.permission.VIBRATE")],
        features: NO_FEATURES,
        components: "",
        policy_sensitive: false,
        kotlin_sources: &["SentinelFiringVibrationPrimitives.kt"],
    },
    Capability {
        id: "wake_lock",
        description: "Keep the CPU awake while the alarm fires (WAKE_LOCK).",
        permissions: &[Permission::new("android.permission.WAKE_LOCK")],
        features: NO_FEATURES,
        components: "",
        policy_sensitive: false,
        kotlin_sources: &["SentinelWakeLockPrimitives.kt"],
    },
    Capability {
        id: "exact_alarm",
        description: "Schedule OS exact alarms (SCHEDULE_EXACT_ALARM / USE_EXACT_ALARM) + boot/time re-arm.",
        permissions: &[
            Permission::new("android.permission.SCHEDULE_EXACT_ALARM"),
            Permission::new("android.permission.USE_EXACT_ALARM"),
        ],
        features: NO_FEATURES,
        components: "",
        policy_sensitive: false,
        kotlin_sources: &[
            "SentinelExactAlarmPrimitives.kt",
            "SentinelAlarmReceiver.kt",
            "SentinelTimeChangeReceiver.kt",
            "SentinelBootReceiver.kt",
            "SentinelBootService.kt",
        ],
    },
    Capability {
        id: "kiosk",
        description: "Kiosk / lock-task takeover while firing (keep the activity in the foreground).",
        permissions: NO_PERMS,
        features: NO_FEATURES,
        components: "",
        policy_sensitive: false,
        kotlin_sources: &[
            "SentinelKioskController.kt",
            "SentinelKioskPrimitives.kt",
            "SentinelKioskInitializer.kt",
        ],
    },
    Capability {
        id: "full_screen_intent",
        description: "Full-screen alarm intent over the lock screen (USE_FULL_SCREEN_INTENT).",
        permissions: &[Permission::new("android.permission.USE_FULL_SCREEN_INTENT")],
        features: NO_FEATURES,
        components: "",
        policy_sensitive: false,
        kotlin_sources: &["SentinelFullScreenIntentPrimitives.kt"],
    },
    // ---- Building blocks with a Kotlin surface -------------------------
    Capability {
        id: "jobs",
        description: "Cross-process job guardian (the :sentinel process that keeps MAIN alive / resurrects it).",
        permissions: NO_PERMS,
        features: NO_FEATURES,
        components: "",
        policy_sensitive: false,
        kotlin_sources: &["SentinelJobGuardian.kt", "SentinelJobsInitializer.kt"],
    },
];

/// Look up a capability by its `sentinel.toml` id.
pub fn capability(id: &str) -> Option<&'static Capability> {
    CAPABILITIES.iter().find(|c| c.id == id)
}

/// Map a Cargo **feature name** (as written in a consumer's `Cargo.toml`,
/// kebab-case, e.g. `"full-screen-intent"`) to the capability **id** it
/// contributes to the manifest/Gradle layer (snake_case, e.g.
/// `"full_screen_intent"`).
///
/// This is the deterministic, source-of-truth mapping `build_sentinel` uses
/// when scoping to a specific consumer via `--app` (reading that app's
/// `Cargo.toml` feature list), so a multi-consumer workspace never has to
/// guess the feature set from build-script output timestamps.
///
/// The mapping is pure **kebab→snake of the feature name**: a feature `foo-bar`
/// contributes capability id `foo_bar`. The id is then looked up in
/// [`CAPABILITIES`] — so the table is the single source of truth and there is
/// no second hand-written list to keep in sync. Composite/building-block
/// features that contribute no manifest capability of their own (e.g.
/// `alarm-kit`, `recipes`, `firing`, `sound-library`, `jobs`, `state-store`)
/// have no row and therefore map to `None`; their constituent firing
/// sub-features are listed explicitly by the consumer (or by the `firing`
/// bundle, expanded via [`FEATURE_EDGES`]). Returns `None` for unknown /
/// non-capability features.
pub fn cargo_feature_to_capability(feature: &str) -> Option<&'static str> {
    let snake = feature.trim().replace('-', "_");
    CAPABILITIES.iter().find(|c| c.id == snake).map(|c| c.id)
}

/// The mobile-sentinel crate's own `Cargo.toml`, embedded at compile time.
/// This is the SINGLE source for the feature dependency graph: the `[features]`
/// table is parsed once (below) instead of being re-listed by hand. Path is
/// relative to this file (`src/build/registry.rs`) → crate root.
const CRATE_CARGO_TOML: &str = include_str!("../../Cargo.toml");

/// Transitive Cargo `[features]` edges (feature → the features it pulls in),
/// parsed directly from the crate's own `Cargo.toml [features]` table — so
/// there is NO hand-maintained mirror to drift. Used by
/// [`capabilities_for_features`] so the `--app` capability derivation matches
/// exactly what the compiled `.so` enabled (Cargo resolves these edges at
/// compile time; we resolve the same closure from the same declarations).
///
/// Built lazily on first use. The map only contains features that have at
/// least one dependency; a leaf feature (`camera = []`) simply has no entry.
static FEATURE_EDGES: Lazy<BTreeMap<String, Vec<String>>> =
    Lazy::new(|| parse_feature_edges(CRATE_CARGO_TOML));

/// Gradle module-dependency edges: a `:sentinel-<module>` that declares
/// `implementation(project(":sentinel-<dep>"))` in its `build.gradle.kts`.
/// [`enabled_modules`] closes the included-module set over these so
/// settings.gradle never references a module it didn't `include`.
///
/// - `exact_alarm` → `jobs`: a scheduled OS wake activates a job and starts the
///   guardian (the one honest edge — scheduling needs the job system to bring
///   MAIN back at fire time).
/// - `accessibility` → `kiosk`: the accessibility service is the kiosk
///   ultra-protection enforcer; it reads `SentinelKioskController`.
const MODULE_DEPS: &[(&str, &[&str])] =
    &[("exact_alarm", &["jobs"]), ("accessibility", &["kiosk"])];

/// `[` minus `]` count in a fragment — used to track when a (possibly
/// multi-line) `name = [ ... ]` feature entry is complete.
fn bracket_delta(s: &str) -> i32 {
    s.chars().fold(0, |acc, c| match c {
        '[' => acc + 1,
        ']' => acc - 1,
        _ => acc,
    })
}

/// Every double-quoted token in a fragment, in order. Feature dependencies in
/// Cargo.toml are always quoted strings, so this extracts a feature's deps
/// without caring about commas / whitespace / line breaks.
fn extract_quoted(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_quote = false;
    for ch in s.chars() {
        if ch == '"' {
            if in_quote {
                out.push(std::mem::take(&mut cur));
            }
            in_quote = !in_quote;
        } else if in_quote {
            cur.push(ch);
        }
    }
    out
}

/// Parse the `[features]` table of a `Cargo.toml` into `feature → deps`. Only
/// features with at least one dependency are kept (leaf `foo = []` entries map
/// to nothing). Handles single-line and multi-line array values and strips
/// `#` comments. This is the one place the feature graph is read; everything
/// else derives from it.
fn parse_feature_edges(cargo_toml: &str) -> BTreeMap<String, Vec<String>> {
    let mut map: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut in_features = false;
    let mut name: Option<String> = None;
    let mut buf = String::new();
    let mut depth: i32 = 0;

    for raw in cargo_toml.lines() {
        // Strip a `#` comment (feature names/deps never contain `#`).
        let line = match raw.find('#') {
            Some(i) => &raw[..i],
            None => raw,
        };
        let line = line.trim();

        if !in_features {
            if line == "[features]" {
                in_features = true;
            }
            continue;
        }
        // A new `[section]` header at top level ends the features table.
        if depth == 0 && line.starts_with('[') {
            break;
        }
        if line.is_empty() {
            continue;
        }

        if depth == 0 {
            // Start of an entry: `name = [ ...`
            let Some((n, rest)) = line.split_once('=') else {
                continue;
            };
            name = Some(n.trim().to_string());
            buf.clear();
            buf.push_str(rest);
            depth += bracket_delta(rest);
        } else {
            // Continuation of a multi-line array value.
            buf.push(' ');
            buf.push_str(line);
            depth += bracket_delta(line);
        }

        if depth <= 0 {
            if let Some(nm) = name.take() {
                let deps = extract_quoted(&buf);
                if !deps.is_empty() {
                    map.insert(nm, deps);
                }
            }
            depth = 0;
            buf.clear();
        }
    }
    map
}

/// Expand a declared feature list into its full transitive closure over
/// [`FEATURE_EDGES`] (Cargo enables a feature's dependencies too, so the
/// shipped `.so` contains them). Deterministic + de-duplicated.
fn expand_feature_closure<S: AsRef<str>>(features: &[S]) -> BTreeSet<String> {
    let mut closure: BTreeSet<String> = BTreeSet::new();
    let mut stack: Vec<String> = features
        .iter()
        .map(|s| s.as_ref().trim().to_string())
        .collect();
    while let Some(f) = stack.pop() {
        if !closure.insert(f.clone()) {
            continue; // already visited
        }
        if let Some(deps) = FEATURE_EDGES.get(&f) {
            for dep in deps {
                if !closure.contains(dep) {
                    stack.push(dep.clone());
                }
            }
        }
    }
    closure
}

/// Expand a consumer's declared Cargo feature list into the capability ids it
/// enables. First resolves the full transitive feature closure (so a feature
/// that pulls in others — e.g. `dismiss_guard = ["screen_pin"]`,
/// `alarm-kit → firing`— contributes ALL the capabilities its
/// closure enables), then maps each resolved feature to its capability id.
///
/// Used by `build_sentinel --app` to derive the exact capability set from the
/// app's `Cargo.toml`, matching what the compiled `.so` enabled — so the
/// manifest + Gradle module set never disagree with the native build.
/// Deterministic, de-duplicated, sorted.
pub fn capabilities_for_features<S: AsRef<str>>(features: &[S]) -> Vec<String> {
    let closure = expand_feature_closure(features);
    let mut out: BTreeSet<String> = BTreeSet::new();
    for f in &closure {
        if let Some(id) = cargo_feature_to_capability(f) {
            out.insert(id.to_string());
        }
    }
    out.into_iter().collect()
}

/// The result of assembling a set of declared capabilities into manifest
/// fragments. Permissions and features are de-duplicated and sorted for
/// deterministic output.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ManifestInjection {
    /// `<uses-permission>` / `<uses-feature>` lines for the `<manifest>` scope.
    pub manifest_lines: Vec<String>,
    /// `<application>`-child component XML blocks.
    pub application_components: Vec<String>,
    /// Declared capability ids that are not recognized.
    pub unknown: Vec<String>,
    /// Human-readable warnings for policy-sensitive capabilities that were
    /// declared. Empty when none are enabled.
    pub policy_warnings: Vec<String>,
}

/// The Gradle capability-module names (`android/caps/<id>` → `:sentinel-<id>`)
/// that must be included in the consumer build for the given declared
/// capabilities.
///
/// A capability has a module exactly when it lists [`Capability::kotlin_sources`];
/// the module dir name equals the id. Every firing sub-feature
/// (`foreground_service` / `firing_audio` / `wake_lock` / `exact_alarm` /
/// `kiosk` / `full_screen_intent`) and the `jobs` building block is now its
/// own independent module — there is no shared "alarm runtime" bundle.
///
/// Some modules depend on others at the Gradle level (`implementation
/// project(...)`); those edges are in [`MODULE_DEPS`]. settings.gradle must
/// `include` every referenced module, so the returned set is closed over those
/// edges (e.g. declaring `exact_alarm` also includes `jobs`; `accessibility`
/// also includes `kiosk`).
///
/// `build_sentinel` includes `:sentinel-core` plus every module returned here.
/// A capability whose module is not returned is never added to
/// `settings.gradle`, so its Kotlin + external Gradle deps are never compiled
/// (structural trimming). De-duplicated and sorted for deterministic output.
pub fn enabled_modules<S: AsRef<str>>(declared: &[S]) -> Vec<&'static str> {
    let declared_set: BTreeSet<&str> = declared.iter().map(|s| s.as_ref().trim()).collect();
    let mut out: BTreeSet<&'static str> = BTreeSet::new();
    for cap in CAPABILITIES {
        if declared_set.contains(cap.id) && !cap.kotlin_sources.is_empty() {
            out.insert(cap.id);
        }
    }

    // Close over Gradle module-dependency edges: a module that is included
    // must have every module it `implementation project(...)`-depends on also
    // included in settings.gradle. Iterate to a fixpoint (edges are shallow).
    loop {
        let mut added = false;
        for (module, deps) in MODULE_DEPS {
            if out.contains(module) {
                for dep in *deps {
                    if out.insert(dep) {
                        added = true;
                    }
                }
            }
        }
        if !added {
            break;
        }
    }

    out.into_iter().collect()
}

/// Assemble manifest fragments for the given declared capability ids.
///
/// - Unknown ids are collected in [`ManifestInjection::unknown`] (never
///   silently dropped).
/// - Permissions/features are de-duplicated across capabilities.
/// - Policy-sensitive capabilities produce a warning string so the build
///   tool can surface them prominently.
pub fn assemble<S: AsRef<str>>(declared: &[S]) -> ManifestInjection {
    let mut perm_lines: BTreeSet<String> = BTreeSet::new();
    let mut feature_lines: BTreeSet<String> = BTreeSet::new();
    let mut components: Vec<String> = Vec::new();
    let mut unknown: Vec<String> = Vec::new();
    let mut policy_warnings: Vec<String> = Vec::new();

    // De-dup declared ids while preserving first-seen order for components.
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for raw in declared {
        let id = raw.as_ref().trim();
        if id.is_empty() || !seen.insert(id.to_string()) {
            continue;
        }
        match capability(id) {
            Some(cap) => {
                for p in cap.permissions {
                    perm_lines.insert(p.render());
                }
                for f in cap.features {
                    feature_lines.insert(f.render());
                }
                if !cap.components.is_empty() {
                    components.push(cap.components.to_string());
                }
                if cap.policy_sensitive {
                    policy_warnings.push(format!(
                        "capability '{}' is POLICY-SENSITIVE on Google Play: {}",
                        cap.id, cap.description
                    ));
                }
            }
            None => unknown.push(id.to_string()),
        }
    }

    // Permissions first, then features, both sorted for determinism.
    let mut manifest_lines: Vec<String> = perm_lines.into_iter().collect();
    manifest_lines.extend(feature_lines);

    ManifestInjection {
        manifest_lines,
        application_components: components,
        unknown,
        policy_warnings,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn baseline_table_does_not_contain_policy_pieces_implicitly() {
        // Nothing is enabled unless explicitly declared.
        let injection = assemble::<&str>(&[]);
        assert!(injection.manifest_lines.is_empty());
        assert!(injection.application_components.is_empty());
        assert!(injection.policy_warnings.is_empty());
        assert!(injection.unknown.is_empty());
    }

    #[test]
    fn accessibility_is_opt_in_and_flagged() {
        let injection = assemble(&["accessibility"]);
        assert_eq!(injection.application_components.len(), 1);
        assert!(injection.application_components[0]
            .contains("com.mobilesentinel.SentinelAccessibilityService"));
        assert_eq!(injection.policy_warnings.len(), 1);
        assert!(injection.policy_warnings[0].contains("accessibility"));
    }

    #[test]
    fn device_admin_is_opt_in_and_flagged() {
        let injection = assemble(&["device_admin"]);
        assert!(injection.application_components[0]
            .contains("com.mobilesentinel.SentinelAdminReceiver"));
        assert_eq!(injection.policy_warnings.len(), 1);
    }

    #[test]
    fn scanner_brings_permission_feature_and_activity() {
        let injection = assemble(&["scanner"]);
        assert!(injection
            .manifest_lines
            .iter()
            .any(|l| l.contains("android.permission.CAMERA")));
        assert!(injection
            .manifest_lines
            .iter()
            .any(|l| l.contains("android.hardware.camera")));
        assert!(injection.application_components[0]
            .contains("com.mobilesentinel.SentinelScannerActivity"));
        assert!(injection.policy_warnings.is_empty());
    }

    #[test]
    fn camera_brings_capture_proxy_and_fileprovider_no_permission() {
        // The `camera` capability delegates to the system camera app
        // (ACTION_IMAGE_CAPTURE), so it needs NO CAMERA permission — it ships
        // a transparent capture proxy + a FileProvider, and advertises the
        // camera hardware feature as not-required.
        let injection = assemble(&["camera"]);
        assert!(
            !injection
                .manifest_lines
                .iter()
                .any(|l| l.contains("android.permission.CAMERA")),
            "system-camera capture must not request the CAMERA permission"
        );
        assert!(injection
            .manifest_lines
            .iter()
            .any(|l| l.contains("android.hardware.camera")));
        let components = injection.application_components.join("\n");
        assert!(components.contains("com.mobilesentinel.SentinelCameraActivity"));
        assert!(components.contains("androidx.core.content.FileProvider"));
        assert!(injection.policy_warnings.is_empty());
    }

    #[test]
    fn enabled_modules_includes_only_declared_with_kotlin() {
        // scanner is declared and has a Kotlin module → included; the
        // undeclared capabilities' modules (sms/contacts/location) are not.
        let modules = enabled_modules(&["scanner"]);
        assert!(modules.contains(&"scanner"));
        assert!(!modules.contains(&"sms"));
        assert!(!modules.contains(&"contacts"));
        assert!(!modules.contains(&"location"));
    }

    #[test]
    fn enabled_modules_firing_subfeature_is_its_own_module() {
        // Each firing sub-feature is now its own per-feature module (no shared
        // "alarm runtime" bundle). wake_lock → :sentinel-wake_lock, nothing else.
        let modules = enabled_modules(&["wake_lock"]);
        assert_eq!(modules, vec!["wake_lock"]);
    }

    #[test]
    fn enabled_modules_exact_alarm_pulls_jobs() {
        // exact_alarm's Gradle module depends on :sentinel-jobs (a scheduled
        // OS wake activates a job), so including exact_alarm closes over jobs.
        let modules = enabled_modules(&["exact_alarm"]);
        assert!(modules.contains(&"exact_alarm"));
        assert!(modules.contains(&"jobs"));

        // accessibility's module depends on the kiosk module (it reads
        // SentinelKioskController), so enabling accessibility includes kiosk.
        let modules = enabled_modules(&["accessibility"]);
        assert!(modules.contains(&"accessibility"));
        assert!(modules.contains(&"kiosk"));
    }

    #[test]
    fn enabled_modules_jobs_is_standalone() {
        // jobs can be enabled with NO firing surface and NO kiosk — it is a
        // self-contained module (the cross-process guardian).
        let modules = enabled_modules(&["jobs"]);
        assert_eq!(modules, vec!["jobs"]);
        assert!(!modules.contains(&"kiosk"));
    }

    #[test]
    fn enabled_modules_lists_each_declared_kotlin_capability() {
        let modules = enabled_modules(&["sms", "contacts", "camera"]);
        assert!(modules.contains(&"sms"));
        assert!(modules.contains(&"contacts"));
        assert!(modules.contains(&"camera"));
        // No firing sub-feature → none of the firing modules.
        assert!(!modules.contains(&"kiosk"));
        assert!(!modules.contains(&"jobs"));
        assert_eq!(modules.len(), 3);
    }

    #[test]
    fn enabled_modules_is_deterministic_and_deduped() {
        let a = enabled_modules(&["camera", "sms"]);
        let b = enabled_modules(&["sms", "camera"]);
        assert_eq!(a, b);
        let mut sorted = a.clone();
        sorted.dedup();
        assert_eq!(a.len(), sorted.len());
    }

    #[test]
    fn cargo_feature_maps_kebab_to_snake_capability() {
        assert_eq!(
            cargo_feature_to_capability("full-screen-intent"),
            Some("full_screen_intent")
        );
        assert_eq!(cargo_feature_to_capability("battery"), Some("battery"));
        assert_eq!(cargo_feature_to_capability("camera"), Some("camera"));
        // Composite/building-block features contribute no capability of their own.
        assert_eq!(cargo_feature_to_capability("alarm-kit"), None);
        assert_eq!(cargo_feature_to_capability("sound-library"), None);
        assert_eq!(cargo_feature_to_capability("nonsense"), None);
    }

    #[test]
    fn capability_ids_match_capabilities_table() {
        // The shared `CAPABILITY_IDS` list (used by build.rs) and the
        // `CAPABILITIES` metadata table (used at build time) must describe the
        // exact same set — adding to one without the other is the classic
        // drift bug this test exists to catch.
        let ids: BTreeSet<&str> = CAPABILITY_IDS.iter().copied().collect();
        let table: BTreeSet<&str> = CAPABILITIES.iter().map(|c| c.id).collect();
        let missing_row: Vec<&&str> = ids.difference(&table).collect();
        let missing_id: Vec<&&str> = table.difference(&ids).collect();
        assert!(
            missing_row.is_empty(),
            "ids in CAPABILITY_IDS with no CAPABILITIES row: {missing_row:?}"
        );
        assert!(
            missing_id.is_empty(),
            "CAPABILITIES rows missing from CAPABILITY_IDS: {missing_id:?}"
        );
    }

    #[test]
    fn firing_bundle_ids_are_known_capabilities() {
        // Every firing sub-feature in the bundle must be a real capability id.
        let ids: BTreeSet<&str> = CAPABILITY_IDS.iter().copied().collect();
        for id in FIRING_BUNDLE_IDS {
            assert!(ids.contains(id), "FIRING_BUNDLE_IDS has unknown id `{id}`");
        }
    }

    #[test]
    fn capabilities_for_features_expands_alarm_kit_to_firing_surface() {
        // alarm-kit composes the full firing surface.
        let caps = capabilities_for_features(&["alarm-kit"]);
        for id in FIRING_BUNDLE_IDS {
            assert!(caps.contains(&id.to_string()), "missing {id}");
        }
        // No leaf capabilities unless declared.
        assert!(!caps.contains(&"camera".to_string()));
    }

    #[test]
    fn capabilities_for_features_matches_alarmfree_declared_set() {
        // The exact feature list alarmfree declares in its Cargo.toml.
        let caps = capabilities_for_features(&[
            "alarm-kit",
            "scanner",
            "haptics",
            "audio",
            "overlay",
            "permissions",
            "sensors",
            "foregrounding",
            "media_picker",
        ]);
        // Leaf capabilities present.
        for id in [
            "scanner",
            "haptics",
            "audio",
            "overlay",
            "permissions",
            "sensors",
            "foregrounding",
            "media_picker",
        ] {
            assert!(caps.contains(&id.to_string()), "missing leaf {id}");
        }
        // Firing surface (from alarm-kit) present.
        for id in FIRING_BUNDLE_IDS {
            assert!(caps.contains(&id.to_string()), "missing firing {id}");
        }
        // No undeclared sensitive capabilities leaked.
        for id in ["sms", "contacts", "location", "calendar", "device_admin"] {
            assert!(!caps.contains(&id.to_string()), "leaked {id}");
        }
    }

    #[test]
    fn capabilities_for_features_empty_is_husk() {
        // No features → no capabilities (the husk: kernel only).
        let caps = capabilities_for_features::<&str>(&[]);
        assert!(caps.is_empty());
    }

    #[test]
    fn capabilities_for_features_resolves_transitive_dismiss_guard_edge() {
        // dismiss_guard = ["screen_pin"] — enabling dismiss_guard alone must
        // also enable screen_pin (its Kotlin module depends on screen_pin's),
        // matching what Cargo compiles into the `.so`.
        let caps = capabilities_for_features(&["dismiss_guard"]);
        assert!(caps.contains(&"dismiss_guard".to_string()));
        assert!(
            caps.contains(&"screen_pin".to_string()),
            "dismiss_guard must transitively pull screen_pin"
        );
        // And the screen_pin module must therefore be wired.
        let modules = enabled_modules(&caps);
        assert!(modules.contains(&"screen_pin"));
        assert!(modules.contains(&"dismiss_guard"));
    }

    #[test]
    fn capabilities_for_features_resolves_accessibility_to_kiosk() {
        // accessibility = ["kiosk"] — pulls the kiosk capability, and its
        // Gradle module depends on the kiosk module the service relaunches
        // through.
        let caps = capabilities_for_features(&["accessibility"]);
        assert!(caps.contains(&"accessibility".to_string()));
        assert!(caps.contains(&"kiosk".to_string()));
        let modules = enabled_modules(&caps);
        assert!(modules.contains(&"accessibility"));
        assert!(modules.contains(&"kiosk"));
    }

    #[test]
    fn feature_edges_reference_only_known_features() {
        // Every dependency named in the parsed feature graph must itself be a
        // known feature: a capability feature, a building-block/bundle feature,
        // or another edge key. Guards against a typo in Cargo.toml's [features].
        let edge_keys: BTreeSet<&str> = FEATURE_EDGES.keys().map(|k| k.as_str()).collect();
        let building_blocks = ["state-store", "sound-library", "jobs", "firing"];
        for deps in FEATURE_EDGES.values() {
            for dep in deps {
                let known = cargo_feature_to_capability(dep).is_some()
                    || edge_keys.contains(dep.as_str())
                    || building_blocks.contains(&dep.as_str());
                assert!(known, "feature graph references unknown feature `{dep}`");
            }
        }
    }

    #[test]
    fn feature_graph_parsed_from_cargo_toml() {
        // The graph is parsed from the crate's own Cargo.toml — not hand-listed
        // — so these well-known edges must be present exactly as declared.
        // (If someone edits [features] in Cargo.toml, this is what tracks it.)
        let deps_of = |feat: &str| -> Option<Vec<&str>> {
            FEATURE_EDGES
                .get(feat)
                .map(|d| d.iter().map(String::as_str).collect())
        };
        assert_eq!(
            deps_of("alarm-kit"),
            Some(vec!["recipes", "firing", "sound-library", "jobs"])
        );
        assert_eq!(deps_of("recipes"), Some(vec!["state-store"]));
        assert_eq!(deps_of("dismiss_guard"), Some(vec!["screen_pin"]));
        // Leaf features (no deps) must NOT appear as edge keys.
        assert!(deps_of("camera").is_none());
    }

    #[test]
    fn firing_definition_matches_shared_bundle() {
        // Cargo.toml's `firing = [...]` (parsed here) and `FIRING_BUNDLE_IDS`
        // (the const build.rs + the registry share) describe the SAME 6
        // surfaces. Cargo declares them kebab-case; the const is snake_case —
        // normalize before comparing. This is the drift guard between the two
        // remaining copies (Cargo.toml, which Cargo requires, and the const,
        // which the build script needs because it can't read Cargo.toml).
        let from_cargo: BTreeSet<String> = FEATURE_EDGES
            .get("firing")
            .expect("firing edge present in Cargo.toml")
            .iter()
            .map(|f| f.replace('-', "_"))
            .collect();
        let from_const: BTreeSet<String> =
            FIRING_BUNDLE_IDS.iter().map(|s| s.to_string()).collect();
        assert_eq!(
            from_cargo, from_const,
            "Cargo.toml `firing` members and FIRING_BUNDLE_IDS must match"
        );
        // And resolving the `firing` feature must expand to exactly those ids.
        let got: BTreeSet<String> = capabilities_for_features(&["firing"]).into_iter().collect();
        assert_eq!(got, from_const, "firing must expand to exactly its bundle");
    }

    #[test]
    fn permissions_are_deduped_across_capabilities() {
        // overlay + camera + overlay (dup) → one SYSTEM_ALERT_WINDOW line.
        let injection = assemble(&["overlay", "camera", "overlay"]);
        let overlay_count = injection
            .manifest_lines
            .iter()
            .filter(|l| l.contains("SYSTEM_ALERT_WINDOW"))
            .count();
        assert_eq!(overlay_count, 1);
    }

    #[test]
    fn unknown_capability_is_reported_not_dropped() {
        let injection = assemble(&["overlay", "telepathy"]);
        assert_eq!(injection.unknown, vec!["telepathy".to_string()]);
    }

    #[test]
    fn typical_alarm_capability_set_excludes_sensitive() {
        let injection = assemble(&["overlay", "battery", "scanner", "sensors", "media_picker"]);
        assert!(injection.unknown.is_empty());
        assert!(
            injection.policy_warnings.is_empty(),
            "a non-sensitive capability set must not enable any policy-sensitive capability"
        );
        // Sanity: no accessibility/device-admin component leaked in.
        for comp in &injection.application_components {
            assert!(!comp.contains("SentinelAccessibilityService"));
            assert!(!comp.contains("SentinelAdminReceiver"));
        }
    }

    #[test]
    fn output_is_deterministic() {
        let a = assemble(&["camera", "overlay", "sensors"]);
        let b = assemble(&["sensors", "overlay", "camera"]);
        assert_eq!(a.manifest_lines, b.manifest_lines);
    }
}
