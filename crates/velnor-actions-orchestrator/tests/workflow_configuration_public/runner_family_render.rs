// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{error::Error, fs};
use velnor_actions_contract::{PermissionLevel, WorkflowPolicy};
use velnor_actions_orchestrator::{finalized_jobs, prepare, render_staged_tree};

fn enable_family(config: &mut String) {
    *config = config.replace("default_branch = \"testmain\"", "default_branch = \"main\"");
    config.push_str("\n[runner_image_family]\n");
}

#[test]
fn runner_family_job_is_plan_gated_and_in_required_fanin() -> Result<(), Box<dyn Error>> {
    let repo = super::maintenance_features::owner_feature_repo(
        WorkflowPolicy::VelnorRepositoryV1,
        false,
        "default",
        "default",
    )?;
    let root = repo.path();
    super::runner_family_selection::copy_locked_tools(root)?;
    fs::create_dir_all(root.join("scripts"))?;
    let script = fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../scripts/check-runner-image-candidate.sh"),
    )?;
    fs::write(
        root.join("scripts/check-runner-image-candidate.sh"),
        &script,
    )?;
    let config_path = root.join(".velnor/config.toml");
    let mut config = fs::read_to_string(&config_path)?;
    enable_family(&mut config);
    fs::write(config_path, config)?;
    let prepared = prepare(root)?;
    let jobs = finalized_jobs(&prepared)?;
    let id = "image-runner-family-linux-amd64";
    let job = jobs.get(id).ok_or("runner family job missing")?;
    assert_job(job, &jobs, id)?;
    let tree = render_staged_tree(&prepared)?;
    let ci = tree
        .get(velnor_actions_workflow_renderer::render::WORKFLOW_PATH)
        .ok_or("workflow missing")?;
    let family = super::support::rendered_job_block(ci, id)?;
    assert_family(job, family, &script)?;
    super::runner_family_guard::assert_actual_guard(job, root, &script)?;
    super::runner_family_guard::assert_prefetch(job)?;
    super::runner_family_guard::assert_binding_rejections(&prepared, job)?;
    super::runner_family_guard::assert_evidence_symlink_rejected(job, root)?;
    assert_upload(job)?;
    assert!(family.contains("mise --no-env --locked --no-hooks install"));
    assert!(
        family.contains("lockfile_version = 3"),
        "selected lock format loses repository identity"
    );
    assert!(
        family.contains("specifiers ="),
        "selected lock omits request bindings"
    );
    assert!(
        family.contains("1340721957"),
        "locked MBX repository identity was dropped"
    );
    assert!(
        family.contains("release.yml@refs/heads/main"),
        "locked MBX workflow was dropped"
    );
    let required = super::support::rendered_job_block(ci, "required")?;
    let expected = required
        .lines()
        .find(|line| line.contains("VELNOR_NEEDS_EXPECTED"))
        .ok_or("required report inventory missing")?;
    assert!(
        expected.contains(id),
        "required report does not bind runner family"
    );
    Ok(())
}

fn assert_job(
    job: &velnor_actions_contract::Job,
    jobs: &std::collections::BTreeMap<String, velnor_actions_contract::Job>,
    id: &str,
) -> Result<(), Box<dyn Error>> {
    assert_eq!(job.runs_on, "ubuntu-26.04");
    assert_eq!(job.needs, ["plan"]);
    assert!(
        job.condition.as_deref().is_some_and(
            |condition| condition.contains("not_affected_jobs") && condition.contains(id)
        )
    );
    let permissions = job.permissions.as_ref().ok_or("permissions missing")?;
    assert_eq!(permissions.contents, PermissionLevel::Read);
    assert_eq!(permissions.id_token, PermissionLevel::None);
    assert_eq!(permissions.attest, PermissionLevel::None);
    assert_eq!(permissions.packages, PermissionLevel::None);
    assert_eq!(permissions.actions, PermissionLevel::None);
    assert_eq!(permissions.pull_requests, PermissionLevel::None);
    assert!(
        jobs.get("required")
            .ok_or("required job missing")?
            .needs
            .iter()
            .any(|need| need == id)
    );
    Ok(())
}

fn assert_family(
    job: &velnor_actions_contract::Job,
    family: &str,
    script: &[u8],
) -> Result<(), Box<dyn Error>> {
    assert!(family.contains("check-runner-image-candidate.sh"));
    assert!(
        family.contains("Restore Cargo sources"),
        "rendered family does not restore sources"
    );
    assert!(
        !family.contains("Restore Mise tools"),
        "rendered family restores an unused global tool home"
    );
    assert!(
        family.contains(&super::runner_family_source::digest(script)),
        "rendered task does not bind the actual producer script"
    );
    assert!(
        job.steps
            .iter()
            .any(|step| step.role == Some(velnor_actions_contract::StepRole::CargoSourcesRestore)),
        "family does not restore Cargo sources"
    );
    assert!(
        !job.steps
            .iter()
            .any(|step| step.role == Some(velnor_actions_contract::StepRole::ToolsCacheRestore)),
        "family restores an unused global tools home"
    );
    assert!(
        family.contains("$RUNNER_TEMP/velnor/cargo"),
        "family does not consume the restored Cargo home"
    );
    assert!(family.contains(r#"export CARGO_HOME=\"$RUNNER_TEMP/velnor/cargo\" MISE_CARGO_HOME=\"$RUNNER_TEMP/velnor/cargo\""#), "family Cargo execution does not use restored sources");
    let install = job
        .steps
        .iter()
        .position(|step| step.name == "Install declared prebuilt task tools")
        .ok_or("install step missing")?;
    let target = job
        .steps
        .iter()
        .position(|step| step.name == "Install pinned Linux musl target")
        .ok_or("target step missing")?;
    let run = job
        .steps
        .iter()
        .position(|step| step.name == "Run runner image candidate")
        .ok_or("task step missing")?;
    let verifies = job
        .steps
        .iter()
        .enumerate()
        .filter_map(|(index, step)| {
            (step.role == Some(velnor_actions_contract::StepRole::VerifyRustManifest))
                .then_some(index)
        })
        .collect::<Vec<_>>();
    assert_eq!(
        verifies.len(),
        2,
        "both installed toolchains must be verified"
    );
    assert!(
        install < verifies[0] && verifies[0] < target && target < verifies[1] && verifies[1] < run,
        "Rust verification must guard both target installation and task execution"
    );
    for index in [verifies[0], target, verifies[1], run] {
        assert!(
            job.steps[index].condition.is_none(),
            "failed verification must block later steps"
        );
    }
    Ok(())
}
fn assert_upload(job: &velnor_actions_contract::Job) -> Result<(), Box<dyn Error>> {
    let upload = job
        .steps
        .iter()
        .find(|step| step.name == "Upload runner image family")
        .ok_or("family upload missing")?;
    assert_eq!(
        upload.condition.as_deref(),
        Some("github.event_name == 'push' && github.ref == 'refs/heads/main'")
    );
    let velnor_actions_contract::StepKind::Action { with, .. } = &upload.kind else {
        return Err("family upload is not an action".into());
    };
    assert_eq!(
        with.get("name").map(String::as_str),
        Some("image-assets-${{ github.run_attempt }}")
    );
    assert_eq!(with.get("compression-level").map(String::as_str), Some("0"));
    let paths = with.get("path").ok_or("family upload paths missing")?;
    for name in [
        "velnor-runner-linux-amd64.tar",
        "velnor-dind-linux-amd64.tar",
        "velnor-resource-probe-linux-amd64.tar",
        "runner-image-provenance.json",
        "dind-image-provenance.json",
        "resource-probe-image-provenance.json",
        "runner-image-publication.json",
        "dind-image-publication.json",
        "resource-probe-image-publication.json",
        "resource_probe",
        "storage_init",
        "job_start_hook",
        "job_started_hook",
    ] {
        assert!(
            paths
                .lines()
                .any(|path| path.ends_with(&format!("/{name}"))),
            "family upload omits {name}"
        );
    }
    assert_eq!(paths.lines().count(), 13);
    Ok(())
}
#[test]
fn runner_family_rejects_untrusted_cargo_wrapper_configuration() -> Result<(), Box<dyn Error>> {
    for (old, new) in [
        ("command = \"mbx\"", "command = \"other\""),
        ("MBX_CARGO_SHIM_MODE = \"1\"", "MBX_CARGO_SHIM_MODE = \"0\""),
        ("command = \"mbx\"", "command = \"mbx\"\nextra = true"),
    ] {
        let repo = super::maintenance_features::owner_feature_repo(
            WorkflowPolicy::VelnorRepositoryV1,
            false,
            "default",
            "default",
        )?;
        let root = repo.path();
        super::runner_family_selection::copy_locked_tools(root)?;
        let config_path = root.join(".velnor/config.toml");
        let mut config = fs::read_to_string(&config_path)?;
        enable_family(&mut config);
        fs::write(config_path, config)?;
        fs::create_dir_all(root.join("scripts"))?;
        fs::write(
            root.join("scripts/check-runner-image-candidate.sh"),
            "exit 0\n",
        )?;
        let mise_path = root.join("mise.toml");
        let mise = fs::read_to_string(&mise_path)?;
        assert!(mise.contains(old));
        fs::write(mise_path, mise.replacen(old, new, 1))?;
        let error = prepare(root)
            .and_then(|prepared| finalized_jobs(&prepared))
            .err()
            .ok_or_else(|| format!("accepted unsafe cargo wrapper {new}"))?;
        assert!(
            ["ambiguous_compile_driver", "build_task_mise_cargo_wrapper"]
                .iter()
                .any(|reason| error.to_string().contains(reason)),
            "{error}"
        );
    }
    Ok(())
}
#[test]
fn runner_family_rejects_changed_mbx_workflow_selector() -> Result<(), Box<dyn Error>> {
    for replacement in [
        "",
        ", workflow = \"other.yml@refs/heads/main\"",
        ", workflow = \"release.yml@refs/heads/main\", unknown = \"x\"",
    ] {
        let repo = super::maintenance_features::owner_feature_repo(
            WorkflowPolicy::VelnorRepositoryV1,
            false,
            "default",
            "default",
        )?;
        let root = repo.path();
        super::runner_family_selection::copy_locked_tools(root)?;
        let config_path = root.join(".velnor/config.toml");
        let mut config = fs::read_to_string(&config_path)?;
        enable_family(&mut config);
        fs::write(config_path, config)?;
        fs::create_dir_all(root.join("scripts"))?;
        fs::write(
            root.join("scripts/check-runner-image-candidate.sh"),
            "exit 0\n",
        )?;
        let mise_path = root.join("mise.toml");
        let mise = fs::read_to_string(&mise_path)?;
        let original = ", workflow = \"release.yml@refs/heads/main\"";
        assert!(mise.contains(original));
        fs::write(mise_path, mise.replacen(original, replacement, 1))?;
        let error = prepare(root)
            .and_then(|prepared| finalized_jobs(&prepared))
            .err()
            .ok_or_else(|| format!("accepted workflow selector {replacement}"))?;
        assert!(
            ["build_task_mbx_selector", "runner_family_mbx_workflow"]
                .iter()
                .any(|reason| error.to_string().contains(reason)),
            "{error}"
        );
    }
    Ok(())
}
#[test]
fn runner_family_rejects_missing_or_invalid_mbx_lock_identity() -> Result<(), Box<dyn Error>> {
    for (old, replacement) in [
        ("repository_ids = { repository = \"1340721957\" }", ""),
        (
            "repository_ids = { repository = \"1340721957\" }",
            "repository_ids = { repository = \"invalid\" }",
        ),
        (
            "signer = \"sigstore-oidc:https://github.com/jdx/mr-boxington/.github/workflows/release.yml\"",
            "signer = \"sigstore-oidc:https://github.com/other/repo/.github/workflows/release.yml\"",
        ),
    ] {
        let repo = super::maintenance_features::owner_feature_repo(
            WorkflowPolicy::VelnorRepositoryV1,
            false,
            "default",
            "default",
        )?;
        let root = repo.path();
        super::runner_family_selection::copy_locked_tools(root)?;
        let config_path = root.join(".velnor/config.toml");
        let mut config = fs::read_to_string(&config_path)?;
        enable_family(&mut config);
        fs::write(config_path, config)?;
        fs::create_dir_all(root.join("scripts"))?;
        fs::write(
            root.join("scripts/check-runner-image-candidate.sh"),
            "exit 0\n",
        )?;
        let lock_path = root.join("mise.lock");
        let mut lock = fs::read_to_string(&lock_path)?;
        let start = lock
            .find("[tools.mr-boxington.\"platforms.linux-x64\"]")
            .ok_or("MBX linux lock missing")?;
        let end = lock[start + 1..]
            .find("\n[")
            .map_or(lock.len(), |offset| start + 1 + offset);
        let section = &lock[start..end];
        assert!(section.contains(old));
        let changed = section.replacen(old, replacement, 1);
        lock.replace_range(start..end, &changed);
        fs::write(lock_path, lock)?;
        let error = prepare(root)
            .and_then(|prepared| finalized_jobs(&prepared))
            .err()
            .ok_or_else(|| format!("accepted invalid MBX lock {replacement}"))?;
        assert!(
            [
                "build_task_artifact:mr-boxington",
                "runner_family_mbx_identity"
            ]
            .iter()
            .any(|reason| error.to_string().contains(reason)),
            "{error}"
        );
    }
    Ok(())
}
#[test]
fn runner_family_rejects_unknown_configuration_fields() -> Result<(), Box<dyn Error>> {
    let repo = super::maintenance_features::owner_feature_repo(
        WorkflowPolicy::VelnorRepositoryV1,
        false,
        "default",
        "default",
    )?;
    let root = repo.path();
    super::runner_family_selection::copy_locked_tools(root)?;
    fs::create_dir_all(root.join("scripts"))?;
    let script = fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../scripts/check-runner-image-candidate.sh"),
    )?;
    fs::write(root.join("scripts/check-runner-image-candidate.sh"), script)?;
    let path = root.join(".velnor/config.toml");
    let mut config = fs::read_to_string(&path)?;
    enable_family(&mut config);
    fs::write(&path, &config)?;
    let prepared = prepare(root)?;
    assert!(finalized_jobs(&prepared)?.contains_key("image-runner-family-linux-amd64"));
    config.push_str("unexpected = true\n");
    fs::write(path, config)?;
    let error = prepare(root).err().ok_or("unknown field accepted")?;
    assert!(error.to_string().contains("unknown_config_field"));
    Ok(())
}
