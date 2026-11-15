// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_release::ReleaseError;

use crate::fixture::{files, refusal};

#[test]
fn a_mise_lock_that_downloads_release_plz_from_elsewhere_is_refused() {
    let own = "https://github.com/release-plz/release-plz/releases/download/release-plz-v0.4.3/release-plz-x86_64-unknown-linux-gnu.tar.gz";
    let api = "https://api.github.com/repos/release-plz/release-plz/releases/assets/1";
    let lock = |url: &str, url_api: &str| {
        format!(
            "[[tools.release-plz]]\nversion = \"0.4.3\"\n\n[tools.release-plz.\"platforms.linux-x64\"]\nchecksum = \"sha256:00\"\nurl = \"{url}\"\nurl_api = \"{url_api}\"\n"
        )
    };
    let mut files = files();
    files.mise_tomls = vec!["[tools]\nrelease-plz = \"0.4.3\"\n".to_owned()];
    files.mise_lock = lock(own, api);
    assert_eq!(refusal(&files), None);
    let valid_with_plus = "https://github.com/release-plz/release-plz/releases/download/v0.4.3+build.1/release-plz_0.4.3-x86_64.tar.gz";
    files.mise_lock = lock(valid_with_plus, api);
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
