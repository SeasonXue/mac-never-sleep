//! Pins the manual macOS release workflow against the review on PR #9.

const RELEASE: &str = include_str!("../../../.github/workflows/release.yml");

#[test]
fn new_release_binds_tag_to_checked_out_commit() {
    assert!(
        RELEASE.contains("--target"),
        "gh release create must pass --target so a missing tag is created from github.sha, not default-branch HEAD"
    );
    assert!(
        RELEASE.contains("GITHUB_SHA"),
        "the tag target must be the commit this job checked out and built"
    );
}

#[test]
fn existing_tag_is_rejected_when_it_names_another_commit() {
    assert!(
        RELEASE.contains("rev-list") || RELEASE.contains("ls-remote"),
        "must resolve an existing tag to a commit before publishing"
    );
    assert!(
        RELEASE.contains("already points") || RELEASE.contains("names a different commit"),
        "mismatched tags must fail the job instead of attaching binaries to the wrong SHA"
    );
}

#[test]
fn existing_release_updates_prerelease_and_notes() {
    assert!(
        RELEASE.contains("gh release edit"),
        "rerunning on an existing tag must update release metadata, not only assets"
    );
    assert!(
        RELEASE.contains("--prerelease"),
        "the workflow_dispatch prerelease input must apply on both create and edit"
    );
}

#[test]
fn replacement_assets_are_not_clobbered_in_place() {
    assert!(
        !RELEASE.contains("--clobber"),
        "gh --clobber deletes the published asset before the replacement lands"
    );
}

#[test]
fn stale_staging_assets_are_removed_before_retry() {
    let body = function_body(RELEASE, "upload_without_clobber");
    let first_upload = body
        .find("gh release upload")
        .expect("staging upload should happen inside upload_without_clobber");
    let prefix = &body[..first_upload];
    assert!(
        prefix.contains(".staging")
            && (prefix.contains("delete-asset") || prefix.contains("delete_asset_if_present")),
        "leftover .staging names must be deleted before the first upload, otherwise a retry cannot upload them: {prefix}"
    );
}

#[test]
fn canonical_assets_stay_until_replacements_are_renamed() {
    assert!(
        RELEASE.contains("--method PATCH") && RELEASE.contains("releases/assets"),
        "promote .staging onto the canonical download name by renaming, not by deleting first"
    );
    let body = function_body(RELEASE, "upload_without_clobber");
    assert!(
        body.contains(".previous"),
        "park the live canonical asset under .previous so its bytes remain until the swap finishes"
    );
    assert!(
        !body.contains("gh release upload \"${tag}\" \"$@\""),
        "do not delete every canonical name and then batch-upload replacements"
    );
}

#[test]
fn rename_uses_rest_numeric_asset_id() {
    let body = function_body(RELEASE, "asset_id");
    assert!(
        !body.contains("gh release view"),
        "gh release view --json assets .id is a GraphQL node ID (RA_...), not the REST integer: {body}"
    );
    assert!(
        body.contains("gh api")
            && (body.contains("releases/tags") || body.contains("releases/assets")),
        "must look up the integer REST asset id: {body}"
    );
}

#[test]
fn orphaned_previous_asset_is_restored_before_cleanup() {
    let body = function_body(RELEASE, "upload_without_clobber");
    let first_upload = body
        .find("gh release upload")
        .expect("staging upload should happen inside upload_without_clobber");
    let prefix = &body[..first_upload];
    assert!(
        prefix.contains(".previous") && prefix.contains("rename_asset"),
        "when .previous exists and the canonical name does not, restore it before treating it as stale: {prefix}"
    );
}

#[test]
fn cli_asset_preserves_unix_executable_mode() {
    assert!(
        RELEASE.contains("never-sleep-cli-macos.zip")
            || RELEASE.contains("never-sleep-cli-macos.tar.gz"),
        "a bare never-sleep-cli-macos download drops the executable bit"
    );
    assert!(
        RELEASE.contains("chmod +x"),
        "release notes must tell users to chmod +x the CLI after download"
    );
}

#[test]
fn remote_tag_lookup_fails_closed_on_transport_errors() {
    assert!(
        RELEASE.contains("ls-remote"),
        "must look up the remote tag before publishing"
    );
    assert!(
        RELEASE.contains("-ne 2") || RELEASE.contains("-eq 2") || RELEASE.contains("!= 2"),
        "git ls-remote --exit-code uses status 2 for an absent tag; other statuses are errors and must abort"
    );
    assert!(
        !RELEASE.contains(
            "if git ls-remote --exit-code --tags origin \"refs/tags/${TAG}\" >/dev/null 2>&1; then"
        ),
        "do not fold transport failures into 'tag is missing'"
    );
}

#[test]
fn legacy_cli_asset_stays_until_zip_is_promoted() {
    let body = function_body(RELEASE, "upload_without_clobber");
    let last_promote = body
        .rfind("rename_asset")
        .expect("staging must be promoted with rename_asset");
    let delete_legacy = body
        .find("delete_asset_if_present \"${tag}\" \"never-sleep-cli-macos\"")
        .expect("the bare never-sleep-cli-macos asset from #9 must still be removed eventually");
    assert!(
        delete_legacy > last_promote,
        "do not delete the legacy CLI until never-sleep-cli-macos.zip has been promoted"
    );
}

fn function_body<'a>(src: &'a str, name: &str) -> &'a str {
    let start = src
        .find(&format!("{name}()"))
        .unwrap_or_else(|| panic!("missing {name}()"));
    let rest = &src[start..];
    let open = rest
        .find('{')
        .unwrap_or_else(|| panic!("{name}() has no body"));
    let mut depth = 0usize;
    for (i, ch) in rest[open..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return &rest[open..=open + i];
                }
            }
            _ => {}
        }
    }
    panic!("unclosed {name}()");
}
