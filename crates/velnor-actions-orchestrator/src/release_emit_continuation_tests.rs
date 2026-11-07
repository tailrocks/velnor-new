// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{TestResult, check_authority, commit, fs, preview_fixture, release};

#[test]
fn same_source_predecessor_renders_workspace_dispatch_from_historical_source() -> TestResult<()> {
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    preview_fixture(root)?;
    fs::remove_dir_all(root.join("src"))?;
    for name in ["alpha", "beta"] {
        fs::create_dir_all(root.join(name).join("src"))?;
        fs::write(
            root.join(name).join("Cargo.toml"),
            format!("[package]\nname='{name}'\nversion='0.1.0'\nedition='2021'\n"),
        )?;
        fs::write(root.join(name).join("src/lib.rs"), "pub fn fixture() {}\n")?;
    }
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers=['alpha','beta']\nresolver='2'\n",
    )?;
    fs::write(
        root.join("Cargo.lock"),
        "version = 4\n\n[[package]]\nname='alpha'\nversion='0.1.0'\n\n[[package]]\nname='beta'\nversion='0.1.0'\n",
    )?;
    let source = commit(root, "workspace release source")?;
    let path = root.join(".velnor/config.toml");
    let config =
        fs::read_to_string(&path)?.replace("packages = ['demo']", "packages = ['alpha', 'beta']");
    fs::write(
        &path,
        format!(
            "{config}\ncontinuation_source_sha='{source}'\n[stacks.rust.release.journal_predecessor]\nrun_id=42\nrun_attempt=1\nworkflow_sha='{}'\n",
            "a".repeat(40)
        ),
    )?;
    fs::write(
        root.join("beta/Cargo.toml"),
        "[package]\nname='beta'\nversion='0.2.0'\nedition='2021'\n",
    )?;
    fs::write(
        root.join("Cargo.lock"),
        "version = 4\n\n[[package]]\nname='alpha'\nversion='0.1.0'\n\n[[package]]\nname='beta'\nversion='0.2.0'\n",
    )?;
    let authority = commit(root, "review continuation policy")?;
    let prep = crate::planner::prepare::prepare_for_candidate(root)?;
    let release_config = release::enabled_release(&prep)?.ok_or("release missing")?;
    let derived = release::derive_release_inputs(&prep, release_config)?;
    assert_eq!(derived.source_sha, source);
    assert_eq!(
        derived
            .selection
            .packages
            .iter()
            .map(|p| (p.name.as_str(), p.version.as_str()))
            .collect::<Vec<_>>(),
        [("alpha", "0.1.0"), ("beta", "0.1.0")]
    );
    let mise =
        crate::planner::pins::resolve_mise_setup(&prep.config, release::RELEASE_HOSTED_RUNNER)?;
    let files = release::release_files_for_config(&prep, &mise, release_config)?;
    let workflow = files
        .iter()
        .find(|f| f.path == ".github/workflows/release.yml")
        .ok_or("workflow missing")?;
    assert!(workflow.bytes.contains("workflow_dispatch"));
    assert!(workflow.bytes.contains(&source));
    check_authority(&derived.intent, &authority, &source)?;
    let mut prior_intent = derived.intent.clone();
    prior_intent.journal_predecessor = None;
    prior_intent.continuation_source_sha = None;
    let mut without_source = derived.intent.clone();
    without_source.continuation_source_sha = None;
    assert_ne!(
        velnor_actions_contract::canonical_json_bytes(&derived.intent)?,
        velnor_actions_contract::canonical_json_bytes(&without_source)?
    );
    assert_eq!(
        crate::crate_release_journal::release_base_intent_sha256(&derived.intent)?,
        crate::crate_release_journal::release_base_intent_sha256(&prior_intent)?
    );
    Ok(())
}
