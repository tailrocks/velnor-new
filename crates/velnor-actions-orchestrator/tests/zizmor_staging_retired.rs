//! Retired generated-path cleanup and deterministic policy parity.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use tempfile::TempDir;
use velnor_actions_contract::WorkflowPolicy;
use velnor_actions_orchestrator::{GenerateOptions, generate, plan_text_checked, prepare};

use super::{TestResult, make_policy_repo, make_repo};

/// Historical input from PR12 8625f5d; blob a29b61ff997ddb403d57a0457f83b932247c080c,
/// SHA-256 c777fb6c4837da2fd38fe4ac68b5235b759fb628de68a22c6adc14153ba086a3.
/// Test-only seed proving an obsolete managed workflow is removed.
const RETIRED_WORKFLOW: &str = include_str!("fixtures/retired-foundation-workflow.yml");
const RETIRED_PATH: &str = ".github/workflows/foundation-qualification.yml";
const ACTION_PIN: &str = "8758d976a1b25eb387f48aa04ea86f57739b84cf";
const HAND_PATH: &str = ".github/hand-maintained/controls.md";
const HAND_BYTES: &[u8] = b"keep these repository-owned bytes\n";

#[test]
fn retired_workflow_cleanup_and_both_policy_generators_are_deterministic() -> TestResult {
    velnor_cleanup_is_deterministic()?;
    consumer_generation_is_deterministic()
}

fn velnor_cleanup_is_deterministic() -> TestResult {
    let repository = make_policy_repo()?;
    let root = repository.path();
    let preparation = prepare(root)?;
    assert_eq!(
        preparation.config.workflow.policy,
        WorkflowPolicy::VelnorRepositoryV1
    );
    let plan = plan_text_checked(&preparation)?;
    assert!(
        !plan.contains(RETIRED_PATH),
        "retired path absent from plan"
    );
    assert!(!plan.contains(ACTION_PIN), "retired pin absent from plan");

    let stale = root.join(RETIRED_PATH);
    let stale_parent = stale.parent().ok_or("workflow path has parent")?;
    fs::create_dir_all(stale_parent)?;
    fs::write(&stale, RETIRED_WORKFLOW)?;
    let hand_file = root.join(HAND_PATH);
    fs::create_dir_all(hand_file.parent().ok_or("hand path has parent")?)?;
    fs::write(&hand_file, HAND_BYTES)?;

    let first = generate(&preparation, &GenerateOptions::default())?;
    assert_eq!(
        first.files_written,
        [
            ".github/AGENTS.md",
            ".github/CLAUDE.md",
            ".github/actionlint.yaml",
            ".github/actions/velnor-tool-seed/action.yml",
            ".github/workflows/ci.yml",
            ".github/workflows/freshness.yml",
        ]
        .map(str::to_owned)
    );
    assert!(
        !stale.exists(),
        "real Velnor generation removes retired workflow"
    );
    assert_eq!(fs::read(&hand_file)?, HAND_BYTES, "hand file survives");
    let first_bytes = generated_bytes(root, &first.files_written)?;
    assert_no_foundation(&first.files_written, &first_bytes);

    let second = generate(&preparation, &GenerateOptions::default())?;
    assert_eq!(first.files_written, second.files_written);
    assert_eq!(
        first_bytes,
        generated_bytes(root, &second.files_written)?,
        "Velnor generate is byte-identical on repeat"
    );
    assert!(
        !stale.exists(),
        "repeat generation keeps retired path absent"
    );
    assert_eq!(
        fs::read(&hand_file)?,
        HAND_BYTES,
        "hand file stays byte-identical"
    );

    Ok(())
}

fn consumer_generation_is_deterministic() -> TestResult {
    let consumer = make_repo(
        "schema = 1\n[workflow]\nname = \"CI\"\npolicy = \"consumer-v1\"\ndefault_branch = \"testmain\"\n",
    )?;
    let consumer_prep = prepare(consumer.path())?;
    assert_eq!(
        consumer_prep.config.workflow.policy,
        WorkflowPolicy::ConsumerV1
    );
    let consumer_plan = plan_text_checked(&consumer_prep)?;
    assert!(!consumer_plan.contains(RETIRED_PATH));
    assert!(!consumer_plan.contains(ACTION_PIN));

    let first_parent = TempDir::new()?;
    let first_preview = first_parent.path().join("preview");
    let consumer_first = generate(
        &consumer_prep,
        &GenerateOptions {
            output_dir: Some(first_preview.clone()),
        },
    )?;
    assert_eq!(
        consumer_first.files_written,
        [
            ".github/AGENTS.md",
            ".github/CLAUDE.md",
            ".github/actionlint.yaml",
            ".github/actions/velnor-tool-seed/action.yml",
            ".github/workflows/ci.yml",
        ]
        .map(str::to_owned)
    );
    let consumer_first_bytes = generated_bytes(&first_preview, &consumer_first.files_written)?;
    assert_no_foundation(&consumer_first.files_written, &consumer_first_bytes);
    assert!(!first_preview.join(RETIRED_PATH).exists());

    let second_parent = TempDir::new()?;
    let second_preview = second_parent.path().join("preview");
    let consumer_second = generate(
        &consumer_prep,
        &GenerateOptions {
            output_dir: Some(second_preview.clone()),
        },
    )?;
    assert_eq!(consumer_first.files_written, consumer_second.files_written);
    assert_eq!(
        consumer_first_bytes,
        generated_bytes(&second_preview, &consumer_second.files_written)?,
        "consumer generate is byte-identical on repeat"
    );
    assert!(!second_preview.join(RETIRED_PATH).exists());
    Ok(())
}

fn generated_bytes(
    root: &Path,
    paths: &[String],
) -> Result<BTreeMap<String, Vec<u8>>, Box<dyn std::error::Error>> {
    paths
        .iter()
        .map(|path| Ok((path.clone(), fs::read(root.join(path))?)))
        .collect()
}

fn assert_no_foundation(paths: &[String], files: &BTreeMap<String, Vec<u8>>) {
    assert!(
        paths
            .iter()
            .all(|path| !path.contains("foundation-qualification")),
        "retired workflow path absent: {paths:?}"
    );
    for (path, bytes) in files {
        assert!(
            !String::from_utf8_lossy(bytes).contains(ACTION_PIN),
            "retired action pin absent from generated {path}"
        );
    }
}
