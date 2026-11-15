// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use crate::release_fixture::{Root, render};
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
            "rustup target add x86_64-unknown-linux-musl",
            "cargo build --release --locked --manifest-path 'it'\\''s/Cargo.toml' --package tools --target x86_64-unknown-linux-musl",
            "tar -czf tools-linux-x64.tar.gz -C 'it'\\''s/target/x86_64-unknown-linux-musl/release' tools",
        ]
    );
    Ok(())
}
