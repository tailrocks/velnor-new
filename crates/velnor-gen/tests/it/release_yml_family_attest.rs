// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use crate::release_family_fixture::{HELPER, generate, tools_like};
use crate::release_fixture::{jobs, steps, uses};
use crate::release_yml_binaries::{job, permissions, runs};
use crate::yaml_tree::Node;

type TestResult = Result<(), Box<dyn Error>>;

const ATTEST: &str = "actions/attest-build-provenance";
const EXTRAS: &str = "checksums_file = \"BINARY-SHA256SUMS\"\ncommit_file = true\nattest = true\n";

#[test]
fn family_attestation_runs_in_the_sums_job_alone_with_the_only_id_token() -> TestResult {
    let (_, tree) = generate(HELPER, &tools_like(EXTRAS))?;
    assert_eq!(tree.get("permissions"), Some(&Node::Map(Vec::new())));
    let sums = job(&tree, "sums-family")?;
    assert_eq!(
        permissions(sums),
        [
            ("attestations".to_owned(), "write".to_owned()),
            ("id-token".to_owned(), "write".to_owned())
        ]
    );
    let attest = steps(sums)
        .iter()
        .find(|step| uses(step).is_some_and(|u| u.starts_with(ATTEST)))
        .ok_or("no attest step")?;
    let (_, sha) = uses(attest).ok_or("no uses")?.split_once('@').ok_or("unpinned")?;
    assert!(sha.len() == 40 && sha.bytes().all(|b| b.is_ascii_hexdigit()));
    assert_eq!(
        attest.at(&["with", "subject-path"]).and_then(Node::text),
        Some("dist/*")
    );
    let position = |prefix: &str| {
        steps(sums)
            .iter()
            .position(|step| uses(step).is_some_and(|u| u.starts_with(prefix)))
    };
    assert!(position(ATTEST) < position("actions/upload-artifact@"));
    for (id, other) in jobs(&tree)? {
        if id != "sums-family" {
            assert!(
                permissions(other)
                    .iter()
                    .all(|(scope, access)| scope != "id-token"
                        && (scope != "attestations" || (id == "verify-family" && access == "read"))),
                "{id}"
            );
        }
    }
    Ok(())
}

#[test]
fn publish_waits_for_a_verification_of_every_family_file() -> TestResult {
    let (_, tree) = generate(HELPER, &tools_like(EXTRAS))?;
    let verify = job(&tree, "verify-family")?;
    assert_eq!(
        permissions(verify),
        [("attestations".to_owned(), "read".to_owned())]
    );
    let verified: Vec<&str> = runs(verify)
        .into_iter()
        .filter(|run| run.contains("gh attestation verify"))
        .collect();
    for file in [
        "dist/app-x86_64-unknown-linux-musl.tar.gz",
        "dist/tools-aarch64-unknown-linux-musl.tar.gz",
        "dist/COMMIT",
        "dist/BINARY-SHA256SUMS",
        "dist/SHA256SUMS",
    ] {
        assert!(
            verified.iter().any(|run| run.contains(&format!("verify {file} "))),
            "{file}"
        );
    }
    let publish = job(&tree, "publish-family")?;
    let needs = publish.get("needs").ok_or("no needs")?.texts();
    assert!(needs.contains(&"verify-family") && needs.contains(&"family"));
    Ok(())
}

#[test]
fn a_family_without_attest_has_no_attestation_job_or_token() -> TestResult {
    let (text, _) = generate(HELPER, &tools_like(""))?;
    assert!(!text.contains("attest") && !text.contains("id-token"));
    Ok(())
}
