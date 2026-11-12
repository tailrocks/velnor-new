// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{error::Error, fs, path::Path};
use velnor_actions_contract::WorkflowPolicy;
use velnor_actions_orchestrator::{finalized_jobs, prepare};
use velnor_actions_workflow_renderer::verification_jobs::{
    WorkflowTaskPolicy, build_runner_family_job,
};

fn family_repo(
    policy: WorkflowPolicy,
    branch: &str,
    enabled: bool,
) -> Result<tempfile::TempDir, Box<dyn Error>> {
    let repo =
        super::maintenance_features::owner_feature_repo(policy, false, "default", "default")?;
    let root = repo.path();
    let project = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for name in [
        "mise.toml",
        "mise.lock",
        "rust-toolchain.toml",
        "scripts/generator-release/cargo-git-handoff.py",
        "scripts/generator-release/cargo-source-handoff.sh",
        "scripts/generator-release/source-qualification.sh",
    ] {
        let dest = root.join(name);
        fs::create_dir_all(dest.parent().ok_or("missing parent")?)?;
        fs::copy(project.join(name), dest)?;
    }
    fs::create_dir_all(root.join("scripts"))?;
    fs::write(
        root.join("scripts/check-runner-image-candidate.sh"),
        "#!/bin/sh\nexit 0\n",
    )?;
    let config = root.join(".velnor/config.toml");
    let mut text = fs::read_to_string(&config)?.replace(
        "default_branch = \"testmain\"",
        &format!("default_branch = \"{branch}\""),
    );
    if enabled {
        text.push_str("\n[runner_image_family]\n");
    }
    fs::write(config, text)?;
    Ok(repo)
}

fn family_repo_with_mac(
    policy: WorkflowPolicy,
    branch: &str,
    enabled: bool,
) -> Result<tempfile::TempDir, Box<dyn Error>> {
    let repo = family_repo(policy, branch, false)?;
    if enabled {
        let config = repo.path().join(".velnor/config.toml");
        let mut text = fs::read_to_string(&config)?;
        text.push_str(super::runner_family_selection::FAMILY_CONFIG);
        fs::write(config, text)?;
    }
    Ok(repo)
}

#[test]
fn runner_family_requires_owner_policy_and_main_branch() -> Result<(), Box<dyn Error>> {
    let owner = family_repo_with_mac(WorkflowPolicy::VelnorRepositoryV1, "main", true)?;
    assert!(prepare(owner.path()).is_ok());
    let nonmain = family_repo_with_mac(WorkflowPolicy::VelnorRepositoryV1, "other", true)?;
    let error = prepare(nonmain.path())
        .err()
        .ok_or("non-main family accepted")?;
    assert!(format!("{error:?}").contains("requires_velnor_owner_main"));
    let implicit = family_repo_with_mac(WorkflowPolicy::VelnorRepositoryV1, "main", true)?;
    let config = implicit.path().join(".velnor/config.toml");
    fs::write(
        &config,
        fs::read_to_string(&config)?.replace("default_branch = \"main\"\n", ""),
    )?;
    let git = super::workflow_configuration_fixture::git;
    git(&["config", "user.name", "Fixture"], implicit.path())?;
    git(
        &["config", "user.email", "fixture@example.invalid"],
        implicit.path(),
    )?;
    git(&["add", "."], implicit.path())?;
    git(
        &["-c", "commit.gpgsign=false", "commit", "-m", "fixture"],
        implicit.path(),
    )?;
    git(
        &["update-ref", "refs/remotes/origin/other", "HEAD"],
        implicit.path(),
    )?;
    git(
        &[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/other",
        ],
        implicit.path(),
    )?;
    let error = prepare(implicit.path())
        .err()
        .ok_or("implicit non-main family accepted")?;
    assert!(format!("{error:?}").contains("requires_velnor_owner_main"));
    let consumer = family_repo_with_mac(WorkflowPolicy::ConsumerV1, "main", true)?;
    let error = prepare(consumer.path())
        .err()
        .ok_or("consumer family accepted")?;
    assert!(format!("{error:?}").contains("requires_velnor_owner_main"));
    let plain = family_repo_with_mac(WorkflowPolicy::VelnorRepositoryV1, "other", false)?;
    assert!(prepare(plain.path()).is_ok());
    let plain_consumer = family_repo_with_mac(WorkflowPolicy::ConsumerV1, "other", false)?;
    assert!(prepare(plain_consumer.path()).is_ok());
    Ok(())
}

#[test]
fn native_authority_rejects_forged_runner_family_artifacts() -> Result<(), Box<dyn Error>> {
    let repo = family_repo_with_mac(WorkflowPolicy::VelnorRepositoryV1, "main", true)?;
    let prepared = prepare(repo.path())?;
    assert!(finalized_jobs(&prepared).is_ok());
    for field in [
        "url",
        "checksum",
        "repository_id",
        "owner_id",
        "signer",
        "version",
        "rust_version",
        "specifiers",
        "url_api",
        "provenance",
    ] {
        let mut changed = prepared.clone();
        let policy = changed
            .workflow
            .context
            .workflow_tasks
            .iter_mut()
            .find_map(|p| match p {
                WorkflowTaskPolicy::RunnerFamily(p) => Some(p),
                _ => None,
            })
            .ok_or("family policy missing")?;
        let key = if field == "rust_version" {
            "rust"
        } else {
            "mr-boxington"
        };
        let tool = policy
            .bootstrap
            .selected_tools
            .iter_mut()
            .find(|t| t.key == key)
            .ok_or("tool missing")?;
        match field {
            "version" => tool.version = "1.29.3".into(),
            "rust_version" => tool.version = "1.98.1".into(),
            "specifiers" => tool.specifiers.push("1.29.2".into()),
            _ => {
                let artifact = tool.artifact.as_mut().ok_or("artifact missing")?;
                match field {
                    "url" => artifact.url = "https://github.com/jdx/mr-boxington/releases/download/v1.29.2/other.tar.gz".into(),
                    "checksum" => artifact.checksum = format!("sha256:{}", "a".repeat(64)),
                    "repository_id" => artifact.repository_id = Some("1".into()),
                    "owner_id" => artifact.owner_id = Some("1".into()),
                    "signer" => artifact.signer = Some("sigstore-oidc:https://github.com/jdx/mr-boxington/.github/workflows/other.yml".into()),
                    "url_api" => artifact.url_api = Some("https://api.github.com/repos/jdx/mr-boxington/releases/assets/1".into()),
                    "provenance" => artifact.provenance = Some("https://example.invalid/provenance".into()),
                    _ => unreachable!(),
                }
            }
        }
        let job = build_runner_family_job(policy, &changed.workflow.context.checkout_uses)
            .map_err(|error| format!("{field}: {error:?}"))?;
        changed
            .workflow
            .ir
            .jobs
            .insert("image-runner-family-linux-amd64".into(), job);
        changed.workflow.ir.validate()?;
        let raw = velnor_actions_workflow_renderer::render::render_workflow_ir(
            &changed.workflow.ir,
            WorkflowPolicy::VelnorRepositoryV1,
            changed.workflow.support.as_ref(),
            &changed.workflow.context,
        );
        assert!(
            matches!(raw, Err(velnor_actions_workflow_renderer::RenderError::InvalidWorkflow(ref problem)) if problem == "native_authority_graph"),
            "forged {field}: {raw:?}"
        );
        let final_result = finalized_jobs(&changed);
        assert!(
            matches!(final_result, Err(velnor_actions_orchestrator::OrchestratorError::Render { ref problem }) if problem == "invalid workflow: native_authority_graph"),
            "forged {field}: {final_result:?}"
        );
    }
    Ok(())
}

#[test]
fn runner_family_rejects_missing_mac_qualification_authority() -> Result<(), Box<dyn Error>> {
    let repo = family_repo(WorkflowPolicy::VelnorRepositoryV1, "main", true)?;
    let error = prepare(repo.path())
        .err()
        .ok_or("runner family accepted without Mac qualification authority")?;
    assert!(format!("{error:?}").contains("mac_qualification_required"));
    Ok(())
}

#[test]
fn runner_family_rejects_invalid_mac_authority_pins() -> Result<(), Box<dyn Error>> {
    let repo = family_repo_with_mac(WorkflowPolicy::VelnorRepositoryV1, "main", true)?;
    let config = repo.path().join(".velnor/config.toml");
    let valid = fs::read_to_string(&config)?;
    assert!(
        prepare(repo.path()).is_ok(),
        "valid fixture authority rejected"
    );
    for (from, to) in [
        ("runner_id = 2", "runner_id = 0"),
        ("runner_id = 2", "runner_id = 21"),
        ("runner_id = 2", "runner_id = 22"),
        (
            ".github/workflows/qualify-runner-family.yml",
            ".github/workflows/other.yml",
        ),
        (
            ".github/workflows/qualify-runner-family.yml",
            "../wrong.yml",
        ),
        ("refs/heads/main", "refs/heads/other"),
        ("cccccccccccccccccccccccccccccccccccccccc", "not-a-sha"),
        ("unix:///tmp/fixture.sock", "tcp://example.invalid"),
        ("1.2.3", "1.2"),
        ("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "not-a-sha"),
        (
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            "not-a-digest",
        ),
    ] {
        fs::write(&config, valid.replacen(from, to, 1))?;
        let error = prepare(repo.path())
            .err()
            .ok_or("invalid Mac authority accepted")?;
        assert!(format!("{error:?}").contains("mac_qualification_invalid"));
    }
    Ok(())
}

#[test]
fn runner_family_requires_canonical_persistent_mac_paths() -> Result<(), Box<dyn Error>> {
    let repo = family_repo_with_mac(WorkflowPolicy::VelnorRepositoryV1, "main", true)?;
    let config = repo.path().join(".velnor/config.toml");
    let valid = fs::read_to_string(&config)?;
    assert!(
        prepare(repo.path()).is_ok(),
        "absolute Mac state directory rejected"
    );
    for (from, to) in [
        ("/tmp/fixture-state", "relative-state"),
        ("/tmp/fixture-state", "/tmp/../state"),
        ("unix:///tmp/fixture.sock", "unix:///tmp/../fixture.sock"),
        ("unix:///tmp/fixture.sock", "unix:////tmp/fixture.sock"),
    ] {
        fs::write(&config, valid.replacen(from, to, 1))?;
        let error = prepare(repo.path())
            .err()
            .ok_or("noncanonical Mac path accepted")?;
        assert!(format!("{error:?}").contains("mac_qualification_invalid"));
    }
    Ok(())
}
