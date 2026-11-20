// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;
use std::process::Output;

use super::{Dir, REPO, TAG, all_files, published_digests, serve, velnor_ci};

fn publish_optional(api: &Path, dir: &Dir, optional: &str) -> Output {
    let dir = dir.0.to_string_lossy().into_owned();
    let args = [
        "release",
        "publish",
        "--repo",
        REPO,
        "--tag",
        TAG,
        "--app",
        "velnor-release",
        "--dir",
        &dir,
        "--optional-asset",
        optional,
        "velnor-ci-linux-x64",
        "velnor-ci-linux-arm64",
    ];
    velnor_ci(&args, &[("GITHUB_API_URL", api)])
}

#[test]
fn release_publish_takes_an_optional_asset_that_is_present_and_skips_one_that_is_not() {
    let (api, fake) = serve();
    let id = fake.app_draft();
    let dir = Dir::new("publish-optional");
    for (name, text) in all_files() {
        dir.write(name, text);
    }
    let output = publish_optional(Path::new(&api), &dir, "velnor-ci-macos-arm64");
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(!fake.is_draft(id));
    assert_eq!(fake.assets(id), published_digests());

    let (api, fake) = serve();
    let id = fake.app_draft();
    let dir = Dir::new("publish-optional-missing");
    let sums: String = all_files()
        .iter()
        .find(|(name, _)| *name == "SHA256SUMS")
        .map(|(_, text)| text.lines().take(2).flat_map(|line| [line, "\n"]).collect())
        .unwrap_or_default();
    for (name, text) in all_files() {
        match name {
            "velnor-ci-macos-arm64" => {}
            "SHA256SUMS" => {
                dir.write(name, &sums);
            }
            _ => {
                dir.write(name, text);
            }
        }
    }
    let output = publish_optional(Path::new(&api), &dir, "velnor-ci-macos-arm64");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(
        stdout.contains("skipped optional velnor-ci-macos-arm64: not found\n"),
        "{stdout}"
    );
    assert!(!fake.is_draft(id));
    assert_eq!(fake.assets(id).len(), 3);
}
