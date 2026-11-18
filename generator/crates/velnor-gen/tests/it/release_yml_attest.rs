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
                    .all(|(scope, access)| scope != "id-token"
                        && (scope != "attestations" || (id == "publish-app" && access == "read"))),
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

#[test]
fn attest_true_verifies_every_asset_with_a_pinned_gh_before_publish_and_fails_closed() -> TestResult
{
    let (_, tree) = render(&[APP, TOOLS], Some(&attested()))?;
    let publish = job(&tree, "publish-app")?;
    assert_eq!(
        permissions(publish),
        [
            ("attestations".to_owned(), "read".to_owned()),
            ("contents".to_owned(), "write".to_owned())
        ]
    );
    let all = steps(publish);
    let run_of = |step: &Node| step.get("run").and_then(Node::text).map(str::to_owned);
    let verifying: Vec<usize> = (0..all.len())
        .filter(|i| run_of(&all[*i]).is_some_and(|r| r.contains("gh attestation verify")))
        .collect();
    let published = (0..all.len())
        .find(|i| run_of(&all[*i]).is_some_and(|r| r.contains("release publish")))
        .ok_or("no publish step")?;
    assert!(verifying.iter().all(|i| *i < published));
    let commands: Vec<String> = verifying.iter().filter_map(|i| run_of(&all[*i])).collect();
    for subject in ["dist/app-linux-x64.tar.gz", "dist/SHA256SUMS"] {
        assert!(
            commands.iter().any(|c| c.contains(subject)),
            "{subject} is not verified"
        );
    }
    for run in &commands {
        assert!(!run.contains("||") && !run.contains("; true"));
        for flag in [
            "--repo \"$GITHUB_REPOSITORY\"",
            "--source-digest \"$GITHUB_SHA\"",
            "--source-ref \"$GITHUB_REF\"",
            "--signer-workflow \"$GITHUB_REPOSITORY/.github/workflows/release.yml\"",
            "--signer-digest \"$GITHUB_SHA\"",
        ] {
            assert!(run.contains(flag), "{flag}");
        }
        assert!(run.contains("mise --no-config --no-env --no-hooks exec gh@2."));
    }
    for (id, other) in jobs(&tree)? {
        if id != "publish-app" {
            assert!(
                permissions(other)
                    .iter()
                    .all(|(scope, _)| scope != "attestations" || id == "sums-app"),
                "{id}"
            );
        }
    }
    Ok(())
}

#[test]
fn attest_off_publish_job_has_no_verification() -> TestResult {
    let (plain, tree) = render(&[APP, TOOLS], Some(TWO_BINARIES))?;
    assert!(!plain.contains("attestation verify"));
    assert_eq!(
        permissions(job(&tree, "publish-app")?),
        [("contents".to_owned(), "write".to_owned())]
    );
    Ok(())
}

#[test]
fn attest_with_a_versioned_asset_name_verifies_the_expanded_names_after_reading_the_version()
-> TestResult {
    let toml = "[[binaries]]\npackage = \"app\"\nattest = true\nasset_name = \"{{ bin }}-{{ version }}-{{ platform }}\"\n";
    let (_, tree) = render(&[APP], Some(toml))?;
    let publish = job(&tree, "publish-app")?;
    let all = steps(publish);
    let run_of = |step: &Node| step.get("run").and_then(Node::text).map(str::to_owned);
    let reader = (0..all.len())
        .find(|i| run_of(&all[*i]).is_some_and(|r| r.contains("VERSION=")))
        .ok_or("no version step")?;
    let verifying: Vec<(usize, String)> = (0..all.len())
        .filter_map(|i| run_of(&all[i]).map(|run| (i, run)))
        .filter(|(_, run)| run.contains("gh attestation verify"))
        .collect();
    assert_eq!(verifying.len(), 4);
    assert!(verifying.iter().all(|(i, _)| reader < *i));
    let archive = verifying
        .iter()
        .map(|(_, run)| run)
        .find(|run| run.contains("tar.gz"))
        .ok_or("no archive verification")?;
    assert!(
        archive.contains(" \"dist/app-${VERSION}-linux-x64.tar.gz\" --repo"),
        "{archive}"
    );
    for (_, run) in &verifying {
        assert!(!run.contains('\''), "{run}");
    }
    for step in all {
        let name = step.get("name").and_then(Node::text).unwrap_or_default();
        assert!(!name.contains('"') && !name.contains('\''), "{name}");
    }
    Ok(())
}
