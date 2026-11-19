// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_gen::GenerateError;
use velnor_release::ReleaseError;

use crate::release_fixture::{APP, render};
use crate::release_yml_binaries::{job, runs};

type TestResult = Result<(), Box<dyn Error>>;

const BUNDLE: &str = "[[binaries]]\npackage = \"app\"\nplatforms = [\"linux-x64\"]\nbins = [{ bin = \"rg\" }]\ninclude = [\"LICENSE\", \"docs/NOTICE\"]\n";

fn build_runs(release_toml: &str) -> Result<Vec<String>, Box<dyn Error>> {
    let (_, tree) = render(&[APP], Some(release_toml))?;
    Ok(runs(job(&tree, "build-app-linux-x64")?)
        .into_iter()
        .map(str::to_owned)
        .collect())
}

#[test]
fn bundle_builds_every_bin_and_packs_them_flat_with_the_included_files() -> TestResult {
    let commands = build_runs(BUNDLE)?;
    let build = commands
        .iter()
        .find(|run| run.starts_with("cargo build"))
        .ok_or("no build step")?;
    assert!(
        build.contains("--package app --bin app --bin rg --target"),
        "{build}"
    );
    let pack = commands
        .iter()
        .find(|run| run.starts_with("tar "))
        .ok_or("no pack step")?;
    assert_eq!(
        pack,
        "tar -czf app-x86_64-unknown-linux-musl.tar.gz -C target/x86_64-unknown-linux-musl/release app rg -C \"$GITHUB_WORKSPACE\" LICENSE -C \"$GITHUB_WORKSPACE/docs\" NOTICE"
    );
    Ok(())
}

#[test]
fn a_binary_without_the_bundle_keys_packs_only_its_bin() -> TestResult {
    let commands = build_runs("[[binaries]]\npackage = \"app\"\nplatforms = [\"linux-x64\"]\n")?;
    assert!(commands.contains(
        &"tar -czf app-x86_64-unknown-linux-musl.tar.gz -C target/x86_64-unknown-linux-musl/release app".to_owned()
    ));
    assert!(commands.iter().any(|run| {
        run.ends_with("--bin app --target x86_64-unknown-linux-musl --target-dir target")
    }));
    Ok(())
}

#[test]
fn bundle_entries_that_could_escape_or_collide_are_refused() -> TestResult {
    let cases = [
        "include = [\"../LICENSE\"]",
        "include = [\"/etc/passwd\"]",
        "include = [\"LICENSE; id\"]",
        "include = [\"a/NOTICE\", \"b/NOTICE\"]",
        "include = [\"app\"]",
        "bins = [{ bin = \"rg; id\" }]",
        "bins = [{ bin = \"app\" }]",
        "bins = [{ bin = \"rg\" }, { bin = \"rg\" }]",
        "bins = [{ bin = \"nothere\" }]",
    ];
    for case in cases {
        let toml = format!("[[binaries]]\npackage = \"app\"\n{case}\n");
        let error = render(&[APP], Some(&toml))
            .err()
            .ok_or(format!("rendered {case}"))?;
        assert!(
            matches!(
                error.downcast_ref::<GenerateError>(),
                Some(GenerateError::Release(
                    ReleaseError::InvalidBundle { .. }
                        | ReleaseError::InvalidBinName { .. }
                        | ReleaseError::BinaryTargetMissing { .. }
                ))
            ),
            "{case}: {error}"
        );
    }
    Ok(())
}
