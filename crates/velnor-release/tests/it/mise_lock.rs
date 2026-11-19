// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_release::{ReleaseError, ReleasePlan};

use crate::fixture::{MISE_LOCK, RELEASE_PLZ_SHA256, RELEASE_PLZ_URL, TestResult, files, refusal};

#[test]
fn a_mise_lock_that_downloads_release_plz_from_elsewhere_is_refused() {
    let own = "https://github.com/release-plz/release-plz/releases/download/release-plz-v0.4.3/release-plz-x86_64-unknown-linux-gnu.tar.gz";
    let api = "https://api.github.com/repos/release-plz/release-plz/releases/assets/1";
    let lock = |url: &str, url_api: &str| {
        format!(
            "[[tools.release-plz]]\nversion = \"0.4.3\"\n\n[tools.release-plz.\"platforms.linux-x64\"]\nchecksum = \"sha256:{RELEASE_PLZ_SHA256}\"\nurl = \"{url}\"\nurl_api = \"{url_api}\"\n"
        )
    };
    let mut files = files();
    files.mise_tomls = vec!["[tools]\nrelease-plz = \"0.4.3\"\n".to_owned()];
    files.mise_lock = lock(own, api);
    assert_eq!(refusal(&files), None);
    for (url, url_api) in [
        ("https://evil.example/release-plz.tar.gz", api),
        (own, "https://evil.example/asset"),
        (
            "https://github.com/release-plz/release-plz/releases/download.evil.example/x",
            api,
        ),
        (
            "https://github.com/evil/release-plz/releases/download/x",
            api,
        ),
        (
            "https://github.com/release-plz/release-plz/releases/download/../../../../evil/x/releases/download/v1/x",
            api,
        ),
        (
            "https://github.com/release-plz/release-plz/releases/download/%2e%2e/%2e%2e/%2e%2e/%2e%2e/evil/x/releases/download/v1/x",
            api,
        ),
        (
            "https://github.com/release-plz/release-plz/releases/download/%2E%2E/x/y",
            api,
        ),
        (
            "https://github.com/release-plz/release-plz/releases/download/\\\\..\\\\evil/x",
            api,
        ),
        (
            "https://github.com/release-plz/release-plz/releases/download/v1/x?x",
            api,
        ),
        (
            "https://github.com/release-plz/release-plz/releases/download/v1/x#x",
            api,
        ),
        (
            "https://github.com/release-plz/release-plz/releases/download/v1//x",
            api,
        ),
        (
            "https://github.com/release-plz/release-plz/releases/download/v1/x/y",
            api,
        ),
        (
            "https://github.com/release-plz/release-plz/releases/download/v1/x@y",
            api,
        ),
        (
            "https://github.com/release-plz/release-plz/releases/download/v1",
            api,
        ),
        (
            own,
            "https://api.github.com/repos/release-plz/release-plz/releases/assets/../../../../evil/x",
        ),
    ] {
        files.mise_lock = lock(url, url_api);
        assert!(
            matches!(
                refusal(&files),
                Some(ReleaseError::ReleasePlzArtifactElsewhere { .. })
            ),
            "{url} {url_api}"
        );
    }
}

#[test]
fn a_mise_lock_archive_for_another_release_tag_is_refused() {
    let mut files = files();
    files.mise_tomls = vec!["[tools]\nrelease-plz = \"0.4.3\"\n".to_owned()];
    let api = "https://api.github.com/repos/release-plz/release-plz/releases/assets/1";
    for url in [
        "https://github.com/release-plz/release-plz/releases/download/release-plz-v0.4.2/release-plz-x86_64-unknown-linux-gnu.tar.gz",
        "https://github.com/release-plz/release-plz/releases/download/v0.4.3/release-plz-x86_64-unknown-linux-gnu.tar.gz",
        "https://github.com/release-plz/release-plz/releases/download/release-plz-v0.4.3+build.1/release-plz-x86_64-unknown-linux-gnu.tar.gz",
    ] {
        files.mise_lock = format!(
            "[[tools.release-plz]]\nversion = \"0.4.3\"\n\n[tools.release-plz.\"platforms.linux-x64\"]\nchecksum = \"sha256:{RELEASE_PLZ_SHA256}\"\nurl = \"{url}\"\nurl_api = \"{api}\"\n"
        );
        assert!(
            matches!(
                refusal(&files),
                Some(ReleaseError::ReleasePlzArtifactElsewhere { .. })
            ),
            "{url}"
        );
    }
}

#[test]
fn a_mise_lock_without_a_verifiable_linux_download_is_refused() {
    let mut files = files();
    for lock in [
        "[[tools.release-plz]]\nversion = \"0.4.3\"\n".to_owned(),
        MISE_LOCK.replace("sha256:", "md5:"),
        MISE_LOCK.replace(RELEASE_PLZ_SHA256, "00"),
        MISE_LOCK.replace("\nurl = ", "\nurl_api = "),
        MISE_LOCK.replace("platforms.linux-x64", "platforms.linux-arm64"),
    ] {
        files.mise_lock = lock;
        assert!(
            matches!(
                refusal(&files),
                Some(ReleaseError::ReleasePlzNotLockedForLinux { .. })
            ),
            "{}",
            files.mise_lock
        );
    }
}

#[test]
fn the_plan_carries_the_locked_linux_download_of_release_plz() -> TestResult {
    let plan = ReleasePlan::read(&files())?;
    assert_eq!(plan.release_plz_sha256, RELEASE_PLZ_SHA256);
    assert_eq!(plan.release_plz_url, RELEASE_PLZ_URL);
    Ok(())
}

#[test]
fn the_cargo_backend_is_refused_by_name_rather_than_as_an_unlocked_download() {
    let mut files = files();
    files.mise_tomls = vec!["[tools]\n\"cargo:release-plz\" = \"0.4.3\"\n".to_owned()];
    files.mise_lock = "[[tools.\"cargo:release-plz\"]]\nversion = \"0.4.3\"\n".to_owned();
    assert_eq!(
        refusal(&files),
        Some(ReleaseError::ReleasePlzBackendNotDownloadable {
            tool: "cargo:release-plz".to_owned()
        })
    );
}

#[test]
fn the_ubi_backend_is_refused_by_name_rather_than_as_an_unlocked_download() {
    let mut files = files();
    files.mise_tomls = vec!["[tools]\n\"ubi:release-plz/release-plz\" = \"0.4.3\"\n".to_owned()];
    files.mise_lock = "[[tools.\"ubi:release-plz/release-plz\"]]\nversion = \"0.4.3\"\n".to_owned();
    assert_eq!(
        refusal(&files),
        Some(ReleaseError::ReleasePlzBackendNotDownloadable {
            tool: "ubi:release-plz/release-plz".to_owned()
        })
    );
}
