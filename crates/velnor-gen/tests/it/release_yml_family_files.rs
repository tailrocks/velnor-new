// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::error::Error;

use crate::release_family_fixture::{HELPER, generate, tools_like};
use crate::release_fixture::{steps, uses};
use crate::release_yml_binaries::{job, permissions, runs};
use crate::yaml_tree::Node;

type TestResult = Result<(), Box<dyn Error>>;

const EXTRAS: &str = "checksums_file = \"BINARY-SHA256SUMS\"\ncommit_file = true\n";

fn words(run: &str) -> BTreeSet<&str> {
    run.split_whitespace().collect()
}

#[test]
fn a_tools_like_family_publishes_the_archives_commit_and_binary_checksums() -> TestResult {
    let (_, tree) = generate(HELPER, &tools_like(EXTRAS))?;
    let publish = runs(job(&tree, "publish-family")?)
        .last()
        .copied()
        .ok_or("no publish")?;
    let published: BTreeSet<&str> = words(publish)
        .into_iter()
        .filter(|word| !word.starts_with("--") && !word.starts_with("velnor-ci") && *word != "release" && *word != "publish")
        .collect();
    assert_eq!(
        published,
        BTreeSet::from([
            "app-x86_64-unknown-linux-musl.tar.gz",
            "app-aarch64-unknown-linux-musl.tar.gz",
            "tools-x86_64-unknown-linux-musl.tar.gz",
            "tools-aarch64-unknown-linux-musl.tar.gz",
            "COMMIT",
            "BINARY-SHA256SUMS",
        ])
    );
    let sums = job(&tree, "sums-family")?;
    let commands = runs(sums);
    let record = steps(sums)
        .iter()
        .find(|step| step.get("run").and_then(Node::text) == Some("echo \"$COMMIT\" > COMMIT"))
        .ok_or("no COMMIT step")?;
    assert_eq!(record.get("working-directory").and_then(Node::text), Some("dist"));
    assert_eq!(
        record.at(&["env", "COMMIT"]).and_then(Node::text),
        Some("${{ needs.family.outputs.commit }}")
    );
    let binary_sums = commands
        .iter()
        .find(|run| run.ends_with("> ../dist/BINARY-SHA256SUMS"))
        .ok_or("no BINARY-SHA256SUMS step")?;
    for member in [
        "app-x86_64-unknown-linux-musl.tar.gz/app",
        "app-aarch64-unknown-linux-musl.tar.gz/app",
        "tools-x86_64-unknown-linux-musl.tar.gz/tools",
        "tools-aarch64-unknown-linux-musl.tar.gz/tools",
    ] {
        assert!(binary_sums.contains(member), "{member}");
    }
    let listed = commands
        .iter()
        .find(|run| run.ends_with("> SHA256SUMS"))
        .ok_or("no SHA256SUMS step")?;
    assert!(listed.contains("COMMIT") && listed.contains("BINARY-SHA256SUMS"));
    let upload = steps(sums)
        .iter()
        .rev()
        .find(|step| uses(step).is_some_and(|u| u.starts_with("actions/upload-artifact@")))
        .ok_or("no upload")?;
    assert_eq!(
        upload.at(&["with", "path"]).and_then(Node::text),
        Some("dist/SHA256SUMS\ndist/BINARY-SHA256SUMS\ndist/COMMIT")
    );
    Ok(())
}

#[test]
fn a_family_without_the_extras_publishes_only_the_archives() -> TestResult {
    let (text, _) = generate(HELPER, &tools_like(""))?;
    assert!(!text.contains("dist/COMMIT") && !text.contains("BINARY-SHA256SUMS"));
    Ok(())
}

#[test]
fn smoke_runs_each_built_binary_before_packing_in_a_job_with_no_secrets() -> TestResult {
    let (_, tree) = generate(HELPER, &tools_like(EXTRAS))?;
    for (package, platform, triple, dir) in [
        ("app", "linux-x64", "x86_64-unknown-linux-musl", "target"),
        ("tools", "linux-arm64", "aarch64-unknown-linux-musl", "tools/target"),
    ] {
        let build = job(&tree, &format!("build-{package}-{platform}"))?;
        let commands = runs(build);
        let run = commands
            .iter()
            .position(|run| *run == format!("{dir}/{triple}/release/{package} --version"))
            .ok_or("no --version run")?;
        let check = commands
            .iter()
            .position(|run| *run == format!("{dir}/{triple}/release/{package} check-tag \"$TAG\""))
            .ok_or("no check-tag run")?;
        let pack = commands
            .iter()
            .position(|run| run.starts_with("tar -czf"))
            .ok_or("no pack")?;
        assert!(run < check && check < pack);
        let step = steps(build)
            .iter()
            .find(|step| step.get("run").and_then(Node::text) == Some(commands[check]))
            .ok_or("no check step")?;
        assert_eq!(
            step.at(&["env", "TAG"]).and_then(Node::text),
            Some("${{ needs.family.outputs.tag }}")
        );
        assert_eq!(
            permissions(build),
            [("contents".to_owned(), "read".to_owned())]
        );
        for step in steps(build) {
            assert!(
                step.get("env").is_none_or(|env| env
                    .keys()
                    .into_iter()
                    .all(|key| !key.contains("TOKEN") && !key.contains("SECRET"))),
            );
        }
    }
    Ok(())
}
