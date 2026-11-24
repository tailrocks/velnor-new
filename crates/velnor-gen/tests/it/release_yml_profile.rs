// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_gen::GenerateError;
use velnor_release::ReleaseError;

use crate::release_fixture::{APP, TOOLS, TWO_BINARIES, render, steps};
use crate::release_yml_binaries::{job, runs};
use crate::yaml_tree::Node;

type TestResult = Result<(), Box<dyn Error>>;

const PROFILED: &str = "roots = [\".\", \"tools\"]\n[[binaries]]\npackage = \"app\"\n[[binaries]]\npackage = \"tools\"\nplatforms = [\"linux-x64\", \"linux-arm64\", \"macos-arm64\"]\nprofile = \"release-tools\"\n";

fn profiled() -> Result<Node, Box<dyn Error>> {
    Ok(render(&[APP, TOOLS], Some(PROFILED))?.1)
}

fn build_runs<'a>(tree: &'a Node, id: &str) -> Result<Vec<&'a str>, Box<dyn Error>> {
    Ok(runs(job(tree, id)?))
}

#[test]
fn a_profiled_binary_builds_with_the_profile_and_packs_from_its_directory() -> TestResult {
    let tree = profiled()?;
    let want = [
        (
            "build-tools-linux-x64",
            "x86_64-unknown-linux-musl",
            "tar --sort=name --mtime=@0 --owner=0 --group=0 --numeric-owner -czf tools-x86_64-unknown-linux-musl.tar.gz -C tools/target/x86_64-unknown-linux-musl/release-tools tools",
        ),
        (
            "build-tools-linux-arm64",
            "aarch64-unknown-linux-musl",
            "tar --sort=name --mtime=@0 --owner=0 --group=0 --numeric-owner -czf tools-aarch64-unknown-linux-musl.tar.gz -C tools/target/aarch64-unknown-linux-musl/release-tools tools",
        ),
        (
            "build-tools-macos-arm64",
            "aarch64-apple-darwin",
            "cp -p tools/target/aarch64-apple-darwin/release-tools/tools \"$RUNNER_TEMP/pack/\"",
        ),
    ];
    for (id, triple, pack) in want {
        let commands = build_runs(&tree, id)?;
        let build = format!(
            "cargo build --profile release-tools --locked --manifest-path Cargo.toml --package tools --bin tools --target {triple} --target-dir target"
        );
        assert!(commands.contains(&build.as_str()), "{id}: {commands:?}");
        assert!(commands.contains(&pack), "{id}: {commands:?}");
    }
    Ok(())
}

#[test]
fn the_profile_belongs_to_its_binary_alone() -> TestResult {
    let tree = profiled()?;
    let commands = build_runs(&tree, "build-app-linux-x64")?;
    assert!(
        commands.contains(&"cargo build --release --locked --manifest-path Cargo.toml --package app --bin app --target x86_64-unknown-linux-musl --target-dir target"),
        "{commands:?}"
    );
    assert!(
        commands.contains(
            &"tar --sort=name --mtime=@0 --owner=0 --group=0 --numeric-owner -czf app-x86_64-unknown-linux-musl.tar.gz -C target/x86_64-unknown-linux-musl/release app"
        ),
        "{commands:?}"
    );
    Ok(())
}

#[test]
fn an_unprofiled_binary_still_builds_with_release() -> TestResult {
    let (_, tree) = render(&[APP, TOOLS], Some(TWO_BINARIES))?;
    let commands = build_runs(&tree, "build-tools-linux-x64")?;
    assert!(
        commands.contains(&"cargo build --release --locked --manifest-path Cargo.toml --package tools --bin tools --target x86_64-unknown-linux-musl --target-dir target"),
        "{commands:?}"
    );
    assert!(
        commands.contains(
            &"tar --sort=name --mtime=@0 --owner=0 --group=0 --numeric-owner -czf tools-x86_64-unknown-linux-musl.tar.gz -C tools/target/x86_64-unknown-linux-musl/release tools"
        ),
        "{commands:?}"
    );
    Ok(())
}

#[test]
fn the_size_settings_cannot_silently_drop_from_any_build_step() -> TestResult {
    let tree = profiled()?;
    for platform in ["linux-x64", "linux-arm64", "macos-arm64"] {
        let id = format!("build-tools-{platform}");
        let mut cargo_builds = 0;
        for step in steps(job(&tree, &id)?) {
            let Some(run) = step.get("run").and_then(Node::text) else {
                continue;
            };
            if run.contains("cargo build") {
                cargo_builds += 1;
                assert!(run.contains("--profile release-tools"), "{id}: {run}");
                assert!(!run.contains("--release"), "{id}: {run}");
            }
            if run.contains("target/") {
                assert!(run.contains("/release-tools"), "{id}: {run}");
                assert!(!run.contains("/release "), "{id}: {run}");
            }
        }
        assert_eq!(cargo_builds, 1, "{id}");
    }
    Ok(())
}

#[test]
fn a_profile_that_is_not_a_plain_cargo_profile_name_is_refused() -> TestResult {
    for profile in [
        "dev", "test", "bench", "debug", "", "-x", "a b", "a;b", "$(id)", "a.b", "../x",
    ] {
        let result = render(
            &[APP],
            Some(&format!(
                "[[binaries]]\npackage = \"app\"\nprofile = {profile:?}\n"
            )),
        );
        let error = result
            .err()
            .ok_or_else(|| format!("rendered {profile:?}"))?;
        assert!(
            matches!(
                error.downcast_ref::<GenerateError>(),
                Some(GenerateError::Release(ReleaseError::InvalidProfile(..)))
            ),
            "{profile:?}: {error}"
        );
    }
    Ok(())
}
