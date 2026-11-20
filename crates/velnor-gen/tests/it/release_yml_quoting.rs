// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use crate::release_fixture::{APP, Root, render};
use crate::release_yml_binaries::{job, runs};

type TestResult = Result<(), Box<dyn Error>>;

#[test]
fn release_yml_quotes_a_root_with_a_single_quote_in_the_build_commands() -> TestResult {
    let quoted = Root {
        dir: "it's",
        package: "tools",
        prefix: "release-plz-tools-",
        publishes: false,
    };
    let (_, tree) = render(
        &[quoted],
        Some(
            "roots = [\"it's\"]\n[[binaries]]\npackage = \"tools\"\nplatforms = [\"linux-x64\"]\n",
        ),
    )?;
    assert_eq!(
        runs(job(&tree, "build-tools-linux-x64")?),
        [
            "test -n \"$COMMIT\" -a \"$CHECKED_OUT\" = \"$COMMIT\"",
            "sudo apt-get install --yes musl-tools",
            "rustup target add x86_64-unknown-linux-musl",
            "cargo build --release --locked --manifest-path Cargo.toml --package tools --bin tools --target x86_64-unknown-linux-musl --target-dir target",
            "tar --sort=name --mtime=@0 --owner=0 --group=0 --numeric-owner -czf tools-x86_64-unknown-linux-musl.tar.gz -C 'it'\\''s/target/x86_64-unknown-linux-musl/release' tools",
        ]
    );
    Ok(())
}

#[test]
fn release_yml_quotes_a_root_with_a_single_quote_in_the_release_plz_runs() -> TestResult {
    let quoted = Root {
        dir: "it's",
        package: "tools",
        prefix: "release-plz-tools-",
        publishes: false,
    };
    let (_, tree) = render(
        &[APP, quoted],
        Some(
            "roots = [\".\", \"it's\"]\n[[binaries]]\npackage = \"tools\"\nplatforms = [\"linux-x64\"]\n",
        ),
    )?;
    let release_plz = |id: &str| -> Result<Vec<&str>, Box<dyn Error>> {
        Ok(runs(job(&tree, id)?)
            .into_iter()
            .filter(|run| run.starts_with("\"$RUNNER_TEMP/release-plz\" release"))
            .collect())
    };
    let base = "--forge github --git-token \"$GITHUB_TOKEN\"";
    let exec = "\"$RUNNER_TEMP/release-plz\"";
    assert_eq!(
        release_plz("release-pr")?,
        [
            format!(
                "{exec} release-pr {base} --manifest-path Cargo.toml --config release-plz.toml"
            ),
            format!(
                "{exec} release-pr {base} --manifest-path 'it'\\''s/Cargo.toml' --config 'it'\\''s/release-plz.toml'"
            ),
        ]
    );
    assert_eq!(
        release_plz("tag")?,
        [
            format!(
                "{exec} release {base} --manifest-path Cargo.toml --config release-plz.toml -o json > \"$RUNNER_TEMP/release-0.json\""
            ),
            format!(
                "{exec} release {base} --manifest-path 'it'\\''s/Cargo.toml' --config 'it'\\''s/release-plz.toml' -o json > \"$RUNNER_TEMP/release-1.json\""
            ),
        ]
    );
    Ok(())
}
