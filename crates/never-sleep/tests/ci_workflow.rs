//! Pins GitHub Actions to the Actions tab (Run workflow), not push or PR.

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
fn github_actions_are_manual_dispatch_only() {
    let dir = workflows_dir();
    let mut paths: Vec<_> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read {dir:?}: {e}"))
        .map(|e| e.unwrap().path())
        .filter(|p| matches!(p.extension().and_then(|e| e.to_str()), Some("yml" | "yaml")))
        .collect();
    paths.sort();
    assert!(!paths.is_empty(), "expected YAML workflows under {dir:?}");

    for path in paths {
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
