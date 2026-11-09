// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::workflow_configuration_fixture::{git, make_repo};
use std::{error::Error, fs};
use velnor_actions_orchestrator::{GenerateOptions, generate, prepare, render_staged_tree};
use velnor_actions_workflow_renderer::premerge_event_proof::WORKFLOW_PATH;

const BASE: &str = "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n[stacks]\nignore = [\"rust\"]\n";
const PRODUCER: &str = "\n[premerge_event_proof]\nenvironment = \"premerge-proof\"\nrepository_id = 7\nowner_id = 11\nreviewer_id = 60\ncaller_workflow = \".github/workflows/ci.yml\"\n";

#[test]
fn producer_generation_preserves_policy_and_removes_disabled_authority()
-> Result<(), Box<dyn Error>> {
    let repository = make_repo(&format!("{BASE}{PRODUCER}"))?;
    let root = repository.path();
    git(
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/example/project.git",
        ],
        root,
    )?;
    let prepared = prepare(root)?;
    let tree = render_staged_tree(&prepared)?;
    let producer = tree.get(WORKFLOW_PATH).ok_or("producer absent")?.to_owned();
    assert!(producer.contains("example/project"));
    assert!(producer.contains("testmain"));
    assert!(producer.contains("VELNOR_PROOF_REVIEWER_ID"));
    assert_eq!(
        render_staged_tree(&prepare(root)?)?.get(WORKFLOW_PATH),
        Some(producer.as_str())
    );
    generate(&prepared, &GenerateOptions::default())?;
    assert_eq!(fs::read_to_string(root.join(WORKFLOW_PATH))?, producer);
    fs::write(root.join(".velnor/config.toml"), BASE)?;
    let disabled = prepare(root)?;
    assert!(render_staged_tree(&disabled)?.get(WORKFLOW_PATH).is_none());
    generate(&disabled, &GenerateOptions::default())?;
    assert!(!root.join(WORKFLOW_PATH).exists());
    fs::write(
        root.join(".velnor/config.toml"),
        format!(
            "{BASE}{}",
            PRODUCER.replace("reviewer_id = 60", "reviewer_id = 0")
        ),
    )?;
    assert!(prepare(root).is_err());
    fs::write(
        root.join(".velnor/config.toml"),
        format!(
            "{BASE}{}",
            PRODUCER.replace("reviewer_id = 60", "reviewer_id = 61")
        ),
    )?;
    let changed = render_staged_tree(&prepare(root)?)?;
    assert_ne!(changed.get(WORKFLOW_PATH), Some(producer.as_str()));
    git(
        &[
            "remote",
            "set-url",
            "origin",
            "https://example.invalid/project",
        ],
        root,
    )?;
    assert!(render_staged_tree(&prepare(root)?).is_err());
    git(
        &[
            "remote",
            "set-url",
            "origin",
            "https://github.com/example/project.git",
        ],
        root,
    )?;
    #[cfg(unix)]
    reject_output_symlink(root)?;
    Ok(())
}

#[cfg(unix)]
fn reject_output_symlink(root: &std::path::Path) -> Result<(), Box<dyn Error>> {
    let target = root.join("untouched");
    fs::write(&target, "untouched")?;
    std::os::unix::fs::symlink(&target, root.join(WORKFLOW_PATH))?;
    generate(&prepare(root)?, &GenerateOptions::default())?;
    assert!(!fs::symlink_metadata(root.join(WORKFLOW_PATH))?.is_symlink());
    assert_eq!(fs::read_to_string(target)?, "untouched");
    Ok(())
}
