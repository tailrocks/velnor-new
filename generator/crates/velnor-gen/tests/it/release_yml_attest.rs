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
fn attest_with_a_versioned_asset_name_quotes_and_binds_the_version_in_every_verification()
-> TestResult {
    let toml = "[[binaries]]\npackage = \"app\"\nplatforms = [\"linux-x64\", \"macos-arm64\"]\nattest = true\nasset_name = \"{{ bin }}-{{ version }}-{{ target }}\"\n";
    let (_, tree) = render(&[APP], Some(toml))?;
    let publish = job(&tree, "publish-app")?;
    let all = steps(publish);
    let verifying: Vec<&Node> = all
        .iter()
        .filter(|step| {
            step.get("run")
                .and_then(Node::text)
                .is_some_and(|run| run.contains("gh attestation verify"))
        })
        .collect();
    assert_eq!(verifying.len(), 3, "two archives and SHA256SUMS");
    let commands: Vec<&str> = verifying
        .iter()
        .filter_map(|step| step.get("run").and_then(Node::text))
        .collect();
    for subject in [
        "\"dist/app-${VERSION}-x86_64-unknown-linux-musl.tar.gz\"",
        "\"dist/app-${VERSION}-aarch64-apple-darwin.tar.gz\"",
        "\"dist/SHA256SUMS\"",
    ] {
        assert!(
            commands
                .iter()
                .any(|run| run.contains(&format!("verify {subject} --repo"))),
            "{subject} is not verified in {commands:?}"
        );
    }
    for step in &verifying {
        let versioned = step
            .get("run")
            .and_then(Node::text)
            .is_some_and(|run| run.contains("${VERSION}"));
        assert_eq!(
            step.at(&["env", "VERSION"]).and_then(Node::text),
            versioned.then_some("${{ steps.version.outputs.version }}")
        );
    }
    let reader = all
        .iter()
        .position(|step| step.get("id").and_then(Node::text) == Some("version"))
        .ok_or("no version step")?;
    let first = all
        .iter()
        .position(|step| {
            step.get("run")
                .and_then(Node::text)
                .is_some_and(|run| run.contains("gh attestation verify"))
        })
        .ok_or("no verify")?;
    assert!(reader < first, "the version is read before it is used");
    Ok(())
}

#[test]
fn attest_with_a_raw_binary_verifies_the_executable_and_its_sidecars() -> TestResult {
    let toml = "[[binaries]]\npackage = \"app\"\nformat = \"raw\"\nattest = true\n";
    let (_, tree) = render(&[APP], Some(toml))?;
    let verified: String = steps(job(&tree, "publish-app")?)
        .iter()
        .filter_map(|step| step.get("run").and_then(Node::text))
        .filter(|run| run.contains("gh attestation verify"))
        .collect::<Vec<_>>()
        .join("\n");
    for subject in [
        "dist/app-linux-x64 ",
        "dist/app-linux-x64.sha256 ",
        "dist/app-linux-x64.provenance.json ",
        "dist/SHA256SUMS ",
    ] {
        assert!(verified.contains(subject), "{subject} is not verified");
    }
    Ok(())
}
