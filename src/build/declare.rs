//! Android declarations in `sentinel.toml`: the manifest and its XML resources, rendered from config.
//!
//! An app that declares anything here gets a generated `AndroidManifest.xml` (no `manifest = ...`
//! file) and the `res/xml/` files the declarations point to. Nothing is written in XML by the app.
//! `build_sentinel` adds the permissions what is declared needs, warns about what Google Play
//! reviews, and refuses combinations Android rejects.
//!
//! The tables are `[declare.*]`, not `[android.*]`: `[android]` already has flat keys (`activity`,
//! `manifest`, `icon`) that a table of the same name could not share.
//!
//! Every key of a component table that is not reserved becomes an `android:` attribute as written
//! (`exported = true`, `launchMode = "singleTask"`; an array is joined with `|`, so
//! `configChanges = ["orientation", "screenSize"]`). A key with a colon (`tools:node`) is written
//! as it is. Attribute names are only checked against the lists on developer.android.com to
//! catch typos (a warning); a new attribute works the day Android has it.
//!
//! ```toml
//! [declare]
//! permissions = ["INTERNET", "POST_NOTIFICATIONS"]        # bare names get android.permission.
//! [[declare.permission]]                                  # with attributes
//! name = "android.permission.WRITE_EXTERNAL_STORAGE"
//! maxSdkVersion = 28
//! [[declare.feature]]                                     # <uses-feature>
//! name = "android.hardware.camera"
//! required = false
//! [declare.application]                                   # attributes of <application>
//! allowBackup = false
//! largeHeap = true
//! [[declare.activity]]                                    # also service, receiver, provider, activity_alias
//! name = ".MainActivity"
//! launcher = true                                         # MAIN + LAUNCHER filter
//! native_lib = "ui"                                       # android.app.lib_name meta-data
//! launchMode = "singleTask"
//! [[declare.service]]
//! name = ".Sync"
//! exported = false
//! foregroundServiceType = "specialUse"
//! special_use = "Keeps the local relay running"           # the PROPERTY_SPECIAL_USE_FGS_SUBTYPE property
//! [[declare.service]]                                     # count: one service per process
//! name = ".G{i}"
//! process = ":g{i}"
//! count = 128
//! [[declare.receiver]]
//! name = ".Admin"
//! admin_policies = ["force-lock", "wipe-data"]            # res/xml generated; BIND_DEVICE_ADMIN set
//! lib = "ui"                                              # mobilesentinel.lib: the code is Rust
//! [[declare.receiver.intent_filter]]
//! actions = ["android.app.action.PROFILE_PROVISIONING_COMPLETE"]
//! [declare.boot_start]                                    # preset: a boot receiver
//! receiver = ".Boot"
//! ```
//!
//! Elements: `permission`, `feature`, `define_permission`, `permission_group`, `permission_tree`,
//! `application` (`meta_data`, `property`), `uses_library`, `uses_native_library`, `activity`,
//! `activity_alias`, `service`, `receiver`, `provider`, `queries`, `network_security`,
//! `resource_xml` (any `res/xml` file as a tree), `element` (any other manifest element as a tree).
//! Component tables take `intent_filter`, `meta_data`, `property` (arrays of tables), `layout`,
//! `grant_uri_permission`, `path_permission`, `count`, `lib`, `native_lib`, `launcher`,
//! `special_use`, `admin_policies`, `admin_resource`, `accessibility`, `file_paths`.
//! Presets: `work_profile`, `boot_start`, `process_pool`.

use std::collections::{BTreeMap, BTreeSet};
use toml::{Table, Value};

type R<T> = Result<T, String>;

const ANDROID_NS: &str = "http://schemas.android.com/apk/res/android";
const TOOLS_NS: &str = "http://schemas.android.com/tools";

/// The declaration keys of `[declare]`; an app with none of them keeps its own manifest or the
/// generated native one.
const KEYS: &[&str] = &[
    "permissions",
    "permission",
    "feature",
    "define_permission",
    "permission_group",
    "permission_tree",
    "application",
    "uses_library",
    "uses_native_library",
    "activity",
    "activity_alias",
    "service",
    "receiver",
    "provider",
    "queries",
    "network_security",
    "resource_xml",
    "element",
    "work_profile",
    "boot_start",
    "process_pool",
];

/// Every foreground service type and the permission it needs besides FOREGROUND_SERVICE.
const FGS_TYPES: &[(&str, Option<&str>)] = &[
    ("camera", Some("FOREGROUND_SERVICE_CAMERA")),
    (
        "connectedDevice",
        Some("FOREGROUND_SERVICE_CONNECTED_DEVICE"),
    ),
    ("dataSync", Some("FOREGROUND_SERVICE_DATA_SYNC")),
    ("health", Some("FOREGROUND_SERVICE_HEALTH")),
    ("location", Some("FOREGROUND_SERVICE_LOCATION")),
    ("mediaPlayback", Some("FOREGROUND_SERVICE_MEDIA_PLAYBACK")),
    (
        "mediaProcessing",
        Some("FOREGROUND_SERVICE_MEDIA_PROCESSING"),
    ),
    (
        "mediaProjection",
        Some("FOREGROUND_SERVICE_MEDIA_PROJECTION"),
    ),
    ("microphone", Some("FOREGROUND_SERVICE_MICROPHONE")),
    ("phoneCall", Some("FOREGROUND_SERVICE_PHONE_CALL")),
    (
        "remoteMessaging",
        Some("FOREGROUND_SERVICE_REMOTE_MESSAGING"),
    ),
    ("shortService", None),
    ("specialUse", Some("FOREGROUND_SERVICE_SPECIAL_USE")),
    ("systemExempted", Some("FOREGROUND_SERVICE_SYSTEM_EXEMPTED")),
];

/// Device admin policies (`<uses-policies>` children).
pub const ADMIN_POLICIES: &[&str] = &[
    "limit-password",
    "watch-login",
    "reset-password",
    "force-lock",
    "wipe-data",
    "expire-password",
    "encrypted-storage",
    "disable-camera",
    "disable-keyguard-features",
];

const FILE_PATH_KINDS: &[&str] = &[
    "root-path",
    "files-path",
    "cache-path",
    "external-path",
    "external-files-path",
    "external-cache-path",
    "external-media-path",
];

const LAUNCH_MODES: &[&str] = &[
    "standard",
    "singleTop",
    "singleTask",
    "singleInstance",
    "singleInstancePerTask",
];

const ORIENTATIONS: &[&str] = &[
    "unspecified",
    "behind",
    "landscape",
    "portrait",
    "reverseLandscape",
    "reversePortrait",
    "sensorLandscape",
    "sensorPortrait",
    "userLandscape",
    "userPortrait",
    "sensor",
    "fullSensor",
    "nosensor",
    "user",
    "fullUser",
    "locked",
];

/// Permissions Google Play reviews or restricts (or that a third-party app is never granted).
const SENSITIVE_PERMISSIONS: &[(&str, &str)] = &[
    ("QUERY_ALL_PACKAGES", "Play allows it only for apps whose core purpose needs the full app list (declaration form)"),
    ("READ_SMS", "SMS permissions are for the default SMS handler only (Play permissions declaration)"),
    ("SEND_SMS", "SMS permissions are for the default SMS handler only (Play permissions declaration)"),
    ("RECEIVE_SMS", "SMS permissions are for the default SMS handler only (Play permissions declaration)"),
    ("RECEIVE_MMS", "SMS permissions are for the default SMS handler only (Play permissions declaration)"),
    ("READ_CALL_LOG", "call log permissions are for the default phone handler only (Play permissions declaration)"),
    ("WRITE_CALL_LOG", "call log permissions are for the default phone handler only (Play permissions declaration)"),
    ("PROCESS_OUTGOING_CALLS", "call log permissions are for the default phone handler only (Play permissions declaration)"),
    ("MANAGE_EXTERNAL_STORAGE", "all-files access needs a Play declaration and a core-purpose reason"),
    ("REQUEST_INSTALL_PACKAGES", "Play restricts apps that install packages"),
    ("REQUEST_DELETE_PACKAGES", "Play restricts apps that delete packages"),
    ("USE_FULL_SCREEN_INTENT", "Play limits full-screen intents to calling and alarm apps"),
    ("SCHEDULE_EXACT_ALARM", "exact alarms are for alarm and calendar apps; Play may ask for a reason"),
    ("USE_EXACT_ALARM", "Play limits it to alarm clock and calendar apps"),
    ("ACCESS_BACKGROUND_LOCATION", "background location needs a Play declaration and a video"),
    ("PACKAGE_USAGE_STATS", "usage access is special app access; Play reviews it"),
    ("SYSTEM_ALERT_WINDOW", "draw-over-apps: Play reviews it"),
    ("BIND_VPN_SERVICE", "VPN apps need the Play VpnService declaration"),
    ("MODIFY_QUIET_MODE", "granted to system or privileged apps only; declaring it is harmless, calling it fails otherwise"),
];

/// What a component's `permission` attribute says about the app.
const SENSITIVE_COMPONENT_PERMISSIONS: &[(&str, &str)] = &[
    ("android.permission.BIND_DEVICE_ADMIN", "device admin: Play reviews apps that use the device admin API"),
    ("android.permission.BIND_ACCESSIBILITY_SERVICE", "accessibility service: Play requires a declaration and that it helps users with disabilities"),
    ("android.permission.BIND_VPN_SERVICE", "VPN service: Play requires the VpnService declaration"),
    ("android.permission.BIND_NOTIFICATION_LISTENER_SERVICE", "notification listener: Play reviews access to notifications"),
];

const ACTIVITY_ATTRS: &[&str] = &[
    "allowEmbedded",
    "allowTaskReparenting",
    "alwaysRetainTaskState",
    "autoRemoveFromRecents",
    "banner",
    "canDisplayOnRemoteDevices",
    "clearTaskOnLaunch",
    "colorMode",
    "configChanges",
    "description",
    "directBootAware",
    "documentLaunchMode",
    "enableOnBackInvokedCallback",
    "enabled",
    "excludeFromRecents",
    "exported",
    "finishOnTaskLaunch",
    "forceQueryable",
    "hardwareAccelerated",
    "icon",
    "immersive",
    "inheritShowWhenLocked",
    "intentMatchingFlags",
    "knownActivityEmbeddingCerts",
    "label",
    "launchMode",
    "lockTaskMode",
    "maxAspectRatio",
    "maxRecents",
    "minAspectRatio",
    "multiprocess",
    "name",
    "noHistory",
    "parentActivityName",
    "permission",
    "persistableMode",
    "preferMinimalPostProcessing",
    "process",
    "recreateOnConfigChanges",
    "relinquishTaskIdentity",
    "requireContentUriPermissionFromCaller",
    "resizeableActivity",
    "roundIcon",
    "screenOrientation",
    "showForAllUsers",
    "showWhenLocked",
    "splitName",
    "stateNotNeeded",
    "supportsPictureInPicture",
    "taskAffinity",
    "theme",
    "turnScreenOn",
    "uiOptions",
    "visibleToInstantApps",
    "windowLayoutInDisplayCutoutMode",
    "windowSoftInputMode",
    "allowUntrustedActivityEmbedding",
    "attributionTags",
];
const SERVICE_ATTRS: &[&str] = &[
    "description",
    "directBootAware",
    "enabled",
    "exported",
    "foregroundServiceType",
    "icon",
    "intentMatchingFlags",
    "isolatedProcess",
    "label",
    "name",
    "permission",
    "process",
    "roundIcon",
    "stopWithTask",
    "useAppZygote",
    "visibleToInstantApps",
    "singleUser",
    "externalService",
];
const RECEIVER_ATTRS: &[&str] = &[
    "directBootAware",
    "enabled",
    "exported",
    "icon",
    "intentMatchingFlags",
    "label",
    "name",
    "permission",
    "process",
    "roundIcon",
    "visibleToInstantApps",
];
const PROVIDER_ATTRS: &[&str] = &[
    "authorities",
    "directBootAware",
    "enabled",
    "exported",
    "grantUriPermissions",
    "icon",
    "initOrder",
    "intentMatchingFlags",
    "label",
    "multiprocess",
    "name",
    "permission",
    "process",
    "readPermission",
    "roundIcon",
    "syncable",
    "writePermission",
    "splitName",
    "forceUriPermissions",
];
const ALIAS_ATTRS: &[&str] = &[
    "enabled",
    "exported",
    "icon",
    "intentMatchingFlags",
    "label",
    "name",
    "permission",
    "roundIcon",
    "targetActivity",
];
const APPLICATION_ATTRS: &[&str] = &[
    "allowTaskReparenting",
    "allowBackup",
    "allowClearUserData",
    "allowCrossUidActivitySwitchFromBelow",
    "allowNativeHeapPointerTagging",
    "appCategory",
    "backupAgent",
    "backupInForeground",
    "banner",
    "dataExtractionRules",
    "debuggable",
    "description",
    "enabled",
    "enableOnBackInvokedCallback",
    "extractNativeLibs",
    "forceQueryable",
    "fullBackupContent",
    "fullBackupOnly",
    "gwpAsanMode",
    "hasCode",
    "hasFragileUserData",
    "hardwareAccelerated",
    "icon",
    "intentMatchingFlags",
    "isGame",
    "isMonitoringTool",
    "killAfterRestore",
    "largeHeap",
    "label",
    "localeConfig",
    "logo",
    "manageSpaceActivity",
    "name",
    "networkSecurityConfig",
    "pageSizeCompat",
    "permission",
    "persistent",
    "process",
    "restoreAnyVersion",
    "requestLegacyExternalStorage",
    "requiredAccountType",
    "resizeableActivity",
    "restrictedAccountType",
    "roundIcon",
    "supportsRtl",
    "taskAffinity",
    "testOnly",
    "theme",
    "uiOptions",
    "usesCleartextTraffic",
    "vmSafeMode",
    "zygotePreloadName",
];
const FILTER_ATTRS: &[&str] = &[
    "autoVerify",
    "icon",
    "label",
    "priority",
    "order",
    "roundIcon",
    "directBootAware",
];
const DATA_ATTRS: &[&str] = &[
    "scheme",
    "host",
    "port",
    "path",
    "pathPattern",
    "pathPrefix",
    "pathSuffix",
    "pathAdvancedPattern",
    "mimeType",
    "fragment",
    "fragmentPattern",
    "fragmentPrefix",
    "fragmentSuffix",
    "fragmentAdvancedPattern",
    "query",
    "queryPattern",
    "queryPrefix",
    "querySuffix",
    "queryAdvancedPattern",
    "mimeGroup",
];

/// The parsed `[declare]` declaration tables of a `sentinel.toml`.
#[derive(Debug, Clone, PartialEq)]
pub struct Declarations {
    android: Table,
}

/// What the generated manifest takes from the rest of `[android]`.
#[derive(Debug, Clone, Default)]
pub struct Context {
    /// Default `android:label` of the application.
    pub label: String,
    /// The launcher icon was staged (`icon` key): `@mipmap/ic_launcher`.
    pub icon: bool,
    /// A NativeActivity app: the standard launcher activity of this library is added.
    pub native_lib: Option<String>,
    /// Permissions with a `maxSdkVersion` below this stay out.
    pub min_sdk: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Rendered {
    pub manifest: String,
    /// (path under `res/`, content), e.g. `xml/device_admin_admin.xml`.
    pub resources: BTreeMap<String, String>,
    /// Play-sensitive items; build_sentinel prints them.
    pub warnings: Vec<String>,
    /// What the build added or derived by itself.
    pub notes: Vec<String>,
}

/// Reads the declarations of a `sentinel.toml`. `None` when it declares none.
pub fn parse(text: &str) -> R<Option<Declarations>> {
    let table: Table = match text.parse() {
        Ok(t) => t,
        Err(e) => {
            // The flat reader of the other keys is more forgiving than TOML; only a file that
            // tries to declare something has to be valid.
            let tries = KEYS.iter().any(|k| text.contains(&format!("declare.{k}")))
                || text.contains("permissions");
            return if tries {
                Err(format!("sentinel.toml: {e}"))
            } else {
                Ok(None)
            };
        }
    };
    let Some(Value::Table(android)) = table.get("declare") else {
        return Ok(None);
    };
    if KEYS.iter().any(|k| android.contains_key(*k)) {
        Ok(Some(Declarations {
            android: android.clone(),
        }))
    } else {
        Ok(None)
    }
}

// ---------------------------------------------------------------------------------------
// XML tree

#[derive(Debug, Clone, PartialEq, Eq)]
struct Node {
    tag: String,
    attrs: Vec<(String, String)>,
    children: Vec<Node>,
    text: Option<String>,
}

impl Node {
    fn new(tag: &str) -> Self {
        Node {
            tag: tag.into(),
            attrs: vec![],
            children: vec![],
            text: None,
        }
    }
    fn with_text(mut self, text: &str) -> Self {
        self.text = Some(text.into());
        self
    }
    /// An `android:` attribute (unless the key has a namespace already).
    fn attr(mut self, key: &str, value: impl Into<String>) -> Self {
        self.attrs.push((qualify(key), value.into()));
        self
    }
    fn literal(mut self, key: &str, value: impl Into<String>) -> Self {
        self.attrs.push((key.into(), value.into()));
        self
    }
    fn with(mut self, child: Node) -> Self {
        self.children.push(child);
        self
    }
    fn write(&self, out: &mut String, depth: usize) {
        let pad = "    ".repeat(depth);
        out.push_str(&format!("{pad}<{}", self.tag));
        if self.attrs.len() <= 2 {
            for (k, v) in &self.attrs {
                out.push_str(&format!(" {k}=\"{}\"", escape(v)));
            }
        } else {
            for (k, v) in &self.attrs {
                out.push_str(&format!("\n{pad}    {k}=\"{}\"", escape(v)));
            }
        }
        if let Some(text) = &self.text {
            out.push_str(&format!(">{}</{}>\n", escape(text), self.tag));
        } else if self.children.is_empty() {
            out.push_str(" />\n");
        } else {
            out.push_str(">\n");
            for c in &self.children {
                c.write(out, depth + 1);
            }
            out.push_str(&format!("{pad}</{}>\n", self.tag));
        }
    }
}

fn qualify(key: &str) -> String {
    if key.contains(':') {
        key.to_string()
    } else {
        format!("android:{key}")
    }
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\n', "&#10;")
}

const XML_HEADER: &str = "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n";

fn xml_file(root: &Node) -> String {
    let mut out = XML_HEADER.to_string();
    root.write(&mut out, 0);
    out
}

// ---------------------------------------------------------------------------------------
// TOML helpers

fn scalar(v: &Value, key: &str, join: &str) -> R<String> {
    Ok(match v {
        Value::String(s) => s.clone(),
        Value::Boolean(b) => b.to_string(),
        Value::Integer(i) => i.to_string(),
        Value::Float(f) => f.to_string(),
        Value::Array(items) => items
            .iter()
            .map(|i| scalar(i, key, join))
            .collect::<R<Vec<_>>>()?
            .join(join),
        other => {
            return Err(format!(
                "{key}: a {} is not an attribute value",
                other.type_str()
            ))
        }
    })
}

fn get_str<'a>(t: &'a Table, key: &str) -> R<Option<&'a str>> {
    match t.get(key) {
        None => Ok(None),
        Some(Value::String(s)) => Ok(Some(s)),
        Some(_) => Err(format!("{key} must be a string")),
    }
}

fn need_str<'a>(t: &'a Table, key: &str, what: &str) -> R<&'a str> {
    get_str(t, key)?.ok_or_else(|| format!("{what} needs {key}"))
}

fn get_bool(t: &Table, key: &str) -> R<Option<bool>> {
    match t.get(key) {
        None => Ok(None),
        Some(Value::Boolean(b)) => Ok(Some(*b)),
        Some(_) => Err(format!("{key} must be true or false")),
    }
}

fn str_list(t: &Table, key: &str, single: Option<&str>) -> R<Vec<String>> {
    let mut out = vec![];
    if let Some(one) = single {
        if let Some(s) = get_str(t, one)? {
            out.push(s.to_string());
        }
    }
    match t.get(key) {
        None => {}
        Some(Value::Array(items)) => {
            for i in items {
                match i {
                    Value::String(s) => out.push(s.clone()),
                    _ => return Err(format!("{key} must be a list of strings")),
                }
            }
        }
        Some(_) => return Err(format!("{key} must be a list of strings")),
    }
    Ok(out)
}

/// A key that holds one table or an array of tables.
fn tables<'a>(t: &'a Table, key: &str) -> R<Vec<&'a Table>> {
    match t.get(key) {
        None => Ok(vec![]),
        Some(Value::Table(one)) => Ok(vec![one]),
        Some(Value::Array(items)) => items
            .iter()
            .map(|i| match i {
                Value::Table(t) => Ok(t),
                _ => Err(format!("{key} must be tables")),
            })
            .collect(),
        Some(_) => Err(format!("{key} must be a table or a list of tables")),
    }
}

fn has_placeholder(v: &Value) -> bool {
    match v {
        Value::String(s) => s.contains("{i}"),
        Value::Array(a) => a.iter().any(has_placeholder),
        Value::Table(t) => t.values().any(has_placeholder),
        _ => false,
    }
}

fn substitute(v: &Value, i: i64) -> Value {
    match v {
        Value::String(s) => Value::String(s.replace("{i}", &i.to_string())),
        Value::Array(a) => Value::Array(a.iter().map(|x| substitute(x, i)).collect()),
        Value::Table(t) => Value::Table(
            t.iter()
                .map(|(k, x)| (k.clone(), substitute(x, i)))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// `count = N` repeats the table N times with `{i}` (0 to N-1) replaced in every string.
fn expand(t: &Table, what: &str) -> R<Vec<Table>> {
    let placeholder = t.iter().any(|(k, v)| k != "count" && has_placeholder(v));
    match t.get("count") {
        None if placeholder => Err(format!("{what}: {{i}} needs count (how many copies)")),
        None => Ok(vec![t.clone()]),
        Some(Value::Integer(n)) if *n >= 1 => {
            if !placeholder {
                return Err(format!(
                    "{what}: count = {n} without {{i}} in any value would declare the same thing {n} times"
                ));
            }
            let mut base = t.clone();
            base.remove("count");
            Ok((0..*n)
                .map(|i| match substitute(&Value::Table(base.clone()), i) {
                    Value::Table(t) => t,
                    _ => unreachable!(),
                })
                .collect())
        }
        Some(_) => Err(format!("{what}: count must be a whole number of 1 or more")),
    }
}

fn snake(name: &str) -> String {
    let last = name.rsplit('.').next().unwrap_or(name);
    last.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect()
}

fn permission_name(s: &str) -> String {
    if s.contains('.') {
        s.to_string()
    } else {
        format!("android.permission.{s}")
    }
}

fn text_table(pairs: &[(&str, Value)]) -> Table {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect()
}

fn s(v: &str) -> Value {
    Value::String(v.to_string())
}

fn strs(v: &[&str]) -> Value {
    Value::Array(v.iter().map(|x| s(x)).collect())
}

// ---------------------------------------------------------------------------------------
// Rendering

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Activity,
    Alias,
    Service,
    Receiver,
    Provider,
}

impl Kind {
    fn tag(self) -> &'static str {
        match self {
            Kind::Activity => "activity",
            Kind::Alias => "activity-alias",
            Kind::Service => "service",
            Kind::Receiver => "receiver",
            Kind::Provider => "provider",
        }
    }
    fn key(self) -> &'static str {
        match self {
            Kind::Alias => "activity_alias",
            other => other.tag(),
        }
    }
    fn known(self) -> &'static [&'static str] {
        match self {
            Kind::Activity => ACTIVITY_ATTRS,
            Kind::Alias => ALIAS_ATTRS,
            Kind::Service => SERVICE_ATTRS,
            Kind::Receiver => RECEIVER_ATTRS,
            Kind::Provider => PROVIDER_ATTRS,
        }
    }
}

const RESERVED: &[&str] = &[
    "name",
    "count",
    "meta_data",
    "property",
    "intent_filter",
    "layout",
    "grant_uri_permission",
    "path_permission",
    "lib",
    "native_lib",
    "launcher",
    "special_use",
    "admin_policies",
    "admin_resource",
    "accessibility",
    "file_paths",
];

#[derive(Default)]
struct Builder {
    /// Declared `<uses-permission>`: name -> attributes.
    permissions: Vec<(String, Vec<(String, String)>)>,
    auto_permissions: BTreeMap<String, String>,
    top: Vec<Node>,
    features: Vec<Node>,
    queries: Option<Node>,
    manifest_elements: Vec<Node>,
    application_elements: Vec<Node>,
    components: BTreeMap<&'static str, Vec<Node>>,
    names: BTreeSet<String>,
    resources: BTreeMap<String, String>,
    warnings: Vec<String>,
    notes: Vec<String>,
}

impl Builder {
    fn warn(&mut self, text: String) {
        if !self.warnings.contains(&text) {
            self.warnings.push(text);
        }
    }

    fn resource(&mut self, path: String, content: String) -> R<()> {
        match self.resources.get(&path) {
            Some(old) if *old != content => Err(format!(
                "res/{path} is generated twice with different content; name them apart (admin_resource and the like)"
            )),
            _ => {
                self.resources.insert(path, content);
                Ok(())
            }
        }
    }

    fn permission(&mut self, name: &str, attrs: Vec<(String, String)>) -> R<()> {
        let name = permission_name(name);
        if let Some((_, old)) = self.permissions.iter().find(|(n, _)| *n == name) {
            if *old != attrs {
                return Err(format!(
                    "{name} is declared twice with different attributes"
                ));
            }
            return Ok(());
        }
        let short = name.rsplit('.').next().unwrap_or(&name).to_string();
        if let Some((_, why)) = SENSITIVE_PERMISSIONS.iter().find(|(p, _)| *p == short) {
            self.warn(format!("{name}: {why}"));
        }
        self.permissions.push((name, attrs));
        Ok(())
    }

    fn auto(&mut self, name: &str, why: String) {
        self.auto_permissions.entry(name.to_string()).or_insert(why);
    }

    fn meta(&self, tag: &str, t: &Table, owner: &str) -> R<Node> {
        let name = need_str(t, "name", &format!("{tag} of {owner}"))?;
        let value = t.get("value");
        let resource = get_str(t, "resource")?;
        let node = Node::new(tag).attr("name", name);
        match (value, resource) {
            (Some(_), Some(_)) => Err(format!(
                "{tag} {name} of {owner}: value and resource together; pick one"
            )),
            (None, None) => Err(format!("{tag} {name} of {owner}: needs value or resource")),
            (Some(v), None) => Ok(node.attr("value", scalar(v, "value", ",")?)),
            (None, Some(r)) => Ok(node.attr("resource", r)),
        }
    }

    fn attributes(
        &mut self,
        t: &Table,
        known: &[&str],
        tag: &str,
        reserved: &[&str],
        owner: &str,
    ) -> R<BTreeMap<String, String>> {
        let mut out = BTreeMap::new();
        for (k, v) in t {
            if reserved.contains(&k.as_str()) {
                continue;
            }
            if !k.contains(':') && !known.contains(&k.as_str()) {
                let near = known
                    .iter()
                    .find(|n| n.eq_ignore_ascii_case(k))
                    .map(|n| format!(" (did you mean {n}?)"))
                    .unwrap_or_default();
                self.warn(format!(
                    "{owner}: {k} is not a known attribute of <{tag}>{near}; written as android:{k}"
                ));
            }
            let join = if k == "authorities" { ";" } else { "|" };
            out.insert(k.clone(), scalar(v, &format!("{owner} {k}"), join)?);
        }
        Ok(out)
    }

    /// `<intent-filter>` (or `<intent>` of `queries` without attributes) and its actions.
    fn filter(
        &self,
        t: &Table,
        tag: &str,
        owner: &str,
        with_attrs: bool,
    ) -> R<(Node, Vec<String>)> {
        let actions = str_list(t, "actions", Some("action"))?;
        let categories = str_list(t, "categories", Some("category"))?;
        if actions.is_empty() {
            return Err(format!(
                "{owner}: an {tag} without an action matches nothing"
            ));
        }
        let mut node = Node::new(tag);
        for (k, v) in t {
            if matches!(
                k.as_str(),
                "actions" | "action" | "categories" | "category" | "data"
            ) {
                continue;
            }
            if !with_attrs {
                return Err(format!("{owner}: {k} is not allowed in a <{tag}>"));
            }
            if !FILTER_ATTRS.contains(&k.as_str()) && !k.contains(':') {
                return Err(format!(
                    "{owner}: {k} is not an attribute of <intent-filter> (actions, categories, data, {})",
                    FILTER_ATTRS.join(", ")
                ));
            }
            node = node.attr(k, scalar(v, k, "|")?);
        }
        for a in &actions {
            node = node.with(Node::new("action").attr("name", a.as_str()));
        }
        for c in &categories {
            node = node.with(Node::new("category").attr("name", c.as_str()));
        }
        let (mut schemes, mut hosts, mut ports_or_paths) = (false, false, false);
        for d in tables(t, "data")? {
            let mut n = Node::new("data");
            for k in d.keys() {
                if !DATA_ATTRS.contains(&k.as_str()) {
                    return Err(format!("{owner}: {k} is not an attribute of <data>"));
                }
                match k.as_str() {
                    "scheme" => schemes = true,
                    "host" => hosts = true,
                    "port"
                    | "path"
                    | "pathPattern"
                    | "pathPrefix"
                    | "pathSuffix"
                    | "pathAdvancedPattern" => ports_or_paths = true,
                    _ => {}
                }
            }
            for k in DATA_ATTRS {
                if let Some(v) = d.get(*k) {
                    n = n.attr(k, scalar(v, k, "")?);
                }
            }
            node = node.with(n);
        }
        if hosts && !schemes {
            return Err(format!(
                "{owner}: <data> has a host but no scheme; Android ignores it"
            ));
        }
        if ports_or_paths && !hosts {
            return Err(format!(
                "{owner}: <data> has a port or path but no host; Android ignores it"
            ));
        }
        Ok((node, actions))
    }

    fn component(&mut self, kind: Kind, t: &Table) -> R<()> {
        let tag = kind.tag();
        let name = need_str(t, "name", &format!("every <{tag}>"))?.to_string();
        let owner = format!("<{tag}> {name}");
        if !self.names.insert(name.clone()) {
            return Err(format!("{owner} is declared twice"));
        }
        let mut attrs = self.attributes(t, kind.known(), tag, RESERVED, &owner)?;
        let mut filters: Vec<Node> = vec![];
        let mut actions: BTreeSet<String> = BTreeSet::new();
        let mut metas: Vec<Node> = vec![];
        let mut properties: Vec<Node> = vec![];
        let short = snake(&name);

        for f in tables(t, "intent_filter")? {
            let (node, acts) = self.filter(f, "intent-filter", &owner, true)?;
            actions.extend(acts);
            filters.push(node);
        }
        if get_bool(t, "launcher")? == Some(true) {
            if !matches!(kind, Kind::Activity | Kind::Alias) {
                return Err(format!("{owner}: launcher is for activities"));
            }
            let mut f = Table::new();
            f.insert("actions".into(), strs(&["android.intent.action.MAIN"]));
            f.insert(
                "categories".into(),
                strs(&["android.intent.category.LAUNCHER"]),
            );
            let (node, acts) = self.filter(&f, "intent-filter", &owner, true)?;
            actions.extend(acts);
            filters.push(node);
            attrs.entry("exported".into()).or_insert("true".into());
        }

        if let Some(v) = t.get("admin_policies") {
            if kind != Kind::Receiver {
                return Err(format!("{owner}: admin_policies is for receivers"));
            }
            let Value::Array(list) = v else {
                return Err(format!("{owner}: admin_policies must be a list"));
            };
            let mut policies = BTreeSet::new();
            for p in list {
                let Value::String(p) = p else {
                    return Err(format!("{owner}: admin_policies must be strings"));
                };
                if p == "all" {
                    policies.extend(ADMIN_POLICIES.iter().map(|x| x.to_string()));
                } else if ADMIN_POLICIES.contains(&p.as_str()) {
                    policies.insert(p.clone());
                } else {
                    return Err(format!(
                        "{owner}: unknown device admin policy \"{p}\"; one of {} or \"all\"",
                        ADMIN_POLICIES.join(", ")
                    ));
                }
            }
            let mut uses = Node::new("uses-policies");
            for p in ADMIN_POLICIES.iter().filter(|p| policies.contains(**p)) {
                uses = uses.with(Node::new(p));
            }
            let res = get_str(t, "admin_resource")?
                .map(str::to_string)
                .unwrap_or_else(|| format!("device_admin_{short}"));
            self.resource(
                format!("xml/{res}.xml"),
                xml_file(
                    &Node::new("device-admin")
                        .literal("xmlns:android", ANDROID_NS)
                        .with(uses),
                ),
            )?;
            metas.push(
                Node::new("meta-data")
                    .attr("name", "android.app.device_admin")
                    .attr("resource", format!("@xml/{res}")),
            );
            attrs
                .entry("permission".into())
                .or_insert("android.permission.BIND_DEVICE_ADMIN".into());
            attrs.entry("exported".into()).or_insert("true".into());
            if !actions.contains("android.app.action.DEVICE_ADMIN_ENABLED") {
                actions.insert("android.app.action.DEVICE_ADMIN_ENABLED".into());
                let mut f = Table::new();
                f.insert(
                    "actions".into(),
                    strs(&["android.app.action.DEVICE_ADMIN_ENABLED"]),
                );
                filters.push(self.filter(&f, "intent-filter", &owner, true)?.0);
            }
        }

        if let Some(v) = t.get("accessibility") {
            if kind != Kind::Service {
                return Err(format!("{owner}: accessibility is for services"));
            }
            let Value::Table(cfg) = v else {
                return Err(format!(
                    "{owner}: accessibility must be a table of attributes"
                ));
            };
            let mut root = Node::new("accessibility-service").literal("xmlns:android", ANDROID_NS);
            for (k, val) in cfg {
                root = root.attr(k, scalar(val, &format!("{owner} accessibility {k}"), "|")?);
            }
            let res = format!("accessibility_{short}");
            self.resource(format!("xml/{res}.xml"), xml_file(&root))?;
            metas.push(
                Node::new("meta-data")
                    .attr("name", "android.accessibilityservice")
                    .attr("resource", format!("@xml/{res}")),
            );
            attrs
                .entry("permission".into())
                .or_insert("android.permission.BIND_ACCESSIBILITY_SERVICE".into());
            attrs.entry("exported".into()).or_insert("true".into());
            if !actions.contains("android.accessibilityservice.AccessibilityService") {
                actions.insert("android.accessibilityservice.AccessibilityService".into());
                let mut f = Table::new();
                f.insert(
                    "actions".into(),
                    strs(&["android.accessibilityservice.AccessibilityService"]),
                );
                filters.push(self.filter(&f, "intent-filter", &owner, true)?.0);
            }
        }

        if let Some(v) = t.get("file_paths") {
            if kind != Kind::Provider {
                return Err(format!("{owner}: file_paths is for providers"));
            }
            let Value::Array(list) = v else {
                return Err(format!("{owner}: file_paths must be a list of tables"));
            };
            let mut root = Node::new("paths");
            for item in list {
                let Value::Table(p) = item else {
                    return Err(format!("{owner}: file_paths must be a list of tables"));
                };
                let kind_name = need_str(p, "kind", &format!("{owner} file_paths entry"))?;
                if !FILE_PATH_KINDS.contains(&kind_name) {
                    return Err(format!(
                        "{owner}: file_paths kind \"{kind_name}\" is not one of {}",
                        FILE_PATH_KINDS.join(", ")
                    ));
                }
                root = root.with(
                    Node::new(kind_name)
                        .literal(
                            "name",
                            need_str(p, "name", &format!("{owner} file_paths entry"))?,
                        )
                        .literal("path", get_str(p, "path")?.unwrap_or(".")),
                );
            }
            let res = format!("file_paths_{short}");
            self.resource(format!("xml/{res}.xml"), xml_file(&root))?;
            metas.push(
                Node::new("meta-data")
                    .attr("name", "android.support.FILE_PROVIDER_PATHS")
                    .attr("resource", format!("@xml/{res}")),
            );
        }

        let mut meta_names: BTreeSet<String> = BTreeSet::new();
        for m in tables(t, "meta_data")? {
            let node = self.meta("meta-data", m, &owner)?;
            meta_names.insert(node.attrs[0].1.clone());
            metas.push(node);
        }
        for (key, meta_name) in [
            ("native_lib", "android.app.lib_name"),
            ("lib", "mobilesentinel.lib"),
        ] {
            if let Some(lib) = get_str(t, key)? {
                if key == "native_lib" && kind != Kind::Activity {
                    return Err(format!("{owner}: native_lib is for activities"));
                }
                if key == "lib" && !super::shims::valid_lib(lib) {
                    return Err(format!("{owner}: lib \"{lib}\": letters and digits only"));
                }
                if !meta_names.contains(meta_name) {
                    metas.push(
                        Node::new("meta-data")
                            .attr("name", meta_name)
                            .attr("value", lib),
                    );
                }
            }
        }
        for p in tables(t, "property")? {
            properties.push(self.meta("property", p, &owner)?);
        }

        // Foreground service types.
        let fgs = attrs.get("foregroundServiceType").cloned();
        if let Some(types) = &fgs {
            if kind != Kind::Service {
                return Err(format!("{owner}: foregroundServiceType is for services"));
            }
            self.auto(
                "android.permission.FOREGROUND_SERVICE",
                format!("{owner} is a foreground service"),
            );
            for ty in types.split('|') {
                let Some((_, perm)) = FGS_TYPES.iter().find(|(n, _)| *n == ty) else {
                    return Err(format!(
                        "{owner}: foregroundServiceType \"{ty}\" is not one of {}",
                        FGS_TYPES
                            .iter()
                            .map(|(n, _)| *n)
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                };
                if let Some(p) = perm {
                    self.auto(
                        &format!("android.permission.{p}"),
                        format!("{owner} has type {ty}"),
                    );
                }
                if ty == "specialUse" {
                    self.warn(format!(
                        "{owner}: specialUse foreground service: Play reviews the subtype text and the Play Console declaration"
                    ));
                }
            }
        }
        let special = get_str(t, "special_use")?;
        let has_subtype_property = properties
            .iter()
            .any(|p| p.attrs[0].1 == "android.app.PROPERTY_SPECIAL_USE_FGS_SUBTYPE");
        let is_special = fgs
            .as_ref()
            .is_some_and(|f| f.split('|').any(|x| x == "specialUse"));
        if let Some(text) = special {
            if !is_special {
                return Err(format!(
                    "{owner}: special_use is the subtype of a specialUse foreground service; add foregroundServiceType = \"specialUse\""
                ));
            }
            properties.push(
                Node::new("property")
                    .attr("name", "android.app.PROPERTY_SPECIAL_USE_FGS_SUBTYPE")
                    .attr("value", text),
            );
        } else if is_special && !has_subtype_property {
            return Err(format!(
                "{owner}: a specialUse foreground service must say what it is for: special_use = \"...\""
            ));
        }

        // Value checks.
        if let Some(p) = attrs.get("process") {
            let ok = p.strip_prefix(':').is_some_and(|r| !r.is_empty())
                || p.chars().next().is_some_and(|c| c.is_ascii_lowercase());
            if !ok {
                return Err(format!(
                    "{owner}: process \"{p}\": use \":name\" for a private process or a lowercase name like com.example.shared"
                ));
            }
        }
        if let Some(m) = attrs.get("launchMode") {
            if kind != Kind::Activity || !LAUNCH_MODES.contains(&m.as_str()) {
                return Err(format!(
                    "{owner}: launchMode \"{m}\" must be one of {} (activities only)",
                    LAUNCH_MODES.join(", ")
                ));
            }
        }
        if let Some(o) = attrs.get("screenOrientation") {
            if !ORIENTATIONS.contains(&o.as_str()) {
                return Err(format!(
                    "{owner}: screenOrientation \"{o}\" must be one of {}",
                    ORIENTATIONS.join(", ")
                ));
            }
        }
        if kind == Kind::Alias && !attrs.contains_key("targetActivity") {
            return Err(format!("{owner}: an activity alias needs targetActivity"));
        }
        if kind == Kind::Provider && !attrs.contains_key("authorities") {
            return Err(format!("{owner}: a provider needs authorities"));
        }
        if !filters.is_empty() && !attrs.contains_key("exported") {
            return Err(format!(
                "{owner}: has an intent filter, so Android 12+ needs exported = true or false"
            ));
        }
        if let Some(perm) = attrs.get("permission") {
            if let Some((_, why)) = SENSITIVE_COMPONENT_PERMISSIONS
                .iter()
                .find(|(p, _)| p == perm)
            {
                self.warn(format!("{owner}: {why}"));
            }
        }

        // Permissions the declared behaviour needs.
        for a in &actions {
            let (perm, what) = match a.as_str() {
                "android.intent.action.BOOT_COMPLETED"
                | "android.intent.action.LOCKED_BOOT_COMPLETED" => {
                    ("RECEIVE_BOOT_COMPLETED", "listens for boot")
                }
                "android.provider.Telephony.SMS_RECEIVED" => ("RECEIVE_SMS", "listens for SMS"),
                "android.intent.action.NEW_OUTGOING_CALL" => {
                    ("PROCESS_OUTGOING_CALLS", "listens for outgoing calls")
                }
                _ => continue,
            };
            self.auto(
                &format!("android.permission.{perm}"),
                format!("{owner} {what}"),
            );
        }

        // Build the node.
        let mut node = Node::new(tag).attr("name", &name);
        for (k, v) in attrs.iter().filter(|(k, _)| *k != "name") {
            node = node.attr(k, v.as_str());
        }
        node.children.extend(filters);
        node.children.extend(metas);
        node.children.extend(properties);
        if let Some(Value::Table(l)) = t.get("layout") {
            let mut n = Node::new("layout");
            for (k, v) in l {
                n = n.attr(k, scalar(v, k, "|")?);
            }
            node.children.push(n);
        } else if t.contains_key("layout") {
            return Err(format!("{owner}: layout must be a table"));
        }
        for (key, child) in [
            ("grant_uri_permission", "grant-uri-permission"),
            ("path_permission", "path-permission"),
        ] {
            for g in tables(t, key)? {
                if kind != Kind::Provider {
                    return Err(format!("{owner}: {key} is for providers"));
                }
                let mut n = Node::new(child);
                for (k, v) in g {
                    n = n.attr(k, scalar(v, k, "|")?);
                }
                node.children.push(n);
            }
        }
        self.components.entry(kind.key()).or_default().push(node);
        Ok(())
    }

    fn tree(&self, t: &Table, owner: &str) -> R<Node> {
        let mut n = Node::new(need_str(t, "tag", owner)?);
        if let Some(Value::Table(a)) = t.get("attrs") {
            for (k, v) in a {
                n = n.literal(k, scalar(v, k, "|")?);
            }
        } else if t.contains_key("attrs") {
            return Err(format!("{owner}: attrs must be a table"));
        }
        for c in tables(t, "children")? {
            n = n.with(self.tree(c, owner)?);
        }
        Ok(n)
    }
}

impl Declarations {
    /// The manifest and resources the declarations stand for.
    pub fn render(&self, cx: &Context) -> R<Rendered> {
        let a = &self.android;
        let mut b = Builder::default();

        // Permissions.
        for p in str_list(a, "permissions", None)? {
            b.permission(&p, vec![])?;
        }
        for p in tables(a, "permission")? {
            let name = need_str(p, "name", "every <uses-permission>")?;
            let attrs = b
                .attributes(
                    p,
                    &[
                        "maxSdkVersion",
                        "usesPermissionFlags",
                        "requiredFeature",
                        "requiredNotFeature",
                        "minSdkVersion",
                    ],
                    "uses-permission",
                    &["name"],
                    &format!("permission {name}"),
                )?
                .into_iter()
                .collect();
            b.permission(name, attrs)?;
        }
        for (key, tag) in [
            ("define_permission", "permission"),
            ("permission_group", "permission-group"),
            ("permission_tree", "permission-tree"),
        ] {
            for p in tables(a, key)? {
                let name = need_str(p, "name", &format!("every <{tag}>"))?;
                let mut n = Node::new(tag).attr("name", name);
                for (k, v) in p.iter().filter(|(k, _)| *k != "name") {
                    n = n.attr(k, scalar(v, k, "|")?);
                }
                b.top.push(n);
            }
        }

        // Features.
        for f in tables(a, "feature")? {
            let mut n = Node::new("uses-feature");
            let (name, gl) = (get_str(f, "name")?, f.get("glEsVersion"));
            match (name, gl) {
                (None, None) => return Err("a <uses-feature> needs name or glEsVersion".into()),
                (Some(_), Some(_)) => {
                    return Err("a <uses-feature> takes name or glEsVersion, not both".into())
                }
                _ => {}
            }
            if let Some(n_) = name {
                n = n.attr("name", n_);
            }
            if let Some(gl) = gl {
                n = n.attr(
                    "glEsVersion",
                    match gl {
                        Value::Integer(i) => format!("0x{i:08x}"),
                        other => scalar(other, "glEsVersion", "")?,
                    },
                );
            }
            for (k, v) in f
                .iter()
                .filter(|(k, _)| !matches!(k.as_str(), "name" | "glEsVersion"))
            {
                n = n.attr(k, scalar(v, k, "|")?);
            }
            b.features.push(n);
        }

        // Presets become the generic tables they stand for.
        let mut by_kind: BTreeMap<&'static str, Vec<Table>> = BTreeMap::new();
        for kind in [
            Kind::Activity,
            Kind::Alias,
            Kind::Service,
            Kind::Receiver,
            Kind::Provider,
        ] {
            for t in tables(a, kind.key())? {
                for e in expand(
                    t,
                    &format!("<{}> {}", kind.tag(), get_str(t, "name")?.unwrap_or("?")),
                )? {
                    by_kind.entry(kind.key()).or_default().push(e);
                }
            }
        }
        self.presets(&mut b, &mut by_kind)?;

        // The standard launcher of a NativeActivity app, unless one is declared.
        if let Some(lib) = &cx.native_lib {
            let declared = by_kind.get("activity").is_some_and(|v| {
                v.iter().any(|t| {
                    get_str(t, "name").ok().flatten() == Some("android.app.NativeActivity")
                })
            });
            if !declared {
                by_kind.entry("activity").or_default().insert(
                    0,
                    text_table(&[
                        ("name", s("android.app.NativeActivity")),
                        ("exported", Value::Boolean(true)),
                        ("windowSoftInputMode", s("adjustResize")),
                        ("configChanges", s(super::host::CONFIG_CHANGES)),
                        ("native_lib", s(lib)),
                        ("launcher", Value::Boolean(true)),
                    ]),
                );
            }
        }
        for kind in [
            Kind::Activity,
            Kind::Alias,
            Kind::Service,
            Kind::Receiver,
            Kind::Provider,
        ] {
            for t in by_kind.get(kind.key()).cloned().unwrap_or_default() {
                b.component(kind, &t)?;
            }
        }

        // Application.
        let mut app_attrs = BTreeMap::new();
        app_attrs.insert("label".to_string(), cx.label.clone());
        if cx.icon {
            app_attrs.insert("icon".into(), "@mipmap/ic_launcher".into());
        }
        if cx.native_lib.is_some() {
            for (k, v) in [
                ("hasCode", "true"),
                ("supportsRtl", "true"),
                ("enableOnBackInvokedCallback", "true"),
                ("theme", "@style/AppTheme"),
            ] {
                app_attrs.insert(k.into(), v.into());
            }
        }
        let mut app_children = vec![];
        for app in tables(a, "application")? {
            app_attrs.extend(b.attributes(
                app,
                APPLICATION_ATTRS,
                "application",
                &["meta_data", "property"],
                "<application>",
            )?);
            for m in tables(app, "meta_data")? {
                app_children.push(b.meta("meta-data", m, "<application>")?);
            }
            for p in tables(app, "property")? {
                app_children.push(b.meta("property", p, "<application>")?);
            }
        }
        if let Some(sec) = tables(a, "network_security")?.first() {
            let cleartext = get_bool(sec, "cleartext")?.unwrap_or(false);
            let mut anchors =
                Node::new("trust-anchors").with(Node::new("certificates").literal("src", "system"));
            if get_bool(sec, "trust_user_certs")? == Some(true) {
                anchors = anchors.with(Node::new("certificates").literal("src", "user"));
            }
            let mut root = Node::new("network-security-config").with(
                Node::new("base-config")
                    .literal("cleartextTrafficPermitted", cleartext.to_string())
                    .with(anchors),
            );
            for d in tables(sec, "domains")? {
                root = root.with(
                    Node::new("domain-config")
                        .literal(
                            "cleartextTrafficPermitted",
                            get_bool(d, "cleartext")?.unwrap_or(true).to_string(),
                        )
                        .with(
                            Node::new("domain")
                                .literal(
                                    "includeSubdomains",
                                    get_bool(d, "include_subdomains")?
                                        .unwrap_or(true)
                                        .to_string(),
                                )
                                .with_text(need_str(d, "name", "a network_security domain")?),
                        ),
                );
            }
            b.resource("xml/network_security_config.xml".into(), xml_file(&root))?;
            app_attrs.insert(
                "networkSecurityConfig".into(),
                "@xml/network_security_config".into(),
            );
        }
        for (key, tag) in [
            ("uses_library", "uses-library"),
            ("uses_native_library", "uses-native-library"),
        ] {
            for l in tables(a, key)? {
                let mut n =
                    Node::new(tag).attr("name", need_str(l, "name", &format!("every <{tag}>"))?);
                if let Some(r) = get_bool(l, "required")? {
                    n = n.attr("required", r.to_string());
                }
                app_children.push(n);
            }
        }

        // Queries.
        if let Some(q) = tables(a, "queries")?.first() {
            let mut n = Node::new("queries");
            for p in str_list(q, "packages", None)? {
                n = n.with(Node::new("package").attr("name", p));
            }
            for p in str_list(q, "providers", None)? {
                n = n.with(Node::new("provider").attr("authorities", p));
            }
            for i in tables(q, "intent")? {
                n = n.with(b.filter(i, "intent", "<queries>", false)?.0);
            }
            b.queries = Some(n);
        }

        // Generic resources and elements.
        for r in tables(a, "resource_xml")? {
            let path = need_str(r, "path", "every resource_xml")?;
            let mut root = b.tree(r, &format!("resource_xml {path}"))?;
            if !root.attrs.iter().any(|(k, _)| k == "xmlns:android") {
                root.attrs
                    .insert(0, ("xmlns:android".into(), ANDROID_NS.into()));
            }
            let path = path.trim_end_matches(".xml");
            b.resource(format!("{path}.xml"), xml_file(&root))?;
        }
        for e in tables(a, "element")? {
            let node = b.tree(e, "element")?;
            match get_str(e, "parent")?.unwrap_or("manifest") {
                "manifest" => b.manifest_elements.push(node),
                "application" => b.application_elements.push(node),
                other => {
                    return Err(format!(
                        "element parent \"{other}\": manifest or application"
                    ))
                }
            }
        }
        b.application_elements.splice(0..0, app_children);

        // Permissions Android needs for what is declared.
        let declared: BTreeSet<String> = b.permissions.iter().map(|(n, _)| n.clone()).collect();
        let auto: Vec<(String, String)> = b.auto_permissions.clone().into_iter().collect();
        for (name, why) in auto {
            if !declared.contains(&name) {
                b.notes.push(format!("added {name} ({why})"));
                b.permission(&name, vec![])?;
            }
        }

        // Output.
        let mut root = Node::new("manifest")
            .literal("xmlns:android", ANDROID_NS)
            .literal("xmlns:tools", TOOLS_NS);
        for (name, attrs) in &b.permissions {
            let max = attrs
                .iter()
                .find(|(k, _)| k == "maxSdkVersion")
                .and_then(|(_, v)| v.parse::<u32>().ok());
            if max.is_some_and(|m| m < cx.min_sdk) {
                b.notes.push(format!(
                    "left out {name}: it stops below minSdk {}",
                    cx.min_sdk
                ));
                continue;
            }
            let mut n = Node::new("uses-permission").attr("name", name);
            n.attrs
                .extend(attrs.iter().map(|(k, v)| (qualify(k), v.clone())));
            root = root.with(n);
        }
        for n in b.top.iter().chain(&b.features) {
            root = root.with(n.clone());
        }
        if let Some(q) = b.queries.clone() {
            root = root.with(q);
        }
        for n in b.manifest_elements.iter() {
            root = root.with(n.clone());
        }
        let mut app = Node::new("application");
        for (k, v) in &app_attrs {
            app = app.attr(k, v.as_str());
        }
        app.children.extend(b.application_elements.iter().cloned());
        for kind in [
            Kind::Activity,
            Kind::Alias,
            Kind::Service,
            Kind::Receiver,
            Kind::Provider,
        ] {
            if let Some(list) = b.components.get(kind.key()) {
                app.children.extend(list.iter().cloned());
            }
        }
        root = root.with(app);
        Ok(Rendered {
            manifest: xml_file(&root),
            resources: b.resources,
            warnings: b.warnings,
            notes: b.notes,
        })
    }

    fn presets(&self, b: &mut Builder, by_kind: &mut BTreeMap<&'static str, Vec<Table>>) -> R<()> {
        let a = &self.android;

        for w in tables(a, "work_profile")? {
            let what = "work_profile";
            let admin = need_str(w, "admin", what)?;
            let compliance = need_str(w, "compliance", what)?;
            let direct = get_bool(w, "directBootAware")?.unwrap_or(true);
            let lib = get_str(w, "lib")?;
            let mut receiver = text_table(&[
                ("name", s(admin)),
                ("directBootAware", Value::Boolean(direct)),
                (
                    "admin_policies",
                    w.get("policies").cloned().unwrap_or(Value::Array(vec![])),
                ),
                (
                    "intent_filter",
                    Value::Array(vec![Value::Table(text_table(&[(
                        "actions",
                        strs(&[
                            "android.app.action.DEVICE_ADMIN_ENABLED",
                            "android.app.action.PROFILE_PROVISIONING_COMPLETE",
                        ]),
                    )]))]),
                ),
            ]);
            let mut activity = text_table(&[
                ("name", s(compliance)),
                ("exported", Value::Boolean(true)),
                ("directBootAware", Value::Boolean(direct)),
                ("permission", s("android.permission.BIND_DEVICE_ADMIN")),
                (
                    "intent_filter",
                    Value::Array(vec![Value::Table(text_table(&[
                        (
                            "actions",
                            strs(&["android.app.action.ADMIN_POLICY_COMPLIANCE"]),
                        ),
                        ("categories", strs(&["android.intent.category.DEFAULT"])),
                    ]))]),
                ),
            ]);
            if let Some(r) = get_str(w, "admin_resource")? {
                receiver.insert("admin_resource".into(), s(r));
            }
            if let Some(l) = lib {
                receiver.insert("lib".into(), s(l));
                activity.insert("lib".into(), s(l));
            }
            by_kind.entry("receiver").or_default().push(receiver);
            by_kind.entry("activity").or_default().push(activity);
            if get_bool(w, "quiet_mode")?.unwrap_or(true) {
                b.permission("android.permission.MODIFY_QUIET_MODE", vec![])?;
            }
        }

        for p in tables(a, "boot_start")? {
            let name = need_str(p, "receiver", "boot_start")?;
            let locked = get_bool(p, "locked")?.unwrap_or(false);
            let mut actions = vec!["android.intent.action.BOOT_COMPLETED".to_string()];
            if locked {
                actions.push("android.intent.action.LOCKED_BOOT_COMPLETED".into());
            }
            if get_bool(p, "package_replaced")?.unwrap_or(false) {
                actions.push("android.intent.action.MY_PACKAGE_REPLACED".into());
            }
            actions.extend(str_list(p, "actions", None)?);
            let mut r = Table::new();
            for (k, v) in p {
                if !matches!(
                    k.as_str(),
                    "receiver" | "locked" | "package_replaced" | "actions"
                ) {
                    r.insert(k.clone(), v.clone());
                }
            }
            r.insert("name".into(), s(name));
            r.entry("exported").or_insert(Value::Boolean(true));
            if locked {
                r.entry("directBootAware").or_insert(Value::Boolean(true));
            }
            r.insert(
                "intent_filter".into(),
                Value::Array(vec![Value::Table(text_table(&[(
                    "actions",
                    Value::Array(actions.iter().map(|x| s(x)).collect()),
                )]))]),
            );
            by_kind.entry("receiver").or_default().push(r);
        }

        for p in tables(a, "process_pool")? {
            let prefix = need_str(p, "prefix", "process_pool")?;
            let kind = get_str(p, "kind")?.unwrap_or("service");
            let key = match kind {
                "service" => "service",
                "receiver" => "receiver",
                "provider" => "provider",
                "activity" => "activity",
                other => {
                    return Err(format!(
                        "process_pool kind \"{other}\": service, receiver, provider or activity"
                    ))
                }
            };
            let default_process = {
                let last = snake(prefix);
                if last.is_empty() {
                    return Err(format!(
                        "process_pool prefix \"{prefix}\" needs a class name part"
                    ));
                }
                format!(":{last}")
            };
            let process = get_str(p, "process_prefix")?
                .map(str::to_string)
                .unwrap_or(default_process);
            let mut t = Table::new();
            for (k, v) in p {
                if !matches!(k.as_str(), "prefix" | "kind" | "process_prefix") {
                    t.insert(k.clone(), v.clone());
                }
            }
            t.insert("name".into(), s(&format!("{prefix}{{i}}")));
            t.insert("process".into(), s(&format!("{process}{{i}}")));
            t.entry("exported").or_insert(Value::Boolean(false));
            if !t.contains_key("count") {
                return Err("process_pool needs count".into());
            }
            for e in expand(&t, "process_pool")? {
                by_kind.entry(key).or_default().push(e);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cx() -> Context {
        Context {
            label: "App".into(),
            icon: false,
            native_lib: None,
            min_sdk: 31,
        }
    }

    fn render_with(src: &str, cx: &Context) -> R<Rendered> {
        parse(src)?.expect("declares something").render(cx)
    }

    fn ok(src: &str) -> Rendered {
        render_with(src, &cx()).unwrap_or_else(|e| panic!("{e}\n{src}"))
    }

    fn err(src: &str) -> String {
        render_with(src, &cx()).expect_err("must be refused")
    }

    fn squash(s: &str) -> String {
        s.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    fn has(r: &Rendered, needle: &str) {
        assert!(
            squash(&r.manifest).contains(&squash(needle)),
            "missing {needle}\n{}",
            r.manifest
        );
    }

    fn lacks(r: &Rendered, needle: &str) {
        assert!(
            !squash(&r.manifest).contains(&squash(needle)),
            "unexpected {needle}\n{}",
            r.manifest
        );
    }

    fn warned(r: &Rendered, needle: &str) {
        assert!(
            r.warnings.iter().any(|w| w.contains(needle)),
            "no warning with {needle}: {:?}",
            r.warnings
        );
    }

    #[test]
    fn a_config_without_declarations_declares_nothing() {
        let flat = "[android]\ngenerate = true\nlabel = \"X\"\nicon = \"a.webp\"\n";
        assert_eq!(parse(flat).unwrap(), None);
        assert_eq!(parse("not toml = = [").unwrap(), None);
        assert!(parse("[declare]\npermissions = [\n").is_err());
        assert!(parse("[declare]\n[[declare.service]]\nname = ").is_err());
    }

    #[test]
    fn permissions_take_short_names_attributes_and_stay_unique() {
        let r = ok(r#"
[declare]
permissions = ["INTERNET", "com.example.CUSTOM", "INTERNET"]
[[declare.permission]]
name = "android.permission.WRITE_EXTERNAL_STORAGE"
maxSdkVersion = 28
[[declare.permission]]
name = "android.permission.BLUETOOTH_SCAN"
usesPermissionFlags = "neverForLocation"
[[declare.permission]]
name = "android.permission.READ_EXTERNAL_STORAGE"
maxSdkVersion = 32
"#);
        has(
            &r,
            r#"<uses-permission android:name="android.permission.INTERNET" />"#,
        );
        has(
            &r,
            r#"<uses-permission android:name="com.example.CUSTOM" />"#,
        );
        has(
            &r,
            r#"<uses-permission android:name="android.permission.BLUETOOTH_SCAN" android:usesPermissionFlags="neverForLocation" />"#,
        );
        has(
            &r,
            r#"android:name="android.permission.READ_EXTERNAL_STORAGE" android:maxSdkVersion="32""#,
        );
        assert_eq!(r.manifest.matches("permission.INTERNET").count(), 1);
        lacks(&r, "WRITE_EXTERNAL_STORAGE");
        assert!(r.notes.iter().any(|n| n.contains("WRITE_EXTERNAL_STORAGE")));
    }

    #[test]
    fn one_permission_twice_with_other_attributes_is_refused() {
        let e = err(r#"
[declare]
permissions = ["CAMERA"]
[[declare.permission]]
name = "CAMERA"
maxSdkVersion = 30
"#);
        assert!(e.contains("declared twice"), "{e}");
    }

    #[test]
    fn features_take_a_name_or_a_gl_version() {
        let r = ok(r#"
[[declare.feature]]
name = "android.hardware.camera"
required = false
[[declare.feature]]
glEsVersion = 0x00030001
required = true
"#);
        has(
            &r,
            r#"<uses-feature android:name="android.hardware.camera" android:required="false" />"#,
        );
        has(
            &r,
            r#"<uses-feature android:glEsVersion="0x00030001" android:required="true" />"#,
        );
        assert!(err("[[declare.feature]]\nrequired = true\n").contains("name or glEsVersion"));
        assert!(err("[[declare.feature]]\nname = \"a\"\nglEsVersion = 1\n").contains("not both"));
    }

    #[test]
    fn the_application_gets_label_default_and_any_attribute() {
        let r = ok(r#"
[declare.application]
allowBackup = false
largeHeap = true
theme = "@style/T"
[[declare.application.meta_data]]
name = "k"
value = "v"
[[declare.application.property]]
name = "p"
resource = "@xml/p"
[[declare.uses_library]]
name = "org.apache.http.legacy"
required = false
"#);
        has(&r, r#"android:label="App""#);
        has(&r, r#"android:allowBackup="false""#);
        has(&r, r#"android:largeHeap="true""#);
        has(&r, r#"<meta-data android:name="k" android:value="v" />"#);
        has(
            &r,
            r#"<property android:name="p" android:resource="@xml/p" />"#,
        );
        has(
            &r,
            r#"<uses-library android:name="org.apache.http.legacy" android:required="false" />"#,
        );
        let r = ok("[declare.application]\nlabel = \"Mine\"\n");
        has(&r, r#"android:label="Mine""#);
        let r = render_with("[declare.application]\n", &Context { icon: true, ..cx() }).unwrap();
        has(&r, r#"android:icon="@mipmap/ic_launcher""#);
    }

    #[test]
    fn an_activity_takes_every_attribute_filter_and_meta_data() {
        let r = ok(r#"
[[declare.activity]]
name = ".Main"
exported = true
launchMode = "singleTask"
configChanges = ["orientation", "screenSize"]
windowSoftInputMode = "adjustResize"
"tools:targetApi" = "34"
native_lib = "ui"
lib = "ui"
layout = { defaultWidth = "600dp", gravity = "end" }
[[declare.activity.intent_filter]]
actions = ["android.intent.action.VIEW"]
categories = ["android.intent.category.DEFAULT", "android.intent.category.BROWSABLE"]
autoVerify = true
data = [{ scheme = "https", host = "example.com", pathPrefix = "/x" }, { mimeType = "image/*" }]
"#);
        has(&r, r#"android:name=".Main""#);
        has(&r, r#"android:launchMode="singleTask""#);
        has(&r, r#"android:configChanges="orientation|screenSize""#);
        has(&r, r#"tools:targetApi="34""#);
        has(
            &r,
            r#"<meta-data android:name="android.app.lib_name" android:value="ui" />"#,
        );
        has(
            &r,
            r#"<meta-data android:name="mobilesentinel.lib" android:value="ui" />"#,
        );
        has(
            &r,
            r#"<layout android:defaultWidth="600dp" android:gravity="end" />"#,
        );
        has(&r, r#"<intent-filter android:autoVerify="true">"#);
        has(
            &r,
            r#"<category android:name="android.intent.category.BROWSABLE" />"#,
        );
        has(
            &r,
            r#"<data android:scheme="https" android:host="example.com" android:pathPrefix="/x" />"#,
        );
        has(&r, r#"<data android:mimeType="image/*" />"#);
        has(&r, r#"xmlns:tools="http://schemas.android.com/tools""#);
    }

    #[test]
    fn launcher_adds_main_and_launcher_and_exports() {
        let r = ok("[[declare.activity]]\nname = \".Main\"\nlauncher = true\n");
        has(&r, r#"android:exported="true""#);
        has(
            &r,
            r#"<action android:name="android.intent.action.MAIN" />"#,
        );
        has(
            &r,
            r#"<category android:name="android.intent.category.LAUNCHER" />"#,
        );
        assert!(err("[[declare.service]]\nname = \".S\"\nlauncher = true\n")
            .contains("launcher is for activities"));
    }

    #[test]
    fn an_activity_alias_a_provider_and_a_receiver() {
        let r = ok(r#"
[[declare.activity_alias]]
name = ".Alias"
targetActivity = ".Main"
exported = true
enabled = false
[[declare.provider]]
name = ".Files"
authorities = ["a.b.files", "a.b.more"]
exported = false
grantUriPermissions = true
file_paths = [{ kind = "files-path", name = "docs", path = "docs/" }, { kind = "cache-path", name = "tmp" }]
grant_uri_permission = [{ pathPrefix = "/public" }]
path_permission = [{ pathPrefix = "/secret", readPermission = "a.b.READ" }]
"#);
        has(
            &r,
            r#"<activity-alias android:name=".Alias" android:enabled="false" android:exported="true" android:targetActivity=".Main" />"#,
        );
        has(&r, r#"android:authorities="a.b.files;a.b.more""#);
        has(
            &r,
            r#"<meta-data android:name="android.support.FILE_PROVIDER_PATHS" android:resource="@xml/file_paths_files" />"#,
        );
        has(
            &r,
            r#"<grant-uri-permission android:pathPrefix="/public" />"#,
        );
        has(
            &r,
            r#"<path-permission android:pathPrefix="/secret" android:readPermission="a.b.READ" />"#,
        );
        let paths = &r.resources["xml/file_paths_files.xml"];
        assert!(
            paths.contains(r#"<files-path name="docs" path="docs/" />"#),
            "{paths}"
        );
        assert!(
            paths.contains(r#"<cache-path name="tmp" path="." />"#),
            "{paths}"
        );
        assert!(err("[[declare.activity_alias]]\nname = \".A\"\n").contains("targetActivity"));
        assert!(err("[[declare.provider]]\nname = \".P\"\n").contains("authorities"));
        assert!(err("[[declare.provider]]\nname = \".P\"\nauthorities = \"a\"\nfile_paths = [{ kind = \"bogus\", name = \"x\" }]\n").contains("bogus"));
    }

    #[test]
    fn a_foreground_service_gets_the_permissions_of_its_types() {
        let r = ok(r#"
[[declare.service]]
name = ".A"
foregroundServiceType = ["dataSync", "location", "shortService"]
[[declare.service]]
name = ".B"
foregroundServiceType = "specialUse"
special_use = "keeps the relay alive"
"#);
        for p in [
            "FOREGROUND_SERVICE",
            "FOREGROUND_SERVICE_DATA_SYNC",
            "FOREGROUND_SERVICE_LOCATION",
            "FOREGROUND_SERVICE_SPECIAL_USE",
        ] {
            has(
                &r,
                &format!(r#"<uses-permission android:name="android.permission.{p}" />"#),
            );
        }
        lacks(&r, "FOREGROUND_SERVICE_SHORT_SERVICE");
        has(
            &r,
            r#"android:foregroundServiceType="dataSync|location|shortService""#,
        );
        has(
            &r,
            r#"<property android:name="android.app.PROPERTY_SPECIAL_USE_FGS_SUBTYPE" android:value="keeps the relay alive" />"#,
        );
        warned(&r, "specialUse");
    }

    #[test]
    fn every_foreground_service_type_has_its_permission() {
        for (ty, perm) in FGS_TYPES {
            let sub = if *ty == "specialUse" {
                "special_use = \"x\"\n"
            } else {
                ""
            };
            let r = ok(&format!(
                "[[declare.service]]\nname = \".S\"\nforegroundServiceType = \"{ty}\"\n{sub}"
            ));
            has(&r, r#"android.permission.FOREGROUND_SERVICE" />"#);
            if let Some(p) = perm {
                has(&r, &format!("android.permission.{p}"));
            }
        }
    }

    #[test]
    fn foreground_service_mistakes_are_refused() {
        assert!(
            err("[[declare.service]]\nname = \".S\"\nforegroundServiceType = \"gps\"\n")
                .contains("\"gps\"")
        );
        assert!(err(
            "[[declare.service]]\nname = \".S\"\nforegroundServiceType = \"specialUse\"\n"
        )
        .contains("special_use"));
        assert!(
            err("[[declare.service]]\nname = \".S\"\nspecial_use = \"x\"\n")
                .contains("foregroundServiceType")
        );
        assert!(
            err("[[declare.receiver]]\nname = \".R\"\nforegroundServiceType = \"dataSync\"\n")
                .contains("services")
        );
        ok(r#"
[[declare.service]]
name = ".S"
foregroundServiceType = "specialUse"
property = [{ name = "android.app.PROPERTY_SPECIAL_USE_FGS_SUBTYPE", value = "x" }]
"#);
    }

    #[test]
    fn a_boot_receiver_gets_its_permission_once() {
        let src = r#"
[[declare.receiver]]
name = ".Boot"
exported = true
[[declare.receiver.intent_filter]]
actions = ["android.intent.action.LOCKED_BOOT_COMPLETED", "android.intent.action.BOOT_COMPLETED"]
"#;
        let r = ok(src);
        assert_eq!(r.manifest.matches("RECEIVE_BOOT_COMPLETED").count(), 1);
        assert!(r
            .notes
            .iter()
            .any(|n| n.contains("RECEIVE_BOOT_COMPLETED") && n.contains(".Boot")));
        let r = ok(&format!(
            "[declare]\npermissions = [\"RECEIVE_BOOT_COMPLETED\"]\n{src}"
        ));
        assert_eq!(r.manifest.matches("RECEIVE_BOOT_COMPLETED").count(), 1);
        assert!(r.notes.is_empty());
        let r = ok(r#"
[[declare.receiver]]
name = ".Sms"
exported = true
[[declare.receiver.intent_filter]]
actions = ["android.provider.Telephony.SMS_RECEIVED"]
"#);
        has(&r, "android.permission.RECEIVE_SMS");
        warned(&r, "SMS");
    }

    #[test]
    fn count_repeats_a_component_with_its_number() {
        let r = ok(r#"
[[declare.service]]
name = ".G{i}"
process = ":g{i}"
exported = false
count = 3
"#);
        for i in 0..3 {
            has(
                &r,
                &format!(
                    r#"<service android:name=".G{i}" android:exported="false" android:process=":g{i}" />"#
                ),
            );
        }
        lacks(&r, ".G3");
        assert!(err("[[declare.service]]\nname = \".G\"\ncount = 3\n").contains("without {i}"));
        assert!(err("[[declare.service]]\nname = \".G{i}\"\n").contains("needs count"));
        assert!(err("[[declare.service]]\nname = \".G{i}\"\ncount = 0\n").contains("1 or more"));
        assert!(
            err("[[declare.service]]\nname = \".G{i}\"\ncount = \"3\"\n").contains("whole number")
        );
    }

    #[test]
    fn intent_filters_are_checked() {
        let base = "[[declare.activity]]\nname = \".A\"\nexported = true\n[[declare.activity.intent_filter]]\n";
        assert!(err(&format!("{base}categories = [\"x\"]\n")).contains("without an action"));
        assert!(err(&format!(
            "{base}action = \"a\"\ndata = {{ host = \"h\" }}\n"
        ))
        .contains("no scheme"));
        assert!(err(&format!(
            "{base}action = \"a\"\ndata = {{ scheme = \"s\", path = \"/p\" }}\n"
        ))
        .contains("no host"));
        assert!(err(&format!(
            "{base}action = \"a\"\ndata = {{ nope = \"s\" }}\n"
        ))
        .contains("nope"));
        assert!(err(&format!("{base}action = \"a\"\nbogus = 1\n")).contains("bogus"));
        let r = ok(&format!(
            "{base}action = \"a\"\ncategory = \"c\"\npriority = 5\n"
        ));
        has(&r, r#"<intent-filter android:priority="5">"#);
        let e = err(
            "[[declare.activity]]\nname = \".A\"\n[[declare.activity.intent_filter]]\naction = \"a\"\n",
        );
        assert!(e.contains("exported"), "{e}");
    }

    #[test]
    fn invalid_values_are_refused() {
        assert!(err("[[declare.service]]\n").contains("needs name"));
        assert!(
            err("[[declare.service]]\nname = \".S\"\n[[declare.service]]\nname = \".S\"\n")
                .contains("twice")
        );
        assert!(err("[[declare.service]]\nname = \".S\"\nprocess = \"Bad\"\n").contains("process"));
        assert!(err("[[declare.service]]\nname = \".S\"\nprocess = \":\"\n").contains("process"));
        assert!(
            err("[[declare.activity]]\nname = \".A\"\nlaunchMode = \"always\"\n")
                .contains("launchMode")
        );
        assert!(
            err("[[declare.service]]\nname = \".A\"\nlaunchMode = \"singleTop\"\n")
                .contains("launchMode")
        );
        assert!(
            err("[[declare.activity]]\nname = \".A\"\nscreenOrientation = \"tilted\"\n")
                .contains("screenOrientation")
        );
        assert!(
            err("[[declare.activity]]\nname = \".A\"\nlabel = { a = 1 }\n")
                .contains("not an attribute value")
        );
        assert!(
            err("[[declare.activity]]\nname = \".A\"\nlib = \"my-lib\"\n")
                .contains("letters and digits")
        );
        assert!(
            err("[[declare.service]]\nname = \".A\"\nnative_lib = \"x\"\n").contains("activities")
        );
        ok("[[declare.service]]\nname = \".S\"\nprocess = \"com.example.shared\"\n");
    }

    #[test]
    fn meta_data_wants_a_value_or_a_resource() {
        let c = "[[declare.service]]\nname = \".S\"\n[[declare.service.meta_data]]\nname = \"k\"\n";
        assert!(err(c).contains("value or resource"));
        assert!(err(&format!("{c}value = 1\nresource = \"@x/y\"\n")).contains("pick one"));
        let r = ok(&format!("{c}value = 42\n"));
        has(&r, r#"<meta-data android:name="k" android:value="42" />"#);
    }

    #[test]
    fn a_device_admin_gets_its_policy_file_and_defaults() {
        let r = ok(r#"
[[declare.receiver]]
name = ".space.Admin"
admin_policies = ["force-lock", "wipe-data"]
lib = "ui"
"#);
        has(
            &r,
            r#"android:permission="android.permission.BIND_DEVICE_ADMIN""#,
        );
        has(&r, r#"android:exported="true""#);
        has(
            &r,
            r#"<meta-data android:name="android.app.device_admin" android:resource="@xml/device_admin_admin" />"#,
        );
        has(
            &r,
            r#"<action android:name="android.app.action.DEVICE_ADMIN_ENABLED" />"#,
        );
        let xml = squash(&r.resources["xml/device_admin_admin.xml"]);
        assert!(
            xml.contains("<uses-policies> <force-lock /> <wipe-data /> </uses-policies>"),
            "{xml}"
        );
        warned(&r, "device admin");
    }

    #[test]
    fn every_device_admin_policy_can_be_selected() {
        let all = ok("[[declare.receiver]]\nname = \".A\"\nadmin_policies = [\"all\"]\n");
        let xml = &all.resources["xml/device_admin_a.xml"];
        for p in ADMIN_POLICIES {
            assert!(xml.contains(&format!("<{p} />")), "{p} in {xml}");
        }
        for p in ADMIN_POLICIES {
            let one = ok(&format!(
                "[[declare.receiver]]\nname = \".A\"\nadmin_policies = [\"{p}\"]\n"
            ));
            assert_eq!(
                one.resources["xml/device_admin_a.xml"]
                    .matches("/>")
                    .count(),
                1
            );
        }
        let named = ok(
            "[[declare.receiver]]\nname = \".A\"\nadmin_policies = []\nadmin_resource = \"policies\"\n",
        );
        assert!(named.resources.contains_key("xml/policies.xml"));
        assert!(
            err("[[declare.receiver]]\nname = \".A\"\nadmin_policies = [\"fly\"]\n")
                .contains("\"fly\"")
        );
        assert!(
            err("[[declare.service]]\nname = \".A\"\nadmin_policies = []\n").contains("receivers")
        );
    }

    #[test]
    fn two_files_of_one_name_with_other_content_are_refused() {
        let e = err(r#"
[[declare.receiver]]
name = ".A"
admin_policies = ["force-lock"]
admin_resource = "same"
[[declare.receiver]]
name = ".B"
admin_policies = ["wipe-data"]
admin_resource = "same"
"#);
        assert!(e.contains("res/xml/same.xml"), "{e}");
        ok(r#"
[[declare.receiver]]
name = ".A"
admin_policies = ["force-lock"]
admin_resource = "same"
[[declare.receiver]]
name = ".B"
admin_policies = ["force-lock"]
admin_resource = "same"
"#);
    }

    #[test]
    fn an_accessibility_service_gets_its_config() {
        let r = ok(r#"
[[declare.service]]
name = ".Watch"
accessibility = { accessibilityEventTypes = ["typeViewClicked", "typeWindowStateChanged"], accessibilityFeedbackType = "feedbackGeneric", canRetrieveWindowContent = true, notificationTimeout = 100 }
"#);
        has(
            &r,
            r#"android:permission="android.permission.BIND_ACCESSIBILITY_SERVICE""#,
        );
        has(
            &r,
            r#"<action android:name="android.accessibilityservice.AccessibilityService" />"#,
        );
        has(
            &r,
            r#"<meta-data android:name="android.accessibilityservice" android:resource="@xml/accessibility_watch" />"#,
        );
        let xml = &r.resources["xml/accessibility_watch.xml"];
        assert!(
            xml.contains(
                r#"android:accessibilityEventTypes="typeViewClicked|typeWindowStateChanged""#
            ),
            "{xml}"
        );
        assert!(
            xml.contains(r#"android:canRetrieveWindowContent="true""#),
            "{xml}"
        );
        warned(&r, "accessibility");
    }

    #[test]
    fn network_security_and_free_form_resources() {
        let r = ok(r#"
[declare.application]
label = "x"
[declare.network_security]
cleartext = false
trust_user_certs = true
domains = [{ name = "10.0.0.5", include_subdomains = false }, { name = "dev.example.com", cleartext = false }]
[[declare.resource_xml]]
path = "xml/backup_rules"
tag = "full-backup-content"
children = [{ tag = "exclude", attrs = { domain = "sharedpref", path = "device.xml" } }]
"#);
        has(
            &r,
            r#"android:networkSecurityConfig="@xml/network_security_config""#,
        );
        let n = squash(&r.resources["xml/network_security_config.xml"]);
        assert!(
            n.contains(r#"<base-config cleartextTrafficPermitted="false">"#),
            "{n}"
        );
        assert!(n.contains(r#"<certificates src="user" />"#), "{n}");
        assert!(
            n.contains(r#"<domain includeSubdomains="false">10.0.0.5</domain>"#),
            "{n}"
        );
        assert!(
            n.contains(r#"<domain-config cleartextTrafficPermitted="false">"#),
            "{n}"
        );
        let b = squash(&r.resources["xml/backup_rules.xml"]);
        assert!(
            b.contains(
                r#"<full-backup-content xmlns:android="http://schemas.android.com/apk/res/android">"#
            ),
            "{b}"
        );
        assert!(
            b.contains(r#"<exclude domain="sharedpref" path="device.xml" />"#),
            "{b}"
        );
    }

    #[test]
    fn any_other_element_goes_in_as_a_tree() {
        let r = ok(r#"
[[declare.element]]
tag = "supports-screens"
attrs = { "android:largeScreens" = "true" }
[[declare.element]]
parent = "application"
tag = "profileable"
attrs = { "android:shell" = "true" }
[[declare.element]]
tag = "uses-sdk-library"
attrs = { "android:name" = "x" }
children = [{ tag = "child" }]
"#);
        has(&r, r#"<supports-screens android:largeScreens="true" />"#);
        has(&r, r#"<profileable android:shell="true" />"#);
        has(
            &r,
            r#"<uses-sdk-library android:name="x"> <child /> </uses-sdk-library>"#,
        );
        assert!(
            err("[[declare.element]]\ntag = \"a\"\nparent = \"activity\"\n").contains("parent")
        );
        assert!(err("[[declare.element]]\nattrs = {}\n").contains("tag"));
    }

    #[test]
    fn queries_list_packages_providers_and_intents() {
        let r = ok(r#"
[declare.queries]
packages = ["com.a", "com.b"]
providers = ["com.a.provider"]
[[declare.queries.intent]]
action = "android.intent.action.SEND"
data = { mimeType = "text/plain" }
"#);
        has(&r, r#"<package android:name="com.a" />"#);
        has(&r, r#"<provider android:authorities="com.a.provider" />"#);
        has(
            &r,
            r#"<intent> <action android:name="android.intent.action.SEND" /> <data android:mimeType="text/plain" /> </intent>"#,
        );
        assert!(
            err("[[declare.queries.intent]]\naction = \"a\"\nlabel = \"x\"\n")
                .contains("not allowed")
        );
    }

    #[test]
    fn the_work_profile_preset_declares_a_profile_owner() {
        let r = ok(r#"
[declare.work_profile]
admin = ".space.Admin"
compliance = ".space.Comply"
policies = ["force-lock"]
lib = "ui"
admin_resource = "device_admin"
"#);
        has(
            &r,
            r#"<action android:name="android.app.action.PROFILE_PROVISIONING_COMPLETE" />"#,
        );
        has(
            &r,
            r#"<action android:name="android.app.action.ADMIN_POLICY_COMPLIANCE" />"#,
        );
        has(
            &r,
            r#"<category android:name="android.intent.category.DEFAULT" />"#,
        );
        has(
            &r,
            r#"<uses-permission android:name="android.permission.MODIFY_QUIET_MODE" />"#,
        );
        has(&r, r#"android:directBootAware="true""#);
        assert_eq!(r.manifest.matches(r#"android:value="ui""#).count(), 2);
        assert!(r.resources.contains_key("xml/device_admin.xml"));
        warned(&r, "device admin");
        let r =
            ok("[declare.work_profile]\nadmin = \".A\"\ncompliance = \".C\"\nquiet_mode = false\n");
        lacks(&r, "MODIFY_QUIET_MODE");
        assert!(err("[declare.work_profile]\nadmin = \".A\"\n").contains("compliance"));
    }

    #[test]
    fn the_boot_start_preset_declares_a_boot_receiver_and_its_permission() {
        let r = ok(r#"
[declare.boot_start]
receiver = ".Boot"
locked = true
package_replaced = true
actions = ["android.intent.action.USER_UNLOCKED"]
lib = "ui"
"#);
        for a in [
            "BOOT_COMPLETED",
            "LOCKED_BOOT_COMPLETED",
            "MY_PACKAGE_REPLACED",
            "USER_UNLOCKED",
        ] {
            has(
                &r,
                &format!(r#"<action android:name="android.intent.action.{a}" />"#),
            );
        }
        has(&r, r#"android:directBootAware="true""#);
        has(
            &r,
            r#"<uses-permission android:name="android.permission.RECEIVE_BOOT_COMPLETED" />"#,
        );
        let r = ok("[declare.boot_start]\nreceiver = \".B\"\n");
        lacks(&r, "LOCKED_BOOT_COMPLETED");
        lacks(&r, "directBootAware");
    }

    #[test]
    fn the_process_pool_preset_declares_services_in_their_own_processes() {
        let r = ok(r#"
[[declare.process_pool]]
prefix = ".worker.W"
count = 2
foregroundServiceType = "dataSync"
"#);
        has(&r, r#"android:name=".worker.W0""#);
        has(&r, r#"android:process=":w0""#);
        has(&r, r#"android:process=":w1""#);
        has(&r, r#"android:exported="false""#);
        has(&r, "FOREGROUND_SERVICE_DATA_SYNC");
        let r = ok(
            "[[declare.process_pool]]\nprefix = \".R\"\ncount = 1\nkind = \"receiver\"\nprocess_prefix = \":rx\"\n",
        );
        has(&r, r#"<receiver android:name=".R0""#);
        has(&r, r#"android:process=":rx0""#);
        assert!(err("[[declare.process_pool]]\nprefix = \".R\"\n").contains("count"));
        assert!(
            err("[[declare.process_pool]]\nprefix = \".R\"\ncount = 1\nkind = \"x\"\n")
                .contains("kind")
        );
    }

    #[test]
    fn play_sensitive_items_are_warned_about() {
        let r = ok("[declare]
permissions = [\"QUERY_ALL_PACKAGES\", \"READ_SMS\", \"INTERNET\", \"SYSTEM_ALERT_WINDOW\"]\n");
        warned(&r, "QUERY_ALL_PACKAGES");
        warned(&r, "READ_SMS");
        warned(&r, "SYSTEM_ALERT_WINDOW");
        assert!(!r.warnings.iter().any(|w| w.contains("INTERNET")));
        assert!(ok("[declare]
permissions = [\"INTERNET\"]\n")
        .warnings
        .is_empty());
    }

    #[test]
    fn a_misspelled_attribute_warns_and_still_passes_through() {
        let r = ok(
            "[[declare.service]]\nname = \".S\"\nexporte = true\ndirectbootaware = true\nmadeUp = 1\n",
        );
        warned(&r, "not a known attribute of <service>");
        assert!(r
            .warnings
            .iter()
            .any(|w| w.contains("did you mean directBootAware")));
        has(&r, r#"android:madeUp="1""#);
    }

    #[test]
    fn a_native_app_keeps_its_standard_launcher_unless_one_is_declared() {
        let native = Context {
            native_lib: Some("main".into()),
            ..cx()
        };
        let r = render_with(
            "[declare]
permissions = [\"CAMERA\"]\n",
            &native,
        )
        .unwrap();
        has(&r, r#"<activity android:name="android.app.NativeActivity""#);
        has(
            &r,
            r#"<meta-data android:name="android.app.lib_name" android:value="main" />"#,
        );
        has(&r, r#"android:enableOnBackInvokedCallback="true""#);
        has(&r, r#"android:theme="@style/AppTheme""#);
        let r = render_with(
            "[[declare.activity]]\nname = \"android.app.NativeActivity\"\nnative_lib = \"mine\"\nlauncher = true\n",
            &native,
        )
        .unwrap();
        assert_eq!(r.manifest.matches("android.app.NativeActivity").count(), 1);
        has(&r, r#"android:value="mine""#);
    }

    #[test]
    fn values_are_escaped_and_the_output_is_stable() {
        let src = "[[declare.service]]\nname = \".S\"\nlabel = \"a & \\\"b\\\" <c>\"\n";
        let r = ok(src);
        has(&r, r#"android:label="a &amp; &quot;b&quot; &lt;c&gt;""#);
        assert_eq!(r.manifest, ok(src).manifest);
        assert!(r
            .manifest
            .starts_with("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<manifest"));
    }
}
