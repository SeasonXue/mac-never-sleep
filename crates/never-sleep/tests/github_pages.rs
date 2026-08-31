//! SEO and product-style contracts for the GitHub Pages marketing site.
//!
//! The pages are static HTML so crawlers see real copy (no JS-only body).
//! Field names here are a publishing contract: titles, hreflang, canonical
//! URLs, and JSON-LD types should stay stable once the site is live.

use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

fn site_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../site")
}

fn read(rel: &str) -> String {
    let path = site_root().join(rel);
    fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("missing {rel} at {}: {err}", path.display()))
}

fn attr<'a>(html: &'a str, needle: &str) -> &'a str {
    html.split(needle)
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .unwrap_or_else(|| panic!("expected {needle} in HTML"))
}

fn json_ld(html: &str) -> Value {
    let block = html
        .split(r#"<script type="application/ld+json">"#)
        .nth(1)
        .and_then(|rest| rest.split("</script>").next())
        .expect("JSON-LD script is required for rich results");
    serde_json::from_str(block.trim()).expect("JSON-LD must be valid JSON")
}

#[test]
fn english_homepage_has_seo_contract() {
    let html = read("index.html");
    assert!(
        html.contains("<html lang=\"en\""),
        "English page must declare lang=en for crawlers"
    );
    assert!(
        html.contains("<title>Never Sleep — Display off, Mac stays awake</title>"),
        "title is the primary SERP headline"
    );
    let description = attr(&html, r#"name="description" content=""#);
    assert!(
        description.contains("display off") && description.contains("Mac"),
        "meta description should name the product job, got: {description}"
    );
    assert_eq!(
        attr(&html, r#"rel="canonical" href=""#),
        "https://seasonxue.github.io/mac-never-sleep/"
    );
    assert_eq!(
        attr(&html, r#"rel="alternate" hreflang="en" href=""#),
        "https://seasonxue.github.io/mac-never-sleep/"
    );
    assert_eq!(
        attr(&html, r#"rel="alternate" hreflang="zh-Hans" href=""#),
        "https://seasonxue.github.io/mac-never-sleep/zh/"
    );
    assert_eq!(
        attr(&html, r#"rel="alternate" hreflang="x-default" href=""#),
        "https://seasonxue.github.io/mac-never-sleep/"
    );
    assert_eq!(
        attr(&html, r#"property="og:title" content=""#),
        "Never Sleep — Display off, Mac stays awake"
    );
    assert_eq!(attr(&html, r#"property="og:type" content=""#), "website");
    assert_eq!(
        attr(&html, r#"property="og:url" content=""#),
        "https://seasonxue.github.io/mac-never-sleep/"
    );
    assert!(
        attr(&html, r#"property="og:image" content=""#).ends_with("/og.png"),
        "Open Graph image must be an absolute PNG"
    );
    assert_eq!(
        attr(&html, r#"name="twitter:card" content=""#),
        "summary_large_image"
    );
    assert!(
        html.contains(r#"name="robots" content="index, follow""#),
        "allow indexing"
    );
}

#[test]
fn chinese_homepage_mirrors_hreflang_and_uses_product_name() {
    let html = read("zh/index.html");
    assert!(
        html.contains("<html lang=\"zh-Hans\""),
        "Chinese page must use zh-Hans, matching the app localization"
    );
    assert!(
        html.contains("<title>熄屏待命 — 屏幕关掉，电脑不睡</title>"),
        "Chinese title should use the Finder display name"
    );
    assert_eq!(
        attr(&html, r#"rel="canonical" href=""#),
        "https://seasonxue.github.io/mac-never-sleep/zh/"
    );
    assert_eq!(
        attr(&html, r#"rel="alternate" hreflang="en" href=""#),
        "https://seasonxue.github.io/mac-never-sleep/"
    );
    assert_eq!(
        attr(&html, r#"rel="alternate" hreflang="zh-Hans" href=""#),
        "https://seasonxue.github.io/mac-never-sleep/zh/"
    );
    assert!(
        html.contains("合盖") && html.contains("尽力而为"),
        "Chinese copy must keep the closed-lid caveat from the product UI"
    );
}

#[test]
fn json_ld_describes_a_free_macos_utility() {
    let html = read("index.html");
    let graph = json_ld(&html);
    let nodes: Vec<&Value> = match graph.get("@graph") {
        Some(Value::Array(items)) => items.iter().collect(),
        _ => vec![&graph],
    };
    let app = nodes
        .iter()
        .find(|node| node.get("@type").and_then(Value::as_str) == Some("SoftwareApplication"))
        .expect("SoftwareApplication node");
    assert_eq!(app.get("name").and_then(Value::as_str), Some("Never Sleep"));
    assert_eq!(
        app.get("alternateName").and_then(Value::as_str),
        Some("熄屏待命")
    );
    assert_eq!(
        app.get("operatingSystem").and_then(Value::as_str),
        Some("macOS 12+")
    );
    assert_eq!(
        app.get("applicationCategory").and_then(Value::as_str),
        Some("UtilitiesApplication")
    );
    let offers = app.get("offers").expect("offers");
    assert_eq!(offers.get("price").and_then(Value::as_str), Some("0"));
    assert!(
        nodes
            .iter()
            .any(|node| node.get("@type").and_then(Value::as_str) == Some("FAQPage")),
        "FAQPage JSON-LD helps sitelinks; crawlers need it in the HTML"
    );
}

#[test]
fn pages_reuse_popover_color_tokens_and_system_fonts() {
    let css = read("assets/style.css");
    for token in ["#f5f5f7", "#1c1c1e", "#007aff", "#34c759"] {
        assert!(
            css.contains(token),
            "site CSS must reuse popover token {token}"
        );
    }
    assert!(
        css.contains("-apple-system") && css.contains("SF Pro Text"),
        "marketing site should use the same system font stack as the menu bar"
    );
    assert!(
        !css.contains("backdrop-filter"),
        "product UI dropped frosted glass; do not bring it back on the site"
    );
}

#[test]
fn english_copy_keeps_product_invariants() {
    let html = read("index.html");
    assert!(
        html.contains("best effort") || html.contains("best-effort"),
        "closed-lid stay-awake must stay qualified"
    );
    assert!(html.contains("⌥⌘P"), "always provide a way back");
    assert!(
        html.contains("never-sleep on --for 8h"),
        "agent contract snippet must stay on the site"
    );
    assert!(
        !html.to_ascii_lowercase().contains("pmset disablesleep"),
        "must not advertise rewriting Energy Saver"
    );
}

#[test]
fn sitemap_and_robots_point_at_github_pages() {
    let robots = read("robots.txt");
    assert!(robots.contains("User-agent: *"));
    assert!(robots.contains("Allow: /"));
    assert!(
        robots.contains("Sitemap: https://seasonxue.github.io/mac-never-sleep/sitemap.xml"),
        "robots.txt must advertise the sitemap"
    );
    let sitemap = read("sitemap.xml");
    for loc in [
        "https://seasonxue.github.io/mac-never-sleep/",
        "https://seasonxue.github.io/mac-never-sleep/zh/",
    ] {
        assert!(sitemap.contains(loc), "sitemap missing {loc}");
    }
    assert!(
        sitemap.contains("hreflang=\"en\"") && sitemap.contains("hreflang=\"zh-Hans\""),
        "xhtml:link hreflang in the sitemap helps Google pair the two locales"
    );
}

#[test]
fn required_static_assets_exist() {
    let root = site_root();
    for rel in [
        ".nojekyll",
        "404.html",
        "manifest.webmanifest",
        "assets/sun.png",
        "assets/moon.png",
        "assets/app-icon.png",
        "assets/og.png",
        "assets/favicon.png",
        "assets/apple-touch-icon.png",
        "assets/screenshots/main-idle-en.png",
        "assets/screenshots/main-active-en.png",
        "assets/screenshots/main-idle-zh.png",
        "assets/screenshots/main-active-zh.png",
        "assets/site.js",
    ] {
        let path = root.join(rel);
        assert!(path.is_file(), "missing Pages asset {}", path.display());
    }
}

#[test]
fn download_cta_points_at_the_github_release() {
    let html = read("index.html");
    assert!(
        html.contains("https://github.com/SeasonXue/mac-never-sleep/releases/latest"),
        "primary download should follow the latest GitHub Release"
    );
    assert!(
        Path::new(&site_root()).join("assets/style.css").is_file(),
        "CSS is a separate file so HTML stays crawlable and cacheable"
    );
}
