// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::error::Error;

use velnor_gen::GenerateError;
use velnor_release::ReleaseError;

use crate::release_fixture::{APP, jobs, render, steps};
use crate::release_yml_binaries::{job, runs as run_steps};
use crate::yaml_tree::Node;

type TestResult = Result<(), Box<dyn Error>>;

fn runs(job: &Node) -> Vec<String> {
    run_steps(job).into_iter().map(str::to_owned).collect()
}

const RAW: &str = "[[binaries]]\npackage = \"app\"\nbin = \"rg\"\nformat = \"raw\"\n";

fn provenance(platform: &str, target: &str) -> String {
    format!(
        "jq -n --rawfile checksum rg-{platform}.sha256 --arg repository \"$REPOSITORY\" --arg commit \"$COMMIT\" --arg tag \"$TAG\" --arg target {target} --arg asset rg-{platform} '{{schema: 1, tag: $tag, repository: $repository, commit: $commit, target: $target, asset: $asset, sha256: $checksum[0:64]}}' > rg-{platform}.provenance.json"
    )
}

#[test]
fn raw_format_publishes_the_bare_binary_with_a_checksum_and_provenance_per_platform() -> TestResult
{
    let (text, tree) = render(&[APP], Some(RAW))?;
    let ids: BTreeSet<&str> = jobs(&tree)?.into_iter().map(|(id, _)| id).collect();
    assert_eq!(
        ids,
        BTreeSet::from([
            "gate",
            "release-pr",
            "tag",
            "build-app-linux-x64",
            "build-app-linux-arm64",
            "build-app-macos-arm64",
            "sums-app",
            "publish-app",
        ])
    );
    assert!(!text.contains("musl"));
    assert!(!text.contains("rg-linux-x64.tar.gz"));

    let linux = job(&tree, "build-app-linux-x64")?;
    assert_eq!(
        runs(linux),
        [
            "test \"$CHECKED_OUT\" = \"$COMMIT\"".to_owned(),
            "rustup target add x86_64-unknown-linux-gnu".to_owned(),
            "cargo build --release --locked --manifest-path Cargo.toml --package app --bin rg --target x86_64-unknown-linux-gnu --target-dir target".to_owned(),
            "cp target/x86_64-unknown-linux-gnu/release/rg rg-linux-x64".to_owned(),
            "sha256sum rg-linux-x64 > rg-linux-x64.sha256".to_owned(),
            provenance("linux-x64", "x86_64-unknown-linux-gnu"),
        ]
    );
    let upload = steps(linux).last().ok_or("no steps")?;
    assert_eq!(
        upload.at(&["with", "path"]).and_then(Node::text),
        Some("rg-linux-x64\nrg-linux-x64.sha256\nrg-linux-x64.provenance.json")
    );
    assert_eq!(
        upload.at(&["with", "name"]).and_then(Node::text),
        Some("app-linux-x64")
    );

    let arm = job(&tree, "build-app-linux-arm64")?;
    assert!(
        runs(arm)
            .contains(&"cp target/aarch64-unknown-linux-gnu/release/rg rg-linux-arm64".to_owned())
    );

    let mac = job(&tree, "build-app-macos-arm64")?;
    let mac_runs = runs(mac);
    assert!(mac_runs.contains(&"shasum -a 256 rg-macos-arm64 > rg-macos-arm64.sha256".to_owned()));
    assert!(
        mac_runs.contains(&"cp target/aarch64-apple-darwin/release/rg rg-macos-arm64".to_owned())
    );

    let publish = job(&tree, "publish-app")?;
    let needs: BTreeSet<&str> = publish
        .get("needs")
        .ok_or("no needs")?
        .texts()
        .into_iter()
        .collect();
    assert_eq!(needs, BTreeSet::from(["gate", "tag", "sums-app"]));
    let assets: Vec<String> = ["linux-x64", "linux-arm64", "macos-arm64"]
        .iter()
        .flat_map(|p| {
            [
                format!("rg-{p}"),
                format!("rg-{p}.sha256"),
                format!("rg-{p}.provenance.json"),
            ]
        })
        .collect();
    let sums = job(&tree, "sums-app")?;
    assert_eq!(
        runs(sums)[0],
        format!("sha256sum {} > SHA256SUMS", assets.join(" "))
    );
    assert_eq!(
        runs(publish),
        [format!(
            "velnor-ci release publish --repo=\"$REPOSITORY\" --tag=\"$TAG\" --app=velnor-release --dir=dist {}",
            assets.join(" ")
        )]
    );
    Ok(())
}

#[test]
fn archive_format_stays_the_default_and_can_be_named() -> TestResult {
    let (default_text, _) = render(
        &[APP],
        Some("[[binaries]]\npackage = \"app\"\nbin = \"rg\"\n"),
    )?;
    let (named_text, _) = render(
        &[APP],
        Some("[[binaries]]\npackage = \"app\"\nbin = \"rg\"\nformat = \"archive\"\n"),
    )?;
    assert_eq!(default_text, named_text);
    assert!(default_text.contains("rg-linux-x64.tar.gz"));
    Ok(())
}

#[test]
fn an_unknown_asset_format_is_refused() {
    let refused = render(
        &[APP],
        Some("[[binaries]]\npackage = \"app\"\nformat = \"zip\"\n"),
    );
    assert!(matches!(
        refused
            .err()
            .and_then(|error| error.downcast::<GenerateError>().ok())
            .map(|error| *error),
        Some(GenerateError::Release(ReleaseError::InvalidToml { .. }))
    ));
}
