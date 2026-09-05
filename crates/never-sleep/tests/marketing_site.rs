//! SEO and product-style contracts for the Cloudflare path canonical.
//!
//! Public origin is `https://xyz-ai.app/never-sleep/`, not GitHub Pages and
//! not the `never-sleep.xyz-ai.app` subdomain. The pages are static HTML so
//! crawlers see real copy (no JS-only body). Field names here are a
//! publishing contract: titles, hreflang, canonical URLs, and JSON-LD types
//! should stay stable once the site is live.

const SITE_ORIGIN: &str = "https://xyz-ai.app/never-sleep";

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
        format!("{SITE_ORIGIN}/")
    );
    assert_eq!(
        attr(&html, r#"rel="alternate" hreflang="en" href=""#),
        format!("{SITE_ORIGIN}/")
    );
    assert_eq!(
        attr(&html, r#"rel="alternate" hreflang="zh-Hans" href=""#),
        format!("{SITE_ORIGIN}/zh/")
    );
    assert_eq!(
        attr(&html, r#"rel="alternate" hreflang="x-default" href=""#),
        format!("{SITE_ORIGIN}/")
    );
    assert_eq!(
        attr(&html, r#"property="og:title" content=""#),
        "Never Sleep — Display off, Mac stays awake"
    );
    assert_eq!(attr(&html, r#"property="og:type" content=""#), "website");
    assert_eq!(
        attr(&html, r#"property="og:url" content=""#),
        format!("{SITE_ORIGIN}/")
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
        format!("{SITE_ORIGIN}/zh/")
    );
    assert_eq!(
        attr(&html, r#"rel="alternate" hreflang="en" href=""#),
        format!("{SITE_ORIGIN}/")
    );
    assert_eq!(
        attr(&html, r#"rel="alternate" hreflang="zh-Hans" href=""#),
        format!("{SITE_ORIGIN}/zh/")
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
        "marketing pages stay opaque; liquid glass lives in the AppKit panel"
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
fn copy_covers_everyday_use_cases_not_only_chatgpt() {
    let en_site = read("index.html");
    let zh_site = read("zh/index.html");
    let en_readme = fs::read_to_string(readme_root().join("README.md"))
        .unwrap_or_else(|err| panic!("missing README.md: {err}"));
    let zh_readme = fs::read_to_string(readme_root().join("README.zh-CN.md"))
        .unwrap_or_else(|err| panic!("missing README.zh-CN.md: {err}"));

    assert!(
        en_site.contains(r#"id="uses""#),
        "English page needs a use-cases section crawlers and nav can reach"
    );
    assert!(
        zh_site.contains(r#"id="uses""#),
        "Chinese page needs the matching 适用场景 section"
    );
    assert!(
        en_readme.contains("## What it's for"),
        "English README must name everyday jobs, not only ChatGPT / Codex"
    );
    assert!(
        zh_readme.contains("## 适用场景"),
        "Chinese README must name 适用场景"
    );

    let en_needles = [
        ("unattended downloads", "download"),
        ("Mac mini-style server", "mini"),
        ("protect the display", "protect"),
        ("lower idle power", "power"),
        ("long-running jobs", "compile"),
    ];
    for (label, needle) in en_needles {
        let hay = en_site.to_ascii_lowercase();
        assert!(
            hay.contains(needle),
            "English site should describe {label}; missing {needle:?}"
        );
        assert!(
            en_readme.to_ascii_lowercase().contains(needle),
            "English README should describe {label}; missing {needle:?}"
        );
    }
    for (label, needle) in [
        ("挂机下载", "下载"),
        ("当成迷你服务器", "服务器"),
        ("护屏", "护屏"),
        ("降低功耗", "功耗"),
        ("长时间任务", "编译"),
    ] {
        assert!(
            zh_site.contains(needle),
            "Chinese site should describe {label}; missing {needle:?}"
        );
        assert!(
            zh_readme.contains(needle),
            "Chinese README should describe {label}; missing {needle:?}"
        );
    }

    let description = attr(&en_site, r#"name="description" content=""#).to_ascii_lowercase();
    assert!(
        description.contains("download") || description.contains("server"),
        "meta description must sell the product job beyond ChatGPT, got: {description}"
    );
    let zh_description = attr(&zh_site, r#"name="description" content=""#);
    assert!(
        zh_description.contains("下载") || zh_description.contains("服务器"),
        "Chinese meta description must sell jobs beyond ChatGPT, got: {zh_description}"
    );
}

#[test]
fn homepage_explains_remote_access_and_phone_control() {
    let en = read("index.html");
    let zh = read("zh/index.html");

    for needle in [
        "Remote access and phone control",
        r#">Phone board</a>"#,
        r#"href="board/">phone board</a>"#,
        "multiple Macs",
        "start or end Screen-Off Standby",
        "will not force it off",
    ] {
        assert!(
            en.contains(needle),
            "English homepage should explain remote use; missing {needle:?}"
        );
    }

    for needle in [
        "远程连接与手机看板",
        r#">手机看板</a>"#,
        r#"href="board/">手机看板</a>"#,
        "多台 Mac",
        "远程开始或结束关屏待命",
        "不会强制关屏",
    ] {
        assert!(
            zh.contains(needle),
            "Chinese homepage should explain remote use; missing {needle:?}"
        );
    }
}

fn passages_around(hay: &str, needle: &str) -> Vec<String> {
    let lower = hay.to_ascii_lowercase();
    let needle_l = needle.to_ascii_lowercase();
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(rel) = lower[from..].find(&needle_l) {
        let abs = from + rel;
        out.push(passage_at(hay, abs));
        from = abs + needle.len().max(1);
    }
    out
}

fn passage_at(hay: &str, abs: usize) -> String {
    let before = &hay[..abs];
    if let Some(details) = before.rfind("<details") {
        if let Some(rel_end) = hay[details..].find("</details>") {
            let end = details + rel_end + "</details>".len();
            if end > abs {
                return hay[details..end].to_string();
            }
        }
    }
    if json_ld_still_open(before) {
        let start = before.rfind('{').unwrap_or(0);
        let end = char_boundary_at_or_before(hay, (abs + 520).min(hay.len()));
        return hay[start..end].to_string();
    }
    let start = before.rfind('\n').map(|i| i + 1).unwrap_or(0);
    let end = abs + hay[abs..].find('\n').unwrap_or(hay[abs..].len());
    hay[start..end].to_string()
}

fn json_ld_still_open(before: &str) -> bool {
    let open = before.rfind("application/ld+json");
    let close = before.rfind("</script>");
    match (open, close) {
        (Some(o), Some(c)) => o > c,
        (Some(_), None) => true,
        _ => false,
    }
}

fn char_boundary_at_or_before(s: &str, mut i: usize) -> usize {
    if i >= s.len() {
        return s.len();
    }
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

fn teams_passage_is_nongoal(passage: &str) -> bool {
    let lower = passage.to_ascii_lowercase();
    lower.contains("will not")
        || lower.contains("does not")
        || lower.contains("do not")
        || lower.contains("not a feature")
        || lower.contains("not keep")
        || lower.contains("won't")
        || passage.contains("不会")
        || passage.contains("不是")
        || passage.contains("不模拟")
        || passage.contains("不让")
}

fn powertoys_passage_claims_always_on_display(passage: &str) -> bool {
    let lower = passage.to_ascii_lowercase();
    let qualified = lower.contains("keep screen on")
        || lower.contains("optional")
        || passage.contains("可选项")
        || passage.contains("勾选");
    if qualified {
        return false;
    }
    lower.contains("never turn the display")
        || lower.contains("not to sleep the display")
        || passage.contains("也不关屏")
        || passage.contains("不要关屏")
}

#[test]
fn copy_names_keep_awake_pain_and_display_off_fix() {
    let en_site = read("index.html");
    let zh_site = read("zh/index.html");
    let en_readme = fs::read_to_string(readme_root().join("README.md"))
        .unwrap_or_else(|err| panic!("missing README.md: {err}"));
    let zh_readme = fs::read_to_string(readme_root().join("README.zh-CN.md"))
        .unwrap_or_else(|err| panic!("missing README.zh-CN.md: {err}"));

    assert!(
        en_site.contains(r#"id="why""#) && en_site.contains(r##"href="#why""##),
        "English nav must send Why to the keep-awake contrast, not skip it"
    );
    assert!(
        zh_site.contains(r#"id="why""#) && zh_site.contains(r##"href="#why""##),
        "Chinese nav must reach the 防休眠 contrast section"
    );

    for (label, hay) in [("English site", &en_site), ("English README", &en_readme)] {
        let lower = hay.to_ascii_lowercase();
        assert!(
            lower.contains("caffeine") && lower.contains("amphetamine"),
            "{label} must name the usual keep-awake tools"
        );
        assert!(
            lower.contains("burn-in") || lower.contains("oled"),
            "{label} must name the lit-panel cost (OLED burn-in)"
        );
        assert!(
            lower.contains("mouse"),
            "{label} must name mouse-jiggle keep-awake, got no mouse"
        );
        assert!(
            lower.contains("does not simulate")
                || lower.contains("do not simulate")
                || lower.contains("does not jiggle")
                || lower.contains("not simulate"),
            "{label} must say Never Sleep does not fake HID"
        );
        let teams = passages_around(hay, "Teams");
        assert!(
            !teams.is_empty(),
            "{label} should name Teams as a non-goal, not omit it"
        );
        for passage in &teams {
            assert!(
                teams_passage_is_nongoal(passage),
                "{label} Teams mention must negate the feature in that passage, got {passage:?}"
            );
        }
        for passage in passages_around(hay, "PowerToys") {
            assert!(
                !powertoys_passage_claims_always_on_display(&passage),
                "{label} must not say PowerToys Awake always keeps the display on, got {passage:?}"
            );
        }
    }

    for (label, hay) in [("Chinese site", &zh_site), ("Chinese README", &zh_readme)] {
        assert!(
            hay.contains("Caffeine") && hay.contains("Amphetamine"),
            "{label} must name the usual keep-awake tools"
        );
        assert!(
            hay.contains("烧屏"),
            "{label} must name 烧屏 as the cost of leaving the panel on"
        );
        assert!(
            hay.contains("鼠标"),
            "{label} must name 模拟鼠标 keep-awake"
        );
        assert!(
            hay.contains("不模拟"),
            "{label} must say Never Sleep 不模拟键鼠"
        );
        let teams = passages_around(hay, "Teams");
        assert!(
            !teams.is_empty(),
            "{label} should name Teams as a non-goal, not omit it"
        );
        for passage in &teams {
            assert!(
                teams_passage_is_nongoal(passage),
                "{label} Teams mention must negate the feature in that passage, got {passage:?}"
            );
        }
        for passage in passages_around(hay, "PowerToys") {
            assert!(
                !powertoys_passage_claims_always_on_display(&passage),
                "{label} must not say PowerToys Awake always keeps the display on, got {passage:?}"
            );
        }
    }

    let en_faq = en_site
        .split(r#"id="faq""#)
        .nth(1)
        .unwrap_or_else(|| panic!("English FAQ section"));
    assert!(
        en_faq.to_ascii_lowercase().contains("caffeine"),
        "English FAQ must answer how this differs from Caffeine-class tools"
    );
    let zh_faq = zh_site
        .split(r#"id="faq""#)
        .nth(1)
        .unwrap_or_else(|| panic!("Chinese FAQ section"));
    assert!(
        zh_faq.contains("Caffeine") || zh_faq.contains("Amphetamine") || zh_faq.contains("防休眠"),
        "Chinese FAQ must answer how this differs from 防休眠 tools"
    );
}

#[test]
fn sitemap_and_robots_point_at_xyz_ai_path() {
    let robots = read("robots.txt");
    assert!(robots.contains("User-agent: *"));
    assert!(robots.contains("Allow: /"));
    assert!(
        robots.contains(&format!("Sitemap: {SITE_ORIGIN}/sitemap.xml")),
        "robots.txt must advertise the sitemap"
    );
    let sitemap = read("sitemap.xml");
    for loc in [format!("{SITE_ORIGIN}/"), format!("{SITE_ORIGIN}/zh/")] {
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
        "assets/board.js",
        "board/index.html",
        "zh/board/index.html",
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
fn workspace_crate_version_matches_info_plist() {
    let version = shipping_app_version();
    let cargo = fs::read_to_string(readme_root().join("Cargo.toml"))
        .expect("workspace Cargo.toml must exist");
    let workspace = cargo
        .split("[workspace.package]")
        .nth(1)
        .expect("workspace.package");
    let cargo_version = workspace
        .lines()
        .find_map(|line| {
            line.trim()
                .strip_prefix("version = \"")
                .and_then(|rest| rest.strip_suffix('"'))
        })
        .expect("workspace.package version");
    assert_eq!(
        cargo_version, version,
        "Cargo.toml and Info.plist must ship the same version"
    );
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
        manifest.contains(r#""start_url": "/never-sleep/""#),
        "PWA start_url must be the xyz-ai.app/never-sleep path, got {manifest}"
    );
    for name in ["README.md", "README.zh-CN.md"] {
        let text = fs::read_to_string(readme_root().join(name))
            .unwrap_or_else(|err| panic!("missing {name}: {err}"));
        assert!(
            text.contains(SITE_ORIGIN),
            "{name} must link the Cloudflare path canonical {SITE_ORIGIN}"
        );
        assert!(
            !text.contains("github.io"),
            "{name} must not keep the GitHub Pages URL"
        );
    }
}

#[test]
fn public_site_urls_do_not_mention_github_pages() {
    for rel in [
        "index.html",
        "zh/index.html",
        "404.html",
        "robots.txt",
        "sitemap.xml",
        "manifest.webmanifest",
    ] {
        let body = read(rel);
        assert!(
            !body.contains("seasonxue.github.io"),
            "{rel} still advertises GitHub Pages as a public URL"
        );
        assert!(
            !body.contains("never-sleep.xyz-ai.app"),
            "{rel} still advertises the retired subdomain as a public URL"
        );
        // GitHub repo/release URLs keep `mac-never-sleep` in the path; those
        // are not the GitHub Pages project prefix.
        let without_repo = body.replace("https://github.com/SeasonXue/mac-never-sleep", "");
        assert!(
            !without_repo.contains("/mac-never-sleep/"),
            "{rel} still uses the GitHub Pages path prefix"
        );
    }
}

#[test]
fn json_ld_urls_use_the_xyz_ai_canonical() {
    for (rel, page) in [
        ("index.html", format!("{SITE_ORIGIN}/")),
        ("zh/index.html", format!("{SITE_ORIGIN}/zh/")),
    ] {
        let graph = json_ld(&read(rel));
        let nodes: Vec<&Value> = match graph.get("@graph") {
            Some(Value::Array(items)) => items.iter().collect(),
            _ => vec![&graph],
        };
        for node in &nodes {
            if let Some(url) = node.get("url").and_then(Value::as_str) {
                if url.starts_with("https://github.com/") {
                    continue;
                }
                assert!(
                    url.starts_with(SITE_ORIGIN),
                    "{rel} JSON-LD url must be on the xyz-ai path, got {url}"
                );
            }
            for key in ["image", "screenshot"] {
                if let Some(url) = node.get(key).and_then(Value::as_str) {
                    assert!(
                        url.starts_with(&format!("{SITE_ORIGIN}/assets/")),
                        "{rel} JSON-LD {key} must be an absolute xyz-ai asset, got {url}"
                    );
                }
            }
        }
        let app = nodes
            .iter()
            .find(|node| node.get("@type").and_then(Value::as_str) == Some("SoftwareApplication"))
            .unwrap_or_else(|| panic!("{rel} needs SoftwareApplication JSON-LD"));
        assert_eq!(app.get("url").and_then(Value::as_str), Some(page.as_str()));
    }
}

#[test]
fn absolute_on_host_paths_use_the_never_sleep_prefix() {
    let html = read("404.html");
    assert!(html.contains("href=\"/never-sleep/\""));
    assert!(html.contains("href=\"/never-sleep/zh/\""));
    assert!(html.contains("href=\"/never-sleep/assets/style.css\""));
    assert!(!html.contains("/mac-never-sleep/"));

    let manifest = read("manifest.webmanifest");
    assert!(manifest.contains("\"start_url\": \"/never-sleep/\""));
    assert!(manifest.contains("\"scope\": \"/never-sleep/\""));
    assert!(manifest.contains("/never-sleep/assets/app-icon.png"));
    assert!(!manifest.contains("/mac-never-sleep/"));
}

#[test]
fn readme_website_links_use_the_xyz_ai_canonical() {
    let en = fs::read_to_string(readme_root().join("README.md")).unwrap();
    let zh = fs::read_to_string(readme_root().join("README.zh-CN.md")).unwrap();
    assert!(en.contains(&format!("{SITE_ORIGIN}/")));
    assert!(zh.contains(&format!("{SITE_ORIGIN}/zh/")));
    assert!(!en.contains("seasonxue.github.io"));
    assert!(!zh.contains("seasonxue.github.io"));
    assert!(!en.contains("never-sleep.xyz-ai.app"));
    assert!(!zh.contains("never-sleep.xyz-ai.app"));
}
