// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_gen::GenerateError;
use velnor_release::ReleaseError;

use crate::release_fixture::{APP, render, steps};
use crate::release_yml_binaries::{job, runs};
use crate::yaml_tree::Node;

type TestResult = Result<(), Box<dyn Error>>;

const PLAIN: &str = "[[binaries]]\npackage = \"app\"\nattest = true\n";
const DIGESTS: &str = "[[binaries]]\npackage = \"app\"\nattest = true\nmember_digests = \"sha256\"\nbins = [{ bin = \"rg\" }]\n";
const TRIPLES: &[&str] = &[
    "x86_64-unknown-linux-musl",
    "aarch64-unknown-linux-musl",
    "aarch64-apple-darwin",
];

#[test]
fn the_member_digests_are_written_from_the_archives_before_sha256sums_lists_them() -> TestResult {
    let (_, tree) = render(&[APP], Some(DIGESTS))?;
    let commands = runs(job(&tree, "sums-app")?);
    let first = commands
        .iter()
        .position(|run| run.starts_with("mkdir -p binary/"))
        .ok_or("no folder step")?;
    let folders: Vec<String> = TRIPLES
        .iter()
        .map(|triple| format!("binary/app-{triple}.tar.gz"))
        .collect();
    assert_eq!(commands[first], format!("mkdir -p {}", folders.join(" ")));
    for (offset, triple) in TRIPLES.iter().enumerate() {
        assert_eq!(
            commands[first + 1 + offset],
            format!("tar -xzf dist/app-{triple}.tar.gz -C binary/app-{triple}.tar.gz app rg")
        );
    }
    let listed: Vec<String> = TRIPLES
        .iter()
        .flat_map(|triple| ["app", "rg"].map(|member| format!("app-{triple}.tar.gz/{member}")))
        .collect();
    let write = commands[first + 4];
    assert_eq!(
        write,
        format!("sha256sum {} > ../dist/BINARY-SHA256SUMS", listed.join(" "))
    );
    let sums = steps(job(&tree, "sums-app")?);
    assert!(
        sums.iter()
            .filter(|step| step.get("run").and_then(Node::text) == Some(write))
            .all(|step| step.get("working-directory").and_then(Node::text) == Some("binary"))
    );
    let listing = commands
        .iter()
        .position(|run| run.starts_with("sha256sum ") && run.ends_with("> SHA256SUMS"))
        .ok_or("no SHA256SUMS step")?;
    assert!(first + 4 < listing, "{commands:?}");
    assert!(
        commands[listing].contains(" BINARY-SHA256SUMS "),
        "{}",
        commands[listing]
    );
    Ok(())
}

#[test]
fn the_binary_digest_file_is_attested_verified_uploaded_and_published() -> TestResult {
    let (_, tree) = render(&[APP], Some(DIGESTS))?;
    let sums = job(&tree, "sums-app")?;
    let attest = steps(sums)
        .iter()
        .find(|step| {
            step.get("uses")
                .and_then(Node::text)
                .is_some_and(|uses| uses.contains("attest"))
        })
        .ok_or("no attest step")?;
    assert_eq!(
        attest.at(&["with", "subject-path"]).and_then(Node::text),
        Some("dist/*")
    );
    let upload = steps(sums).last().ok_or("no steps")?;
    assert_eq!(
        upload.at(&["with", "path"]).and_then(Node::text),
        Some("dist/SHA256SUMS\ndist/BINARY-SHA256SUMS")
    );
    let verify = runs(job(&tree, "verify-app")?);
    assert!(
        verify
            .iter()
            .any(|run| run.contains("gh attestation verify dist/BINARY-SHA256SUMS "))
    );
    let publish = runs(job(&tree, "publish-app")?)[0].to_owned();
    assert!(publish.ends_with(" BINARY-SHA256SUMS"), "{publish}");
    Ok(())
}

#[test]
fn without_the_key_the_workflow_names_no_binary_digest_file() -> TestResult {
    let (plain, _) = render(&[APP], Some(PLAIN))?;
    assert!(!plain.contains("BINARY-SHA256SUMS"));
    assert!(!plain.contains("$RUNNER_TEMP/binary"));
    Ok(())
}

#[test]
fn a_raw_binary_has_no_archive_to_take_a_digest_from() {
    let config = "[[binaries]]\npackage = \"app\"\nformat = \"raw\"\nmember_digests = \"sha256\"\n";
    let error = render(&[APP], Some(config))
        .err()
        .and_then(|error| error.downcast::<GenerateError>().ok());
    assert!(
        matches!(
            error.as_deref(),
            Some(GenerateError::Release(ReleaseError::KeyNeedsFormat {
                key: "member_digests",
                format: "archive",
                ..
            }))
        ),
        "{error:?}"
    );
}
