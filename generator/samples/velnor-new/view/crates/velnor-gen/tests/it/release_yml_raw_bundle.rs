// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_gen::GenerateError;
use velnor_release::ReleaseError;

use crate::release_fixture::{APP, render, steps};
use crate::release_yml_binaries::{job, runs};
use crate::yaml_tree::Node;

type TestResult = Result<(), Box<dyn Error>>;

const RAW_BUNDLE: &str = "[[binaries]]\npackage = \"app\"\nplatforms = [\"linux-x64\"]\nformat = \"raw\"\nbins = [{ bin = \"rg\" }]\n";

fn assets() -> Vec<String> {
    ["app", "rg"]
        .iter()
        .flat_map(|bin| {
            [
                format!("{bin}-linux-x64"),
                format!("{bin}-linux-x64.sha256"),
                format!("{bin}-linux-x64.provenance.json"),
            ]
        })
        .collect()
}

#[test]
fn raw_format_publishes_every_extra_bin_as_its_own_asset() -> TestResult {
    let (_, tree) = render(&[APP], Some(RAW_BUNDLE))?;
    let build = job(&tree, "build-app-linux-x64")?;
    let commands = runs(build);
    assert!(commands.iter().any(|run| run.starts_with("cargo build")
        && run.contains("--package app --bin app --bin rg --target")));
    for bin in ["app", "rg"] {
        let target = "target/x86_64-unknown-linux-gnu/release";
        assert!(commands.contains(&format!("cp {target}/{bin} {bin}-linux-x64").as_str()));
        assert!(
            commands
                .contains(&format!("sha256sum {bin}-linux-x64 > {bin}-linux-x64.sha256").as_str())
        );
        assert!(commands.contains(&format!("rustc --version > {bin}-linux-x64.rustc").as_str()));
        assert!(commands.iter().any(|run| {
            run.starts_with("jq -e -R -s ")
                && run.ends_with(&format!("> {bin}-linux-x64.provenance.json"))
                && run.contains(&format!("--arg asset {bin}-linux-x64 "))
                && run.contains(&format!("--rawfile rust {bin}-linux-x64.rustc "))
                && run.contains("version: $tag[5:]")
        }));
    }
    let upload = steps(build).last().ok_or("no steps")?;
    assert_eq!(
        upload.at(&["with", "path"]).and_then(Node::text),
        Some(assets().join("\n").as_str())
    );
    assert_eq!(
        runs(job(&tree, "sums-app")?)[0],
        format!("sha256sum {} > SHA256SUMS", assets().join(" "))
    );
    assert_eq!(
        runs(job(&tree, "publish-app")?),
        [format!(
            "velnor-ci release publish --repo=\"$REPOSITORY\" --tag=\"$TAG\" --app=velnor-release --dir=dist {}",
            assets().join(" ")
        )]
    );
    Ok(())
}

#[test]
fn raw_format_with_included_files_is_refused() {
    let refused = render(
        &[APP],
        Some("[[binaries]]\npackage = \"app\"\nformat = \"raw\"\ninclude = [\"LICENSE\"]\n"),
    );
    assert!(matches!(
        refused
            .err()
            .and_then(|error| error.downcast::<GenerateError>().ok())
            .map(|error| *error),
        Some(GenerateError::Release(ReleaseError::InvalidBundle { .. }))
    ));
}
