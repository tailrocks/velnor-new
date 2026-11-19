// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_gen::GenerateError;
use velnor_release::ReleaseError;

use crate::release_fixture::{APP, jobs, render, steps};
use crate::release_tagged_commit::render_with;
use crate::release_yml_binaries::{job, runs};
use crate::yaml_tree::Node;

type TestResult = Result<(), Box<dyn Error>>;

const RAW: &str = "[[binaries]]\npackage = \"app\"\nbin = \"rg\"\nformat = \"raw\"\n";
const SMOKE: &str =
    "[[binaries]]\npackage = \"app\"\nbin = \"rg\"\nformat = \"raw\"\nsmoke = true\n";
const STAGED: &str = "[[binaries]]\npackage = \"app\"\nbin = \"rg\"\nformat = \"raw\"\nsmoke = true\nasset_name = \"{{ bin }}-{{ version }}-{{ platform }}\"\n";
const MUSL: &str =
    "[[binaries]]\npackage = \"app\"\nbin = \"rg\"\nformat = \"raw\"\ntarget = \"musl\"\n";
const COMMIT: &str =
    "[[binaries]]\npackage = \"app\"\nbin = \"rg\"\nformat = \"raw\"\ncommit_file = true\n";

fn refused(config: &str) -> Option<ReleaseError> {
    match render(&[APP], Some(config))
        .err()?
        .downcast::<GenerateError>()
    {
        Ok(error) => match *error {
            GenerateError::Release(error) => Some(error),
            _ => None,
        },
        Err(_) => None,
    }
}

#[test]
fn smoke_adds_run_and_check_tag_steps_after_packing_each_raw_asset() -> TestResult {
    let (_, tree) = render(&[APP], Some(SMOKE))?;
    for platform in ["linux-x64", "linux-arm64", "macos-arm64"] {
        let build = job(&tree, &format!("build-app-{platform}"))?;
        let commands = runs(build);
        let provenance = commands
            .iter()
            .position(|run| run.ends_with(&format!("> rg-{platform}.provenance.json")))
            .ok_or("no provenance step")?;
        assert_eq!(
            &commands[provenance + 1..],
            [
                format!("./rg-{platform} --version"),
                format!("./rg-{platform} check-tag \"$TAG\""),
            ]
        );
        let all = steps(build);
        let check = &all[all.len() - 2];
        assert_eq!(
            check.at(&["env", "TAG"]).and_then(Node::text),
            Some("${{ needs.tag.outputs.app }}")
        );
        assert!(
            all.last()
                .and_then(|step| step.get("uses"))
                .and_then(Node::text)
                .is_some_and(|uses| uses.starts_with("actions/upload-artifact@"))
        );
    }
    Ok(())
}

#[test]
fn smoke_runs_the_staged_asset_from_the_stage_folder_when_the_name_has_the_version() -> TestResult {
    let (_, tree) = render(&[APP], Some(STAGED))?;
    let build = job(&tree, "build-app-linux-x64")?;
    let smoke: Vec<&Node> = steps(build)
        .iter()
        .filter(|step| {
            step.get("run").and_then(Node::text).is_some_and(|run| {
                run.ends_with(" --version") && !run.contains("rustc") || run.contains(" check-tag ")
            })
        })
        .collect();
    assert_eq!(smoke.len(), 2);
    for step in &smoke {
        assert_eq!(
            step.get("working-directory").and_then(Node::text),
            Some("${{ runner.temp }}/stage")
        );
    }
    assert_eq!(
        runs(build)
            .into_iter()
            .filter(|run| run.starts_with("\"./") || run.starts_with("./"))
            .collect::<Vec<_>>(),
        [
            "\"./rg-${VERSION}-linux-x64\" --version",
            "\"./rg-${VERSION}-linux-x64\" check-tag \"$TAG\"",
        ]
    );
    Ok(())
}

#[test]
fn smoke_run_values_have_no_expression_and_take_the_tag_from_env() -> TestResult {
    for config in [SMOKE, STAGED] {
        let (_, tree) = render(&[APP], Some(config))?;
        for (id, job) in jobs(&tree)? {
            for run in runs(job) {
                assert!(
                    !run.contains("${{") && !run.contains('\n') && !run.contains(';'),
                    "{id}: {run}"
                );
                if run.contains("check-tag") {
                    assert!(run.ends_with("check-tag \"$TAG\""), "{id}: {run}");
                }
            }
        }
    }
    Ok(())
}

#[test]
fn smoke_on_an_archive_binary_runs_the_built_binary_before_packing() -> TestResult {
    let (_, tree) = render(
        &[APP],
        Some("[[binaries]]\npackage = \"app\"\nsmoke = true\n"),
    )?;
    let commands = runs(job(&tree, "build-app-linux-x64")?);
    let run = commands
        .iter()
        .position(|run| *run == "target/x86_64-unknown-linux-musl/release/app --version")
        .ok_or("no run")?;
    let pack = commands
        .iter()
        .position(|run| run.starts_with("tar -czf"))
        .ok_or("no pack")?;
    assert_eq!(
        commands[run + 1],
        "target/x86_64-unknown-linux-musl/release/app check-tag \"$TAG\""
    );
    assert!(run + 1 < pack);
    Ok(())
}

#[test]
fn musl_raw_builds_the_musl_triple_and_installs_musl_tools() -> TestResult {
    let (_, tree) = render(&[APP], Some(MUSL))?;
    for (id, job) in jobs(&tree)?
        .into_iter()
        .filter(|(id, _)| id.starts_with("build-"))
    {
        assert!(
            !runs(job).iter().any(|run| run.contains("linux-gnu")),
            "{id}"
        );
    }
    let linux = job(&tree, "build-app-linux-x64")?;
    let commands = runs(linux);
    assert!(commands.contains(&"sudo apt-get install --yes musl-tools"));
    assert!(commands.contains(&"rustup target add x86_64-unknown-linux-musl"));
    assert!(commands.contains(&"cp target/x86_64-unknown-linux-musl/release/rg rg-linux-x64"));
    let build_step = steps(linux)
        .iter()
        .find(|step| {
            step.get("run")
                .and_then(Node::text)
                .is_some_and(|run| run.starts_with("cargo build"))
        })
        .ok_or("no build step")?;
    assert_eq!(
        build_step.at(&["env", "TARGET_CC"]).and_then(Node::text),
        Some("musl-gcc")
    );
    assert!(
        runs(job(&tree, "build-app-linux-arm64")?)
            .contains(&"rustup target add aarch64-unknown-linux-musl")
    );
    let mac = runs(job(&tree, "build-app-macos-arm64")?);
    assert!(mac.contains(&"rustup target add aarch64-apple-darwin"));
    assert!(!mac.iter().any(|run| run.contains("musl")));
    Ok(())
}

#[test]
fn musl_needs_a_linux_platform() {
    for config in [
        "[[binaries]]\npackage = \"app\"\ntarget = \"musl\"\nplatforms = [\"macos-arm64\"]\n",
        "[[binaries]]\npackage = \"app\"\nformat = \"raw\"\ntarget = \"musl\"\nplatforms = [\"macos-arm64\"]\n",
    ] {
        assert!(matches!(
            refused(config),
            Some(ReleaseError::MuslNeedsLinux { .. })
        ));
    }
}

#[test]
fn commit_file_publishes_the_tag_commit_as_an_asset_listed_in_the_checksums() -> TestResult {
    let tree = render_with("0.1.81", &[APP], COMMIT)?;
    let sums = job(&tree, "sums-app")?;
    let record = steps(sums)
        .iter()
        .find(|step| step.get("run").and_then(Node::text) == Some("echo \"$COMMIT\" > COMMIT"))
        .ok_or("no COMMIT step")?;
    assert_eq!(
        record.get("working-directory").and_then(Node::text),
        Some("dist")
    );
    assert_eq!(
        record.at(&["env", "COMMIT"]).and_then(Node::text),
        Some("${{ needs.tag.outputs.app_commit }}")
    );
    let commands = runs(sums);
    assert!(
        commands
            .iter()
            .position(|run| *run == "echo \"$COMMIT\" > COMMIT")
            < commands
                .iter()
                .position(|run| run.ends_with("> SHA256SUMS"))
    );
    assert!(commands[1].split(' ').any(|word| word == "COMMIT"));
    let upload = steps(sums).last().ok_or("no steps")?;
    assert_eq!(
        upload.at(&["with", "path"]).and_then(Node::text),
        Some("dist/SHA256SUMS\ndist/COMMIT")
    );
    let publish = runs(job(&tree, "publish-app")?);
    assert!(publish[0].ends_with(" COMMIT"));
    Ok(())
}

#[test]
fn commit_file_records_the_commit_the_tag_job_recorded() -> TestResult {
    let (_, tree) = render(&[APP], Some(COMMIT))?;
    let record = steps(job(&tree, "sums-app")?)
        .iter()
        .find(|step| step.get("run").and_then(Node::text) == Some("echo \"$COMMIT\" > COMMIT"))
        .ok_or("no COMMIT step")?;
    assert_eq!(
        record.at(&["env", "COMMIT"]).and_then(Node::text),
        Some("${{ needs.tag.outputs.app_commit }}")
    );
    Ok(())
}

#[test]
fn without_the_keys_the_workflow_is_byte_identical() -> TestResult {
    let (plain, _) = render(&[APP], Some(RAW))?;
    let (explicit, _) = render(
        &[APP],
        Some(&RAW.replace(
            "format = \"raw\"\n",
            "format = \"raw\"\ntarget = \"gnu\"\nsmoke = false\ncommit_file = false\n",
        )),
    )?;
    assert_eq!(plain, explicit);
    let archive = "[[binaries]]\npackage = \"app\"\n";
    let (plain, _) = render(&[APP], Some(archive))?;
    let (explicit, _) = render(
        &[APP],
        Some(&format!("{archive}smoke = false\ncommit_file = false\n")),
    )?;
    assert_eq!(plain, explicit);
    Ok(())
}
