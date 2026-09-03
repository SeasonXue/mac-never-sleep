//! Phone board pages and the Cloudflare Worker that hosts them.
//!
//! Public origin is `https://xyz-ai.app/never-sleep/`. The gateway already
//! binds Worker `mac-never-sleep` and strips `/never-sleep`, so API paths on
//! the Worker are `/api/...` while the phone fetches `/never-sleep/api/...`.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

const SITE_ORIGIN: &str = "https://xyz-ai.app/never-sleep";

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(rel: &str) -> String {
    let path = root().join(rel);
    fs::read_to_string(&path).unwrap_or_else(|err| panic!("missing {rel}: {err}"))
}

#[test]
fn wrangler_worker_is_the_existing_mac_never_sleep_service() {
    let wrangler = read("wrangler.jsonc");
    assert!(
        wrangler.contains(r#""name": "mac-never-sleep""#),
        "must keep the gateway Service Binding name"
    );
    assert!(wrangler.contains(r#""directory": "./site""#));
    assert!(wrangler.contains("run_worker_first"));
    assert!(wrangler.contains("/api/*"));
    assert!(wrangler.contains("BoardHub"));
    assert!(
        !wrangler.contains("custom_domain"),
        "custom domain stays on the gateway"
    );
}

#[test]
fn board_pages_are_mobile_first_and_bilingual() {
    let en = read("site/board/index.html");
    let zh = read("site/zh/board/index.html");
    assert!(en.contains("<html lang=\"en\""));
    assert!(zh.contains("<html lang=\"zh-Hans\""));
    assert!(en.contains("Start Screen-Off Standby") || en.contains("board.js"));
    assert!(en.contains("noindex"));
    assert!(zh.contains("noindex"));
    assert_eq!(
        en.split(r#"rel="canonical" href=""#)
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap(),
        format!("{SITE_ORIGIN}/board/")
    );
    assert!(en.contains(r#"href="../assets/style.css""#));
    assert!(zh.contains(r#"href="../../assets/style.css""#));
    assert!(en.contains("Start or End Screen-Off Standby") || en.contains("will not fight"));
    assert!(zh.contains("开始") && zh.contains("结束关屏待命"));
}

#[test]
fn board_client_uses_public_prefix_and_has_no_unauthenticated_toggle() {
    let js = read("site/assets/board.js");
    assert!(js.contains("/never-sleep/api"));
    assert!(js.contains("function apiBase"));
    assert!(js.contains("Start Screen-Off Standby"));
    assert!(js.contains("开始关屏待命"));
    assert!(js.contains("End Standby"));
    assert!(js.contains("结束待命"));
    assert!(js.contains("device_token"));
    assert!(js.contains("/command"));
    assert!(!js.contains("cmd: \"toggle\""));
    assert!(!js.contains("\"cmd\":\"quit\""));
    assert!(
        !js.contains("innerHTML"),
        "untrusted heartbeat fields must not go through innerHTML"
    );
    assert!(js.contains("textContent"));
    assert!(js.contains("networkError"));
    assert!(
        !js.contains("st.online && st.active"),
        "offline Macs must keep last-known standby, not paint Standby off"
    );
    assert!(js.contains("st.active ? copy.standbyOn : copy.standbyOff"));
    assert!(
        js.contains("function withListFailure"),
        "list transport failures must mark cached devices offline"
    );
    assert!(js.contains("withListFailure(lastStatuses)"));
    assert!(js.contains("LIST_MAX_DEVICES = 32"));
    assert!(js.contains("while (devices.length > LIST_MAX_DEVICES)"));
    assert!(js.contains("devices.shift()"));
    assert!(js.contains("function hrefWithPairingCode"));
    assert!(js.contains("waitForClaimThenLanguageHref"));
    assert!(js.contains("preventDefault"));
    assert!(js.contains("clearPairingQuery"));
    assert!(js.contains("history.replaceState"));
    assert!(js.contains("function mergeListStatuses"));
    assert!(js.contains("claimInFlight"));
    assert!(js.contains("function devicePendingCmd"));
    assert!(js.contains("pendingByDevice"));
    assert!(js.contains("function nextRefreshGeneration"));
    assert!(js.contains("isCurrentRefresh"));
    assert!(js.contains("refreshInFlight"));
    assert!(js.contains("refreshQueued"));
}

#[test]
fn worker_board_logic_is_tested_in_node() {
    let status = Command::new("node")
        .args(["--test", "worker/test/board.test.js"])
        .current_dir(root())
        .status()
        .expect("node --test");
    assert!(status.success(), "worker/test/board.test.js must pass");
}

#[test]
fn worker_shards_per_device_not_one_global_board() {
    let index = read("worker/src/index.js");
    assert!(
        !index.contains("idFromName(\"board\")"),
        "a single named DO serializes every home onto one queue"
    );
    assert!(index.contains("device:"));
    assert!(index.contains("pair:"));
    assert!(index.contains("shardName"));
    assert!(index.contains("expired_codes"));
    assert!(index.contains("capListEntries"));
    assert!(index.contains("persistBoardAction"));
    assert!(index.contains("publishReservedPairing"));
    assert!(index.contains("blockConcurrencyWhile"));
    assert!(index.contains("createSerialQueue"));
    assert!(index.contains("setAlarm"));
    assert!(index.contains("alarm()"));
    let board = read("worker/src/board.js");
    assert!(board.contains("pairingCodeIsLive"));
    assert!(board.contains("LIST_MAX_DEVICES = 32"));
    assert!(board.contains("COMMAND_TTL_SECS"));
    assert!(board.contains("fitStoredDevices"));
    assert!(board.contains("expireOffers"));
    assert!(board.contains("#dropUnverifiedDevices") || board.contains("lastSeen != null"));
    assert!(board.contains("collectListParts"));
    assert!(index.contains("collectListParts"));
    assert!(board.contains("bestEffortCleanup"));
    assert!(index.contains("bestEffortCleanup"));
    assert!(board.contains("alarmNeedsUpdate"));
    assert!(board.contains("commitAlarmUnix"));
    assert!(index.contains("commitAlarmUnix"));
    assert!(index.contains("bestEffortCleanup(res, () =>"));
    assert!(board.contains("confirmLive"));
    assert!(board.contains("isAllowedDuration"));
    assert!(board.contains("error: \"taken\""));
    assert!(
        !board.contains("7 * 24 * 3600") && !board.contains("604800"),
        "status counters follow JsonStatus u64, not a seven-day ceiling"
    );
    let cloud = read("crates/never-sleep/src/cloud.rs");
    assert!(cloud.contains("retain_pending"));
    assert!(
        !cloud.contains("const CAP: usize = 64"),
        "delivered command ids must not be truncated before ack"
    );
}

#[test]
fn mac_display_name_uses_gethostname_not_etc_hostname() {
    let cloud = read("crates/never-sleep/src/cloud.rs");
    assert!(cloud.contains("libc::gethostname"));
    assert!(
        !cloud.contains("\"/etc/hostname\""),
        "LaunchServices GUI sessions do not have /etc/hostname"
    );
    assert!(!cloud.contains("\"/proc/sys/kernel/hostname\""));
}

#[test]
fn readme_describes_pairing_and_remote_standby() {
    let en = read("README.md");
    let zh = read("README.zh-CN.md");
    assert!(en.contains("## Phone board"));
    assert!(zh.contains("## 手机看板"));
    assert!(en.contains("never-sleep pair"));
    assert!(zh.contains("never-sleep pair"));
    assert!(en.contains("Start Screen-Off Standby"));
    assert!(en.contains("did not apply"));
    assert!(zh.contains("不会假装"));
    assert!(en.contains(&format!("{SITE_ORIGIN}/board/")));
    assert!(zh.contains(&format!("{SITE_ORIGIN}/zh/board/")));
}
