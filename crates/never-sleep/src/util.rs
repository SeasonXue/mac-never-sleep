#[cfg(any(test, target_os = "macos"))]
pub fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

/// True when a LaunchAgent still points at the pre-rename Chinese bundle,
/// or at any path other than the currently running app/binary.
#[cfg(any(test, target_os = "macos"))]
pub fn launch_agent_is_stale(plist: &str, current_target: &str) -> bool {
    plist.contains("熄屏待命.app") || !plist.contains(current_target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_xml_metacharacters() {
        assert_eq!(
            xml_escape(r#"C:\A & B <app>.app"#),
            r#"C:\A &amp; B &lt;app&gt;.app"#
        );
        assert_eq!(
            xml_escape(r#"say "hi" & 'bye'"#),
            r#"say &quot;hi&quot; &amp; &apos;bye&apos;"#
        );
    }

    #[test]
    fn launch_agent_stale_after_bundle_rename() {
        let old = r#"<string>/Applications/熄屏待命.app</string>"#;
        assert!(launch_agent_is_stale(old, "/Applications/Never Sleep.app"));
        let current = r#"<string>/Applications/Never Sleep.app</string>"#;
        assert!(!launch_agent_is_stale(
            current,
            "/Applications/Never Sleep.app"
        ));
        let elsewhere = r#"<string>/tmp/never-sleep</string>"#;
        assert!(launch_agent_is_stale(
            elsewhere,
            "/Applications/Never Sleep.app"
        ));
    }
}
