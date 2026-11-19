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

fn provenance(platform: &str, target: &str) -> Vec<String> {
    let name = format!("rg-{platform}");
    vec![
        format!("rustc --version > {name}.rustc"),
        format!(
            "jq -e -R -s --arg repository \"$REPOSITORY\" --arg commit \"$COMMIT\" --arg tag \"$TAG\" --arg target {target} --arg asset {name} --rawfile rust {name}.rustc '{{schema: 1}} + capture(\"\\\\A(?<sha256>[0-9a-f]{{64}})  {name}\\\\n\\\\z\") + {{tag: $tag, version: $tag[5:], repository: $repository, commit: $commit, target: $target, asset: $asset, toolchain: {{rust: $rust[0:-1]}}}}' {name}.sha256 > {name}.provenance.json"
        ),
    ]
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
    assert!(!text.contains("rg-x86_64-unknown-linux-gnu.tar.gz"));

    let linux = job(&tree, "build-app-linux-x64")?;
    assert_eq!(
        runs(linux),
        [
            "test \"$CHECKED_OUT\" = \"$COMMIT\"".to_owned(),
            "rustup target add x86_64-unknown-linux-gnu".to_owned(),
            "cargo build --release --locked --manifest-path Cargo.toml --package app --bin rg --target x86_64-unknown-linux-gnu --target-dir target".to_owned(),
            "cp target/x86_64-unknown-linux-gnu/release/rg rg-linux-x64".to_owned(),
            "sha256sum rg-linux-x64 > rg-linux-x64.sha256".to_owned(),
        ]
        .into_iter()
        .chain(provenance("linux-x64", "x86_64-unknown-linux-gnu"))
        .collect::<Vec<_>>()
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
fn raw_format_names_the_arm_and_macos_assets() -> TestResult {
    let (_, tree) = render(&[APP], Some(RAW))?;
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
    assert!(default_text.contains("rg-x86_64-unknown-linux-musl.tar.gz"));
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

fn provenance_script() -> Result<String, Box<dyn Error>> {
    let (_, tree) = render(&[APP], Some(RAW))?;
    let script = runs(job(&tree, "build-app-linux-x64")?)
        .into_iter()
        .find(|run| run.contains("provenance.json"))
        .ok_or("no provenance step")?;
    Ok(script)
}

fn run_provenance(sidecar: &str) -> Result<(bool, String), Box<dyn Error>> {
    use std::process::Command;

    let dir = std::env::temp_dir().join(format!(
        "velnor-raw-prov-{}-{}",
        std::process::id(),
        sidecar.len()
    ));
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join("rg-linux-x64.sha256"), sidecar)?;
    std::fs::write(dir.join("rg-linux-x64.rustc"), "rustc 1.99.0 (stub)\n")?;
    let output = Command::new("/bin/sh")
        .arg("-c")
        .arg(provenance_script()?)
        .current_dir(&dir)
        .env("PATH", "/usr/bin:/bin")
        .env("REPOSITORY", "o/r")
        .env("COMMIT", "abc")
        .env("TAG", "app-v1.2.3")
        .output()?;
    let produced =
        std::fs::read_to_string(dir.join("rg-linux-x64.provenance.json")).unwrap_or_default();
    std::fs::remove_dir_all(&dir)?;
    Ok((output.status.success(), produced))
}

#[test]
fn provenance_is_written_only_for_a_strict_sidecar() -> TestResult {
    let digest = "a".repeat(64);
    let (ok, json) = run_provenance(&format!("{digest}  rg-linux-x64\n"))?;
    assert!(ok);
    let record: serde_json::Value = serde_json::from_str(&json)?;
    assert_eq!(
        record,
        serde_json::json!({
            "schema": 1,
            "sha256": digest,
            "tag": "app-v1.2.3",
            "version": "1.2.3",
            "repository": "o/r",
            "commit": "abc",
            "target": "x86_64-unknown-linux-gnu",
            "asset": "rg-linux-x64",
            "toolchain": {"rust": "rustc 1.99.0 (stub)"},
        })
    );
    for bad in [
        format!("{digest}  other\n"),
        format!("{}  rg-linux-x64\n", "a".repeat(63)),
        format!("{}  rg-linux-x64\n", "A".repeat(64)),
        format!("{digest}  rg-linux-x64\n{digest}  rg-linux-x64\n"),
        format!("{digest}  rg-linux-x64 extra\n"),
        format!("{digest}  rg-linux-x64"),
        String::new(),
    ] {
        let (ok, json) = run_provenance(&bad)?;
        assert!(!ok, "accepted {bad:?}");
        assert!(json.is_empty(), "wrote provenance for {bad:?}");
    }
    Ok(())
}
