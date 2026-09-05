//! Pins CI/release jobs to manual runs and constrains the production web deploy.

use std::fs;
use std::path::{Path, PathBuf};

fn workflows_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.github/workflows")
}

/// Body of the top-level `on:` mapping (events, inputs, comments).
fn on_mapping(src: &str) -> &str {
    let on_at = src
        .find("\non:")
        .map(|i| i + 1)
        .or_else(|| src.find("on:"))
        .expect("workflow must declare on:");
    assert!(
        src[on_at..].starts_with("on:"),
        "expected on: at byte {on_at}"
    );
    let after = &src[on_at + "on:".len()..];
    let mut end = 0;
    let mut first = true;
    for line in after.split_inclusive('\n') {
        if first {
            first = false;
            end += line.len();
            continue;
        }
        let trimmed = line.trim_end_matches(['\n', '\r']);
        if trimmed.is_empty()
            || trimmed.starts_with('#')
            || trimmed.starts_with(' ')
            || trimmed.starts_with('\t')
        {
            end += line.len();
            continue;
        }
        break;
    }
    &after[..end]
}

#[test]
fn non_deploy_github_actions_are_manual_dispatch_only() {
    let dir = workflows_dir();
    let mut paths: Vec<_> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read {dir:?}: {e}"))
        .map(|e| e.unwrap().path())
        .filter(|p| matches!(p.extension().and_then(|e| e.to_str()), Some("yml" | "yaml")))
        .collect();
    paths.sort();
    assert!(!paths.is_empty(), "expected YAML workflows under {dir:?}");

    for path in paths {
        if path.file_name().and_then(|name| name.to_str()) == Some("deploy-web.yml") {
            continue;
        }
        let src = fs::read_to_string(&path).unwrap();
        let on = on_mapping(&src);
        let name = path.file_name().unwrap().to_string_lossy();
        assert!(
            on.contains("workflow_dispatch"),
            "{name} must use workflow_dispatch (Actions → Run workflow): {on}"
        );
        for event in ["push:", "pull_request", "pull_request_target:", "schedule:"] {
            assert!(
                !on.contains(event),
                "{name} must not auto-start on {event}: {on}"
            );
        }
    }
}

#[test]
fn web_deploy_is_gated_tested_and_smoke_checked() {
    let src = fs::read_to_string(workflows_dir().join("deploy-web.yml"))
        .expect("deploy-web.yml must publish the Worker and static site");
    let on = on_mapping(&src);

    for required in [
        "workflow_dispatch:",
        "push:",
        "branches: [main]",
        ".github/workflows/deploy-web.yml",
        "worker/**",
        "site/**",
        "wrangler.jsonc",
        "package.json",
        "package-lock.json",
    ] {
        assert!(
            on.contains(required),
            "deploy trigger must contain {required}: {on}"
        );
    }

    for required in [
        "environment: production",
        "group: never-sleep-production",
        "cancel-in-progress: false",
        "actions/checkout@",
        "github.event_name == 'push' && github.sha || 'main'",
        "actions/setup-node@",
        "npm ci",
        "npm test",
        "cloudflare/wrangler-action@v3",
        "CLOUDFLARE_API_TOKEN",
        "CLOUDFLARE_ACCOUNT_ID",
        "wranglerVersion: \"4.128.0\"",
        "command: deploy",
        "https://xyz-ai.app/never-sleep/zh/board/",
        "https://xyz-ai.app/never-sleep/api/list",
    ] {
        assert!(src.contains(required), "web deploy must contain {required}");
    }
}

#[test]
fn linux_ci_runs_worker_node_tests() {
    let src = fs::read_to_string(workflows_dir().join("ci.yml")).unwrap();
    let linux = src
        .split("test-linux:")
        .nth(1)
        .expect("test-linux job")
        .split("build-macos:")
        .next()
        .unwrap();
    assert!(
        linux.contains("actions/setup-node") && linux.contains("npm test"),
        "Worker regressions must fail Linux CI, not only local `node --test`"
    );
}
