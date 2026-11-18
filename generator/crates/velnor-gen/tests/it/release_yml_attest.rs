// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use crate::release_fixture::{APP, TOOLS, TWO_BINARIES, jobs, render, steps, uses};
use crate::release_yml_binaries::{job, permissions};
use crate::yaml_tree::Node;

type TestResult = Result<(), Box<dyn Error>>;

const ATTEST: &str = "actions/attest-build-provenance";

fn attested() -> String {
    TWO_BINARIES.replacen(
        "package = \"app\"\n",
        "package = \"app\"\nattest = true\n",
        1,
    )
}

fn attest_steps(job: &Node) -> Vec<&Node> {
    steps(job)
        .iter()
        .filter(|step| uses(step).is_some_and(|u| u.starts_with(ATTEST)))
        .collect()
}

#[test]
fn attest_true_adds_a_pinned_provenance_step_over_the_assets_and_checksums_in_that_job_only()
-> TestResult {
    let (_, tree) = render(&[APP, TOOLS], Some(&attested()))?;
    assert_eq!(tree.get("permissions"), Some(&Node::Map(Vec::new())));
    let sums = job(&tree, "sums-app")?;
    assert_eq!(
        permissions(sums),
        [
            ("attestations".to_owned(), "write".to_owned()),
            ("id-token".to_owned(), "write".to_owned())
        ]
    );
    let found = attest_steps(sums);
    assert_eq!(found.len(), 1);
    let reference = uses(found[0]).ok_or("no uses")?;
    let (_, sha) = reference.split_once('@').ok_or("unpinned")?;
    assert!(sha.len() == 40 && sha.bytes().all(|b| b.is_ascii_hexdigit()));
    let subjects = found[0]
        .at(&["with", "subject-path"])
        .and_then(Node::text)
        .ok_or("no subject-path")?;
    assert_eq!(subjects, "dist/*");
    let upload = steps(sums)
        .iter()
        .position(|step| uses(step).is_some_and(|u| u.starts_with("actions/upload-artifact@")))
        .ok_or("no upload")?;
    let attest = steps(sums)
        .iter()
        .position(|step| uses(step).is_some_and(|u| u.starts_with(ATTEST)))
        .ok_or("no attest")?;
    assert!(attest < upload);
    for (id, other) in jobs(&tree)? {
        if id != "sums-app" {
            assert!(attest_steps(other).is_empty(), "{id}");
            assert!(
                permissions(other)
                    .iter()
                    .all(|(scope, _)| scope != "id-token" && scope != "attestations"),
                "{id}"
            );
        }
    }
    Ok(())
}

#[test]
fn attest_false_or_absent_leaves_the_workflow_unchanged() -> TestResult {
    let (plain, _) = render(&[APP, TOOLS], Some(TWO_BINARIES))?;
    let explicit = TWO_BINARIES.replacen(
        "package = \"app\"\n",
        "package = \"app\"\nattest = false\n",
        1,
    );
    let (off, tree) = render(&[APP, TOOLS], Some(&explicit))?;
    assert_eq!(plain, off);
    assert!(!plain.contains("attest"));
    assert!(!plain.contains("id-token"));
    assert_eq!(
        job(&tree, "sums-app")?.get("permissions"),
        Some(&Node::Map(Vec::new()))
    );
    Ok(())
}
