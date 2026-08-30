use never_sleep_core::Lang;

/// Resolve UI language: env override, then macOS preferred languages, then Unix locale.
/// Falls back to English.
pub fn detect() -> Lang {
    if let Some(lang) = Lang::from_override_env() {
        return lang;
    }
    #[cfg(target_os = "macos")]
    if let Some(lang) = from_apple_languages() {
        return lang;
    }
    Lang::from_unix_locale()
}

#[cfg(target_os = "macos")]
fn from_apple_languages() -> Option<Lang> {
    let output = std::process::Command::new("defaults")
        .args(["read", "-g", "AppleLanguages"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let tags = parse_quoted_tokens(&text);
    Lang::from_preferred_tags(&tags)
}

#[cfg(any(test, target_os = "macos"))]
fn parse_quoted_tokens(text: &str) -> Vec<String> {
    let mut tags = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find('"') {
        rest = &rest[start + 1..];
        if let Some(end) = rest.find('"') {
            let token = &rest[..end];
            if !token.is_empty() {
                tags.push(token.to_string());
            }
            rest = &rest[end + 1..];
        } else {
            break;
        }
    }
    tags
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_apple_languages_plist() {
        let raw = r#"
(
    "zh-Hans-CN",
    "en-US"
)
"#;
        let tags = parse_quoted_tokens(raw);
        assert_eq!(tags, vec!["zh-Hans-CN", "en-US"]);
        assert_eq!(Lang::from_preferred_tags(&tags), Some(Lang::Zh));
    }
}
