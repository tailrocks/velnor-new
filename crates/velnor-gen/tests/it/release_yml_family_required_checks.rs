// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_gen::GenerateError;
use velnor_release::{FamilyError, ReleaseError};

use crate::release_family_fixture::{HELPER, generate, refused, tools_like};
use crate::release_yml_binaries::{job, permissions, runs};
use crate::yaml_tree::Node;

type TestResult = Result<(), Box<dyn Error>>;

const TAG_PUSH: &str = "family_trigger = \"tag_push\"\ncommit_file = true\nattest = true\n";
const CHECKS_OFF: &str = "family_required_checks = false\n";

fn config(extra: &str) -> String {
    tools_like(&format!("{extra}{TAG_PUSH}"))
}

#[test]
fn the_default_gate_waits_for_the_required_checks() -> TestResult {
    for extra in ["", "family_required_checks = true\n"] {
        let (_, tree) = generate(HELPER, &config(extra))?;
        let gate = job(&tree, "gate")?;
        assert!(
            runs(gate)
                .iter()
                .any(|run| run.starts_with("velnor-ci release-gate"))
        );
        assert!(
            runs(gate)
                .iter()
                .any(|run| run.contains("compare/$DEFAULT_BRANCH...$COMMIT"))
        );
        assert_eq!(
            gate.at(&["outputs", "ready"]).and_then(Node::text),
            Some("${{ steps.gate.outputs.ready }}")
        );
    }
    Ok(())
}

#[test]
fn without_the_required_checks_the_gate_keeps_the_default_branch_check_only() -> TestResult {
    let (_, tree) = generate(HELPER, &config(CHECKS_OFF))?;
    let gate = job(&tree, "gate")?;
    let commands = runs(gate);
    assert_eq!(commands.len(), 1, "{commands:?}");
    assert!(commands[0].contains("compare/$DEFAULT_BRANCH...$COMMIT"));
    assert_eq!(
        gate.at(&["outputs", "ready"]).and_then(Node::text),
        Some("true")
    );
    assert_eq!(
        permissions(gate),
        [("contents".to_owned(), "read".to_owned())]
    );
    Ok(())
}

#[test]
fn without_the_required_checks_the_tag_binding_attestation_and_verification_stay() -> TestResult {
    let (text, tree) = generate(HELPER, &config(CHECKS_OFF))?;
    let family = runs(job(&tree, "family")?);
    assert!(
        family
            .iter()
            .any(|run| run.starts_with("velnor-ci release family-tag")
                && run.contains("--pushed-tag=\"$PUSHED_TAG\""))
    );
    assert!(text.contains("actions/attest-build-provenance@"));
    let publish = job(&tree, "publish-family")?;
    assert!(
        publish
            .get("needs")
            .map(Node::texts)
            .is_some_and(|needs| needs.contains(&"verify-family"))
    );
    assert!(
        runs(job(&tree, "verify-family")?)
            .iter()
            .any(|run| run.contains("gh attestation verify"))
    );
    let default = generate(HELPER, &config(""))?.1;
    for id in ["family", "sums-family", "verify-family", "publish-family"] {
        assert_eq!(
            job(&tree, id)?.get("steps"),
            job(&default, id)?.get("steps")
        );
    }
    Ok(())
}

#[test]
fn the_key_needs_the_tag_push_family() -> TestResult {
    let error = refused("family_required_checks = false\n[[binaries]]\npackage = \"app\"\n")?;
    assert!(
        matches!(
            error,
            GenerateError::Release(ReleaseError::Family(FamilyError::NeedsFamilyTag {
                key: "family_required_checks"
            }))
        ),
        "{error}"
    );
    let error = refused(&tools_like(CHECKS_OFF))?;
    assert!(
        matches!(
            error,
            GenerateError::Release(ReleaseError::Family(FamilyError::RequiredChecksNeedTagPush))
        ),
        "{error}"
    );
    Ok(())
}

#[test]
fn the_key_conflicts_with_pr_only_checks() -> TestResult {
    let error = refused(&config(&format!(
        "{CHECKS_OFF}pr_only_checks = [\"Rust CI\"]\n"
    )))?;
    assert!(
        matches!(
            error,
            GenerateError::Release(ReleaseError::Family(
                FamilyError::RequiredChecksOffWithPrOnlyChecks
            ))
        ),
        "{error}"
    );
    Ok(())
}
