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
