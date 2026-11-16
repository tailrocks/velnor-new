// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_gen::GenerateError;
use velnor_release::ReleaseError;

use crate::release_fixture::{APP, Root, TOOLS, TWO_BINARIES, render, steps};
use crate::release_yml_binaries::{job, runs};
use crate::yaml_tree::Node;

type TestResult = Result<(), Box<dyn Error>>;

#[test]
fn release_yml_build_packs_the_named_bin_and_uploads_exactly_that_file() -> TestResult {
    let (_, tree) = render(
        &[APP],
        Some("[[binaries]]\npackage = \"app\"\nbin = \"rg\"\nplatforms = [\"linux-arm64\"]\n"),
    )?;
    let build = job(&tree, "build-app-linux-arm64")?;
    assert_eq!(
        runs(build),
        [
            "test \"$CHECKED_OUT\" = \"$COMMIT\"",
            "sudo apt-get install --yes musl-tools",
            "rustup target add aarch64-unknown-linux-musl",
            "cargo build --release --locked --manifest-path Cargo.toml --package app --bin rg --target aarch64-unknown-linux-musl --target-dir target",
            "tar -czf rg-linux-arm64.tar.gz -C target/aarch64-unknown-linux-musl/release rg",
        ]
    );
    let upload = steps(build).last().ok_or("no steps")?;
    assert_eq!(
        upload.at(&["with", "name"]).and_then(Node::text),
        Some("app-linux-arm64")
    );
    assert_eq!(
        upload.at(&["with", "path"]).and_then(Node::text),
        Some("rg-linux-arm64.tar.gz")
    );
    assert_eq!(
        upload
            .at(&["with", "if-no-files-found"])
            .and_then(Node::text),
        Some("error")
    );
    let publish = job(&tree, "publish-app")?;
    assert!(runs(publish)[0].ends_with(" --dir dist rg-linux-arm64.tar.gz"));
    Ok(())
}

#[test]
fn release_yml_refuses_a_bin_name_that_would_add_a_second_command() -> TestResult {
    for bin in ["rg; echo", "a b", "$(id)", "a.b"] {
        let result = render(
            &[APP, Root { ..TOOLS }],
            Some(&format!(
                "roots = [\".\", \"tools\"]\n[[binaries]]\npackage = \"app\"\nbin = {bin:?}\n"
            )),
        );
        let error = result.err().ok_or("rendered")?;
        assert!(
            matches!(
                error.downcast_ref::<GenerateError>(),
                Some(GenerateError::Release(ReleaseError::InvalidBinName { .. }))
            ),
            "{bin:?}: {error}"
        );
    }
    assert!(render(&[APP, TOOLS], Some(TWO_BINARIES)).is_ok());
    Ok(())
}
