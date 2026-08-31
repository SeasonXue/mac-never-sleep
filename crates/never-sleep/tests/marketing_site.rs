//! SEO and product-style contracts for the Cloudflare marketing site.
//!
//! The pages are static HTML so crawlers see real copy (no JS-only body).
//! Field names here are a publishing contract: titles, hreflang, canonical
//! URLs, and JSON-LD types should stay stable once the site is live.

const SITE: &str = "https://never-sleep.xyz-ai.app";

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
        "https://never-sleep.xyz-ai.app/"
    );
    assert_eq!(
        attr(&html, r#"rel="alternate" hreflang="en" href=""#),
        "https://never-sleep.xyz-ai.app/"
    );
    assert_eq!(
        attr(&html, r#"rel="alternate" hreflang="zh-Hans" href=""#),
        "https://never-sleep.xyz-ai.app/zh/"
    );
    assert_eq!(
        attr(&html, r#"rel="alternate" hreflang="x-default" href=""#),
        "https://never-sleep.xyz-ai.app/"
    );
    assert_eq!(
        attr(&html, r#"property="og:title" content=""#),
        "Never Sleep — Display off, Mac stays awake"
    );
    assert_eq!(attr(&html, r#"property="og:type" content=""#), "website");
    assert_eq!(
        attr(&html, r#"property="og:url" content=""#),
        "https://never-sleep.xyz-ai.app/"
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
        html.contains("<title>Never Sleep — 屏幕关掉，电脑不睡</title>"),
        "Chinese title should use the Never Sleep product name"
    );
    assert_eq!(
        attr(&html, r#"rel="canonical" href=""#),
        "https://never-sleep.xyz-ai.app/zh/"
    );
    assert_eq!(
        attr(&html, r#"rel="alternate" hreflang="en" href=""#),
        "https://never-sleep.xyz-ai.app/"
    );
    assert_eq!(
        attr(&html, r#"rel="alternate" hreflang="zh-Hans" href=""#),
        "https://never-sleep.xyz-ai.app/zh/"
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
    assert!(
        app.get("alternateName").is_none(),
        "Never Sleep is the only product name; do not advertise 熄屏待命 as an alternateName"
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
fn sitemap_and_robots_point_at_the_product_origin() {
    let robots = read("robots.txt");
    assert!(robots.contains("User-agent: *"));
    assert!(robots.contains("Allow: /"));
    assert!(
        robots.contains(&format!("Sitemap: {SITE}/sitemap.xml")),
        "robots.txt must advertise the sitemap"
    );
    let sitemap = read("sitemap.xml");
    for loc in [format!("{SITE}/"), format!("{SITE}/zh/")] {
        assert!(sitemap.contains(&loc), "sitemap missing {loc}");
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
        assert!(path.is_file(), "missing site asset {}", path.display());
    }
}

fn readme_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn css_rule<'a>(css: &'a str, selector: &str) -> &'a str {
    let needle = format!("{selector} {{");
    css.split(&needle)
        .nth(1)
        .and_then(|rest| rest.split('}').next())
        .unwrap_or_else(|| panic!("expected {selector} rule"))
}

fn png_ihdr_color_type(path: &Path) -> u8 {
    let data = fs::read(path).unwrap_or_else(|err| panic!("{}: {err}", path.display()));
    assert!(
        data.starts_with(b"\x89PNG\r\n\x1a\n"),
        "{} is not a PNG",
        path.display()
    );
    assert_eq!(&data[12..16], b"IHDR", "{} missing IHDR", path.display());
    data[25]
}

#[test]
fn how_to_section_labels_share_the_card_column() {
    let css = read("assets/style.css");
    let help = css_rule(&css, ".help-section");
    let steps = css_rule(&css, ".steps");
    assert!(
        help.contains("max-width: 640px"),
        "Get started / Keep in mind labels must use the same column as the cards, got {help:?}"
    );
    assert!(
        steps.contains("max-width: 640px"),
        "step cards stay in a 640px column, got {steps:?}"
    );
    assert!(
        help.contains("margin: 0 auto") || help.contains("margin-left: auto"),
        "labels must center with the cards instead of hugging the left of the wrap, got {help:?}"
    );
}

#[test]
fn marketing_screenshots_keep_transparent_rounded_corners() {
    let names = [
        "howto-en.png",
        "howto-zh.png",
        "main-active-en.png",
        "main-active-zh.png",
        "main-idle-en.png",
        "main-idle-zh.png",
        "settings-en.png",
        "settings-zh.png",
    ];
    for dir in [
        site_root().join("assets/screenshots"),
        readme_root().join("docs/screenshots"),
    ] {
        for name in names {
            let path = dir.join(name);
            let color = png_ihdr_color_type(&path);
            assert_eq!(
                color, 6,
                "{} must be RGBA so rounded-corner leftover pixels stay transparent on the gallery cards, got color type {color}",
                path.display()
            );
        }
    }
}

#[test]
fn hero_shots_top_align_when_captions_wrap() {
    let css = read("assets/style.css");
    let shots = css_rule(&css, ".shots");
    assert!(
        shots.contains("align-items: start"),
        "hero shots must top-align the panels; wrapping figcaptions must not lift one card, got {shots:?}"
    );
    assert!(
        !shots.contains("align-items: end"),
        "align-items: end lines cards to the caption baseline and breaks when copy wraps, got {shots:?}"
    );
    let gallery = css_rule(&css, ".gallery");
    assert!(
        gallery.contains("align-items: start"),
        "gallery tiles must top-align so wrapping captions cannot shift the screenshots, got {gallery:?}"
    );
}

#[test]
fn sun_coin_uses_idle_white_surface() {
    let css = read("assets/style.css");
    let sun = css_rule(&css, ".coin.sun");
    assert!(
        sun.contains("#ffffff") || sun.contains("#fff"),
        "the idle sun coin must use the popover white surface, got {sun:?}"
    );
    for rel in ["index.html", "zh/index.html"] {
        let html = read(rel);
        assert!(
            html.contains(r#"class="coin sun""#),
            "{rel} must mark the left celestial coin as the white-backed sun"
        );
        assert!(
            html.contains(r#"class="coin moon""#),
            "{rel} must keep the moon on the dark active surface"
        );
    }
}

#[test]
fn readme_screenshot_tables_keep_images_top_aligned() {
    for name in ["README.md", "README.zh-CN.md"] {
        let text = fs::read_to_string(readme_root().join(name))
            .unwrap_or_else(|err| panic!("missing {name}: {err}"));
        let table = text
            .split("<table>")
            .nth(1)
            .and_then(|rest| rest.split("</table>").next())
            .unwrap_or_else(|| panic!("{name} must keep the screenshot table"));
        let tds = table.matches("<td").count();
        let top = table.matches("valign=\"top\"").count();
        assert_eq!(
            tds, top,
            "{name}: every screenshot cell must valign=top so wrapping captions cannot shift the images ({tds} td, {top} valign=top)"
        );
        for cell in table.split("<td").skip(1) {
            let cell = cell.split("</td>").next().expect("td must close");
            assert!(
                !(cell.contains("<img") && cell.contains("<sub")),
                "{name}: keep each screenshot and its caption in separate rows so wrapping cannot move the images"
            );
        }
    }
}

#[test]
fn pages_brand_never_sleep_in_both_languages() {
    for rel in ["index.html", "zh/index.html", "404.html"] {
        let html = read(rel);
        assert!(
            !html.contains("熄屏待命"),
            "{rel} must brand the product as Never Sleep, not 熄屏待命"
        );
    }
    for name in ["README.md", "README.zh-CN.md"] {
        let text = fs::read_to_string(readme_root().join(name))
            .unwrap_or_else(|err| panic!("missing {name}: {err}"));
        assert!(
            !text.contains("熄屏待命"),
            "{name} must brand the product as Never Sleep, not 熄屏待命"
        );
    }
}

fn shipping_app_version() -> String {
    let plist = fs::read_to_string(readme_root().join("packaging/Info.plist"))
        .expect("packaging/Info.plist must exist");
    plist
        .split("<key>CFBundleShortVersionString</key>")
        .nth(1)
        .and_then(|rest| rest.split("<string>").nth(1))
        .and_then(|rest| rest.split("</string>").next())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| panic!("CFBundleShortVersionString missing from Info.plist"))
        .to_string()
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

#[test]
fn pages_do_not_advertise_building_from_source() {
    for rel in ["index.html", "zh/index.html"] {
        let html = read(rel);
        for needle in [
            "cargo build",
            "package-macos.sh",
            "build it on a Mac",
            "自行编译",
        ] {
            assert!(
                !html.contains(needle),
                "{rel} must send people to the GitHub Release, not a source build; found {needle:?}"
            );
        }
    }
}

#[test]
fn pages_show_the_shipping_app_version() {
    let version = shipping_app_version();
    assert!(
        version.chars().all(|ch| ch.is_ascii_digit() || ch == '.'),
        "CFBundleShortVersionString should be a dotted version, got {version:?}"
    );
    for rel in ["index.html", "zh/index.html"] {
        let html = read(rel);
        assert!(
            html.contains(&format!("v{version}")),
            "{rel} must show the shipping version as v{version}"
        );
        assert!(
            html.contains(r#"class="release-ver""#),
            "{rel} download card must surface the version, not hide it in JSON-LD"
        );
        let graph = json_ld(&html);
        let nodes: Vec<&Value> = match graph.get("@graph") {
            Some(Value::Array(items)) => items.iter().collect(),
            _ => vec![&graph],
        };
        let app = nodes
            .iter()
            .find(|node| node.get("@type").and_then(Value::as_str) == Some("SoftwareApplication"))
            .unwrap_or_else(|| panic!("{rel} needs SoftwareApplication JSON-LD"));
        assert_eq!(
            app.get("softwareVersion").and_then(Value::as_str),
            Some(version.as_str()),
            "{rel} JSON-LD softwareVersion must match Info.plist, got {:?}",
            app.get("softwareVersion")
        );
    }
}

#[test]
fn download_steps_center_as_a_shrink_wrapped_block() {
    let css = read("assets/style.css");
    let ol = css_rule(&css, ".get ol");
    assert!(
        ol.contains("width: fit-content") || ol.contains("width: max-content"),
        "the numbered install steps must shrink to the text so the block can sit in the middle of the card, got {ol:?}"
    );
    assert!(
        ol.contains("margin:") && ol.contains("auto"),
        "the steps list must be centered as a block, not hug the left padding, got {ol:?}"
    );
    assert!(
        !ol.contains("max-width: 520px"),
        "a 520px left-aligned column is what made the list look left-heavy, got {ol:?}"
    );
}

#[test]
fn pages_bind_visible_version_to_latest_github_release() {
    let js = read("assets/site.js");
    assert!(
        js.contains("api.github.com/repos/SeasonXue/mac-never-sleep/releases/latest"),
        "the marketing page must read the latest GitHub Release tag instead of freezing a version in HTML"
    );
    assert!(js.contains("tag_name"), "latest-release JSON uses tag_name");
    for rel in ["index.html", "zh/index.html"] {
        let html = read(rel);
        assert!(
            html.contains("data-release-tag"),
            "{rel} visible version labels must be live-updated from GitHub Releases"
        );
        assert!(
            html.contains(r#"class="release-ver""#),
            "{rel} download card still shows a version even before JS runs"
        );
    }
}

#[test]
fn latest_release_label_keeps_workflow_tag_names() {
    let js = read("assets/site.js");
    let func = js
        .split("function latestReleaseLabel")
        .nth(1)
        .and_then(|rest| rest.split("function ").next())
        .expect("latestReleaseLabel must exist so the page can show GitHub tag_name");
    assert!(
        !func.contains(".match("),
        "do not extract a dotted version from tag_name; release.yml accepts arbitrary tags such as release/v1 and release#1, got {func}"
    );
    assert!(
        func.contains(".trim()"),
        "still strip surrounding whitespace from tag_name, got {func}"
    );
    for tag in ["release/v1", "release#1"] {
        assert!(
            js.contains(tag),
            "site.js must name {tag} as a valid GitHub Release tag so the dotted-version regex cannot come back"
        );
    }
}

#[test]
fn github_pages_deploy_is_removed() {
    let root = readme_root();
    assert!(
        !root.join(".github/workflows/pages.yml").exists(),
        "Cloudflare hosts the site; do not keep a GitHub Pages deploy workflow"
    );
    assert!(
        !site_root().join(".nojekyll").exists(),
        ".nojekyll is a GitHub Pages Jekyll switch; Cloudflare does not need it"
    );
}

#[test]
fn site_is_served_from_the_custom_domain_root() {
    for rel in [
        "index.html",
        "zh/index.html",
        "404.html",
        "robots.txt",
        "sitemap.xml",
        "manifest.webmanifest",
    ] {
        let text = read(rel);
        assert!(
            !text.contains("github.io"),
            "{rel} must not advertise the retired GitHub Pages host"
        );
    }
    for rel in ["404.html", "manifest.webmanifest"] {
        let text = read(rel);
        assert!(
            !text.contains("/mac-never-sleep/"),
            "{rel} must not use the GitHub Pages project-site prefix"
        );
    }
    let manifest = read("manifest.webmanifest");
    assert!(
        manifest.contains(r#""start_url": "/""#),
        "PWA start_url must be the domain root on Cloudflare, got {manifest}"
    );
    for name in ["README.md", "README.zh-CN.md"] {
        let text = fs::read_to_string(readme_root().join(name))
            .unwrap_or_else(|err| panic!("missing {name}: {err}"));
        assert!(
            text.contains(SITE),
            "{name} must link the Cloudflare origin {SITE}"
        );
        assert!(
            !text.contains("github.io"),
            "{name} must not keep the GitHub Pages URL"
        );
    }
}
