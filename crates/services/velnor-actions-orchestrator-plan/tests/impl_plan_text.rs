//! Deterministic plan text over a consumer preparation.

use std::collections::BTreeMap;

use velnor_actions_orchestrator_generation::finalized::finalized_jobs;
use velnor_actions_orchestrator_generation::prepare::{GenerationPreparation, prepare};
use velnor_actions_orchestrator_plan::plan::{plan_text, plan_text_checked};

use crate::impl_common::TestResult;

/// Consumer fixture root carrying config plus the release install receipt.
fn consumer_prep() -> Result<(tempfile::TempDir, GenerationPreparation), Box<dyn std::error::Error>>
{
    let root = tempfile::TempDir::new()?;
    std::fs::create_dir(root.path().join(".velnor"))?;
    std::fs::write(
        root.path().join(".velnor/config.toml"),
        "schema = 1\n[workflow]\npolicy = 'consumer-v1'\ndefault_branch = 'main'\n",
    )?;
    std::fs::write(
        root.path().join(".velnor/release-manifest.json"),
        include_str!("../../../../fixtures/consumer-release-manifest.json"),
    )?;
    let prep = prepare(root.path())?;
    Ok((root, prep))
}

#[test]
fn checked_plan_renders_header_branch_and_runner() -> TestResult {
    let (_root, prep) = consumer_prep()?;
    let text = plan_text_checked(&prep).map_err(|err| format!("plan text: {err}"))?;
    assert!(
        text.contains(&format!(
            "Velnor Actions plan {}",
            env!("CARGO_PKG_VERSION")
        )),
        "{text}"
    );
    assert!(text.contains("Push branch: main"), "{text}");
    assert!(
        text.contains("Runner: ubuntu-26.04 (latest pinned default)"),
        "{text}"
    );
    Ok(())
}

#[test]
fn checked_plan_lists_finalized_jobs() -> TestResult {
    let (_root, prep) = consumer_prep()?;
    let text = plan_text_checked(&prep).map_err(|err| format!("plan text: {err}"))?;
    assert!(text.contains("    - plan ("), "{text}");
    assert!(text.contains("    - required ("), "{text}");
    Ok(())
}

#[test]
fn plan_text_is_deterministic() -> TestResult {
    let (_root, prep) = consumer_prep()?;
    let jobs = finalized_jobs(&prep).map_err(|err| format!("finalized jobs: {err}"))?;
    assert_eq!(plan_text(&prep, &jobs), plan_text(&prep, &jobs));
    Ok(())
}

#[test]
fn plan_text_without_jobs_keeps_header() -> TestResult {
    let (_root, prep) = consumer_prep()?;
    let text = plan_text(&prep, &BTreeMap::new());
    assert!(
        text.contains(&format!(
            "Velnor Actions plan {}",
            env!("CARGO_PKG_VERSION")
        )),
        "{text}"
    );
    assert!(text.contains("Workflow to generate"), "{text}");
    Ok(())
}
