//! A hand-written manifest of a real app (template, 128 generated guest services and the device
//! admin policy file) is expressible as declarations: the rendered manifest has the same tree.
//! The one difference is wanted: the template's receiver listens for BOOT_COMPLETED without
//! asking for RECEIVE_BOOT_COMPLETED; the declared version adds it.

use mobile_sentinel::build::declare::{parse, Context, Rendered};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug)]
struct Elem {
    tag: String,
    attrs: BTreeMap<String, String>,
    children: Vec<Elem>,
}

impl Elem {
    /// Attributes and children sorted: the order of elements and attributes means nothing.
    fn canonical(&self) -> String {
        let mut kids: Vec<String> = self.children.iter().map(Elem::canonical).collect();
        kids.sort();
        let attrs: Vec<String> = self
            .attrs
            .iter()
            .map(|(k, v)| format!("{k}={v:?}"))
            .collect();
        format!("<{} {}>[{}]", self.tag, attrs.join(" "), kids.join(","))
    }

    fn name(&self) -> Option<&str> {
        self.attrs.get("android:name").map(String::as_str)
    }
}

fn parse_xml(text: &str) -> Elem {
    let mut stack: Vec<Elem> = vec![];
    let mut root = None;
    let mut rest = text;
    while let Some(lt) = rest.find('<') {
        rest = &rest[lt..];
        if rest.starts_with("<?") {
            rest = &rest[rest.find("?>").unwrap() + 2..];
        } else if rest.starts_with("<!--") {
            rest = &rest[rest.find("-->").unwrap() + 3..];
        } else if rest.starts_with("</") {
            let done = stack.pop().unwrap();
            match stack.last_mut() {
                Some(parent) => parent.children.push(done),
                None => root = Some(done),
            }
            rest = &rest[rest.find('>').unwrap() + 1..];
        } else {
            let end = rest.find('>').unwrap();
            let mut head = &rest[1..end];
            let selfclose = head.ends_with('/');
            if selfclose {
                head = &head[..head.len() - 1];
            }
            let tag_end = head.find(char::is_whitespace).unwrap_or(head.len());
            let mut attrs = BTreeMap::new();
            let mut a = head[tag_end..].trim();
            while let Some(eq) = a.find('=') {
                let key = a[..eq].trim().to_string();
                let q = a[eq + 1..].trim_start();
                let close = q[1..].find('"').unwrap() + 1;
                if !key.starts_with("xmlns:") {
                    attrs.insert(key, q[1..close].replace("&quot;", "\""));
                }
                a = q[close + 1..].trim_start();
            }
            let el = Elem {
                tag: head[..tag_end].to_string(),
                attrs,
                children: vec![],
            };
            if selfclose {
                match stack.last_mut() {
                    Some(parent) => parent.children.push(el),
                    None => root = Some(el),
                }
            } else {
                stack.push(el);
            }
            rest = &rest[end + 1..];
        }
    }
    root.expect("a root element")
}

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn original_manifest() -> Elem {
    let template = std::fs::read_to_string(fixtures().join("virtual-main-manifest.xml")).unwrap();
    let guests: String = (0..128)
        .map(|i| {
            format!(
                "<service android:name=\".G{i}\" android:exported=\"false\" android:process=\":g{i}\" />\n"
            )
        })
        .collect();
    parse_xml(&template.replace("{{GUEST_SERVICES}}", &guests))
}

fn render() -> Rendered {
    let text = std::fs::read_to_string(fixtures().join("virtual-main-sentinel.toml")).unwrap();
    parse(&text)
        .unwrap()
        .expect("the fixture declares")
        .render(&Context {
            label: "Virtual Main".into(),
            icon: false,
            native_lib: None,
            min_sdk: 31,
        })
        .unwrap()
}

#[test]
fn the_declared_manifest_is_the_hand_written_one_plus_the_boot_permission() {
    let original = original_manifest();
    let mut declared = parse_xml(&render().manifest);

    let boot = "android.permission.RECEIVE_BOOT_COMPLETED";
    let before = declared.children.len();
    declared.children.retain(|c| c.name() != Some(boot));
    assert_eq!(
        declared.children.len(),
        before - 1,
        "RECEIVE_BOOT_COMPLETED is added"
    );
    assert!(
        !original.children.iter().any(|c| c.name() == Some(boot)),
        "the template never had it"
    );

    assert_eq!(original.canonical(), declared.canonical());
}

#[test]
fn the_pieces_a_reader_would_look_for_are_there() {
    let r = render();
    let m = &r.manifest;
    for needle in [
        "android.permission.FOREGROUND_SERVICE_SPECIAL_USE",
        "android.permission.MODIFY_QUIET_MODE",
        "android.permission.QUERY_ALL_PACKAGES",
        "android.permission.RECEIVE_BOOT_COMPLETED",
        "android.app.action.PROFILE_PROVISIONING_COMPLETE",
        "android.app.action.ADMIN_POLICY_COMPLIANCE",
        "android.intent.action.LOCKED_BOOT_COMPLETED",
        "android.app.PROPERTY_SPECIAL_USE_FGS_SUBTYPE",
        "android:process=\":space\"",
        "android:process=\":g127\"",
    ] {
        assert!(m.contains(needle), "{needle}");
    }
    assert!(!m.contains(":g128"));
    assert_eq!(m.matches("<service").count(), 130);
}

#[test]
fn the_device_admin_policy_file_is_the_hand_written_one() {
    let r = render();
    let declared = parse_xml(&r.resources["xml/device_admin.xml"]);
    let original = parse_xml(
        &std::fs::read_to_string(fixtures().join("virtual-main-device-admin.xml")).unwrap(),
    );
    assert_eq!(original.canonical(), declared.canonical());
}

#[test]
fn the_build_warns_about_what_play_reviews() {
    let r = render();
    for what in ["QUERY_ALL_PACKAGES", "device admin", "specialUse"] {
        assert!(
            r.warnings.iter().any(|w| w.contains(what)),
            "{what}: {:?}",
            r.warnings
        );
    }
}
