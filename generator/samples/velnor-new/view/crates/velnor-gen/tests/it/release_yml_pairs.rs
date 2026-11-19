// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_gen::GenerateError;
use velnor_release::ReleaseError;

use crate::release_fixture::{APP, render, steps};
use crate::release_yml_binaries::{job, runs};
use crate::yaml_tree::Node;

type TestResult = Result<(), Box<dyn Error>>;

fn entry(extra: &str) -> String {
    format!(
        "[[binaries]]\npackage = \"app\"\nplatforms = [\"linux-x64\"]\nbins = [{{ bin = \"rg\" }}]\n{extra}"
    )
}

#[test]
fn a_versioned_asset_name_names_every_bin_of_a_raw_release() -> TestResult {
    let toml = entry("format = \"raw\"\nasset_name = \"{{ bin }}-{{ version }}-{{ platform }}\"\n");
    let (_, tree) = render(&[APP], Some(&toml))?;
    let build = job(&tree, "build-app-linux-x64")?;
    let commands = runs(build);
    let version = "echo \"version=${TAG#app-v}\" >> \"$GITHUB_OUTPUT\"";
    assert!(commands.contains(&version), "{commands:?}");
    assert!(
        commands.contains(&"mkdir \"$RUNNER_TEMP/stage\""),
        "{commands:?}"
    );
    for bin in ["app", "rg"] {
        let name = format!("{bin}-${{VERSION}}-linux-x64");
        assert!(
            commands.contains(
                &format!(
                    "cp target/x86_64-unknown-linux-gnu/release/{bin} \"$RUNNER_TEMP/stage/{name}\""
                )
                .as_str()
            ),
            "{commands:?}"
        );
        assert!(commands.contains(&format!("sha256sum \"{name}\" > \"{name}.sha256\"").as_str()));
        assert!(commands.iter().any(|run| {
            run.starts_with("jq -e -R -s ")
                && run.contains(&format!("--arg asset \"{name}\" "))
                && run.contains(&format!("--rawfile rust \"{name}.rustc\" "))
                && run.ends_with(&format!("\"{name}.sha256\" > \"{name}.provenance.json\""))
                && !run.contains(&format!("  {name}\\\\n"))
        }));
    }
    let staged: Vec<&str> = steps(build)
        .iter()
        .filter(|step| {
            step.get("run")
                .and_then(Node::text)
                .is_some_and(|run| run.starts_with("sha256sum") || run.contains(".provenance"))
        })
        .filter_map(|step| step.get("working-directory").and_then(Node::text))
        .collect();
    assert_eq!(staged, ["${{ runner.temp }}/stage"; 4]);
    let upload = steps(build)
        .last()
        .and_then(|step| step.at(&["with", "path"]))
        .and_then(Node::text);
    assert_eq!(
        upload,
        Some("${{ runner.temp }}/stage/*\n!${{ runner.temp }}/stage/*.rustc")
    );

    for job_id in ["sums-app", "publish-app"] {
        let commands = runs(job(&tree, job_id)?);
        assert!(commands.contains(&version), "{job_id}: {commands:?}");
        let listing = commands.last().ok_or("no commands")?;
        for bin in ["app", "rg"] {
            for suffix in ["", ".sha256", ".provenance.json"] {
                assert!(
                    listing.contains(&format!(" \"{bin}-${{VERSION}}-linux-x64{suffix}\"")),
                    "{job_id}: {listing}"
                );
            }
        }
    }
    Ok(())
}

#[test]
fn an_unversioned_asset_name_names_every_bin_of_a_raw_release_without_staging() -> TestResult {
    let toml = entry("format = \"raw\"\nasset_name = \"{{ bin }}_{{ target }}\"\n");
    let (text, tree) = render(&[APP], Some(&toml))?;
    assert!(!text.contains("stage"));
    assert!(!text.contains("VERSION"));
    let build = job(&tree, "build-app-linux-x64")?;
    let commands = runs(build);
    for bin in ["app", "rg"] {
        let name = format!("{bin}_x86_64-unknown-linux-gnu");
        assert!(
            commands.contains(
                &format!("cp target/x86_64-unknown-linux-gnu/release/{bin} {name}").as_str()
            ),
            "{commands:?}"
        );
        assert!(commands.contains(&format!("sha256sum {name} > {name}.sha256").as_str()));
        assert!(commands.iter().any(|run| {
            run.contains(
                "capture(\"\\\\A(?<sha256>[0-9a-f]{64})  \\\\Q\" + $asset + \"\\\\E\\\\n\\\\z\")",
            ) && run.contains(&format!("--arg asset {name} "))
        }));
    }
    let upload = steps(build)
        .last()
        .and_then(|step| step.at(&["with", "path"]))
        .and_then(Node::text)
        .ok_or("no upload")?;
    assert_eq!(upload.lines().count(), 6, "{upload}");
    assert!(upload.starts_with("app_x86_64-unknown-linux-gnu\n"));
    Ok(())
}

#[test]
fn a_raw_asset_name_that_cannot_tell_the_bins_apart_is_refused() -> TestResult {
    let toml = entry("format = \"raw\"\nasset_name = \"tool-{{ platform }}\"\n");
    let error = render(&[APP], Some(&toml))
        .err()
        .ok_or("rendered a template that names every bin the same")?;
    assert!(matches!(
        error.downcast_ref::<GenerateError>(),
        Some(GenerateError::Release(
            ReleaseError::InvalidAssetName { .. }
        ))
    ));
    let one_bin =
        "[[binaries]]\npackage = \"app\"\nformat = \"raw\"\nasset_name = \"tool-{{ platform }}\"\n";
    render(&[APP], Some(one_bin))?;
    Ok(())
}

#[test]
fn a_versioned_archive_packs_the_extra_bins_and_files_under_the_expanded_name() -> TestResult {
    let toml =
        entry("include = [\"LICENSE\"]\nasset_name = \"{{ bin }}-{{ version }}-{{ platform }}\"\n");
    let (_, tree) = render(&[APP], Some(&toml))?;
    let commands = runs(job(&tree, "build-app-linux-x64")?);
    assert!(
        commands.contains(
            &"tar -czf \"$RUNNER_TEMP/stage/app-${VERSION}-linux-x64.tar.gz\" -C target/x86_64-unknown-linux-musl/release app rg -C \"$GITHUB_WORKSPACE\" LICENSE"
        ),
        "{commands:?}"
    );
    assert!(commands.iter().any(|run| {
        run.starts_with("cargo build") && run.contains("--package app --bin app --bin rg --target")
    }));
    Ok(())
}
