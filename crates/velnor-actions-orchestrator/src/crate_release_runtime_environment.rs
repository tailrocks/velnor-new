// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    CrateReleaseAdmissionReceipt, CrateReleaseIntent, Duration, GitHubAdmissionOutcome,
    GitHubAdmissionRequest, Instant, NativeGitHubAdmissionRead, OpenOptions, OrchestratorError,
    Path, PathBuf, Write, admit_github_evidence, env, internal, thread,
};

pub(crate) fn request_from_environment(
    intent: &CrateReleaseIntent,
    workflow_sha: &str,
) -> Result<GitHubAdmissionRequest, OrchestratorError> {
    let repository = required("GITHUB_REPOSITORY")?;
    let event = required("GITHUB_EVENT_NAME")?;
    let git_ref = required("GITHUB_REF")?;
    let workflow_ref = required("GITHUB_WORKFLOW_REF")?;
    let workflow_commit = required("GITHUB_WORKFLOW_SHA")?;
    if !crate::release::release_identity::same_repository(&repository, &intent.repository)
        || git_ref != format!("refs/heads/{}", intent.default_branch)
        || workflow_ref
            != format!(
                "{repository}/{}@{git_ref}",
                velnor_actions_contract::CRATE_RELEASE_WORKFLOW_PATH
            )
        || workflow_commit != workflow_sha
    {
        return Err(internal("crate_release_workflow_identity"));
    }
    let approved_source = intent.exact_source_sha();
    let recovery_sha = source_for_event(&event, approved_source)?;
    if let Some(source) = recovery_sha.as_deref() {
        let event_path = required_path("GITHUB_EVENT_PATH")?;
        let event_text = crate::safe_read::read_event_file(&event_path, 1_048_576)
            .map_err(|_| internal("crate_release_dispatch_event_read"))?;
        let event_value: serde_json::Value = serde_json::from_str(&event_text)
            .map_err(|_| internal("crate_release_dispatch_event_invalid"))?;
        validate_dispatch_inputs(intent, source, &event_value)?;
    }
    let source_sha = recovery_sha
        .clone()
        .unwrap_or_else(|| workflow_sha.to_owned());
    let run_id = required("GITHUB_RUN_ID")?
        .parse()
        .map_err(|_| internal("crate_release_run_id"))?;
    let run_attempt = required("GITHUB_RUN_ATTEMPT")?
        .parse()
        .map_err(|_| internal("crate_release_run_attempt"))?;
    Ok(GitHubAdmissionRequest {
        repository,
        default_branch: intent.default_branch.clone(),
        environment: intent.environment.clone(),
        workflow_path: velnor_actions_contract::CRATE_RELEASE_WORKFLOW_PATH.to_owned(),
        required_workflow_path: intent.required_workflow_path.clone(),
        trusted_reviewer_id: intent.trusted_reviewer_id,
        workflow_sha: workflow_sha.to_owned(),
        run_id,
        run_attempt,
        event,
        source_sha,
        recovery_sha,
    })
}

fn validate_dispatch_inputs(
    intent: &CrateReleaseIntent,
    source: &str,
    event: &serde_json::Value,
) -> Result<(), OrchestratorError> {
    let inputs = event
        .get("inputs")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| internal("crate_release_dispatch_inputs"))?;
    let exact = |name: &str, expected: &str| {
        inputs.get(name).and_then(serde_json::Value::as_str) == Some(expected)
    };
    if source.len() != 40
        || !source
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        || !exact(
            "plan",
            &crate::release::release_identity::plan_id_for_source(source),
        )
        || !exact("source_sha", source)
        || (intent.oidc_qualification && !exact("mode", "release"))
        || (!intent.oidc_qualification && inputs.contains_key("mode"))
        || match &intent.recovery {
            Some(recovery) => !exact("version", &recovery.version),
            None => inputs.contains_key("version"),
        }
    {
        return Err(internal("crate_release_dispatch_inputs"));
    }
    Ok(())
}

fn source_for_event(
    event: &str,
    approved_source: Option<&str>,
) -> Result<Option<String>, OrchestratorError> {
    match (event, approved_source) {
        ("workflow_dispatch", Some(source)) => Ok(Some(source.to_owned())),
        ("push", None) => Ok(None),
        ("push", Some(_)) => Err(internal("crate_release_exact_source_requires_dispatch")),
        ("workflow_dispatch", None) => Err(internal("crate_release_recovery_missing")),
        _ => Err(internal("crate_release_event_invalid")),
    }
}

pub(super) fn source_admit(
    request: &GitHubAdmissionRequest,
    token: &str,
    require_candidate: bool,
) -> Result<(), OrchestratorError> {
    let deadline = Instant::now() + Duration::from_secs(600);
    let outcome = poll_pending_admission(
        || {
            let github = NativeGitHubAdmissionRead::new(token.to_owned())?;
            admit_github_evidence(request, &github)
        },
        || Instant::now() < deadline,
        || thread::sleep(Duration::from_secs(5)),
    )?;
    match outcome {
        GitHubAdmissionOutcome::Candidate(_) => {
            let approval = format!(
                "I reviewed the complete release policy at {} and source {}, and authorize this source for run {}.",
                request.workflow_sha, request.source_sha, request.run_id
            );
            println!(
                "Review the release policy and approve the protected environment with this comment: {approval}"
            );
            append_output(&format!(
                "eligible=true\nsource_sha={}\napproval_comment={approval}\n",
                request.source_sha
            ))
        }
        GitHubAdmissionOutcome::Skipped if require_candidate => {
            Err(internal("crate_release_candidate_disappeared"))
        }
        GitHubAdmissionOutcome::Skipped => append_output("eligible=false\n"),
    }
}

pub(super) fn read_receipt(path: &Path) -> Result<CrateReleaseAdmissionReceipt, OrchestratorError> {
    let text = crate::safe_read::read_event_file(path, 1_048_576)
        .map_err(|_| internal("crate_release_receipt_read"))?;
    serde_json::from_str(&text).map_err(|_| internal("crate_release_receipt_invalid"))
}

pub(super) fn required(name: &str) -> Result<String, OrchestratorError> {
    env::var(name)
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| internal(&format!("crate_release_missing_{name}")))
}
pub(super) fn required_path(name: &str) -> Result<PathBuf, OrchestratorError> {
    let path = PathBuf::from(required(name)?);
    if !path.is_absolute() {
        return Err(internal("crate_release_path_invalid"));
    }
    Ok(path)
}
pub(super) fn append_output(body: &str) -> Result<(), OrchestratorError> {
    let path = required_path("GITHUB_OUTPUT")?;
    let mut file = OpenOptions::new()
        .append(true)
        .open(path)
        .map_err(|_| internal("crate_release_output"))?;
    file.write_all(body.as_bytes())
        .map_err(|_| internal("crate_release_output"))
}

pub(super) fn require_authority_implementation() -> Result<(), OrchestratorError> {
    Err(internal("crate_release_authority_not_qualified"))
}

pub(super) fn poll_pending_admission<T>(
    mut observe: impl FnMut() -> Result<T, OrchestratorError>,
    mut may_retry: impl FnMut() -> bool,
    mut wait: impl FnMut(),
) -> Result<T, OrchestratorError> {
    loop {
        match observe() {
            Err(OrchestratorError::Internal { problem })
                if matches!(
                    problem.as_str(),
                    "crate_release_required_pending"
                        | "crate_release_runs_changed"
                        | "crate_release_checks_changed"
                        | "crate_release_jobs_changed"
                ) && may_retry() =>
            {
                wait();
                if !may_retry() {
                    return Err(OrchestratorError::Internal { problem });
                }
            }
            result => return result,
        }
    }
}

pub(super) fn write_checkpoint(
    path: &Path,
    stage: &str,
    receipt: &CrateReleaseAdmissionReceipt,
) -> Result<(), OrchestratorError> {
    let body = serde_json::to_vec(&serde_json::json!({"schema":1,"stage":stage,"receipt":receipt}))
        .map_err(|_| internal("crate_release_checkpoint_write"))?;
    velnor_actions_contract::exclusive_write::write_exclusive(
        path,
        &body,
        "crate_release_checkpoint",
    )
    .map_err(Into::into)
}

pub(crate) fn release_mode(receipt: &CrateReleaseAdmissionReceipt) -> &'static str {
    use crate::crate_release_controller::CrateReleaseAction;
    let publish = receipt
        .packages
        .iter()
        .any(|package| matches!(package.action, CrateReleaseAction::Publish));
    let repair = receipt.packages.iter().any(|package| {
        matches!(
            package.action,
            CrateReleaseAction::RepairTag { .. } | CrateReleaseAction::RepairRelease { .. }
        )
    });
    match (publish, repair) {
        (true, true) => "mixed",
        (true, false) => "publish",
        (false, true) => "repair",
        (false, false) => "none",
    }
}

#[cfg(test)]
mod exact_source_tests {
    use super::{
        CrateReleaseIntent, request_from_environment, source_for_event, validate_dispatch_inputs,
    };

    fn recovery_intent(enabled: bool) -> Result<CrateReleaseIntent, Box<dyn std::error::Error>> {
        Ok(serde_json::from_value(serde_json::json!({
            "schema":1,"oidc_qualification":enabled,"repository":"owner/project","default_branch":"main","environment":"release",
            "required_workflow_path":".github/workflows/ci.yml","trusted_reviewer_id":1,
            "generator_version":"0.1.78","generator_source_sha":"c".repeat(40),"generator_manifest_sha256":"d".repeat(64),
            "release_plz_version":"0.4.3","runner":"ubuntu-26.04","authentication":"trusted-publishing",
            "rust_version":"1.99.1","manifest_path":"Cargo.toml","pr_write_paths":["Cargo.toml"],
            "registry":"crates-io","packages":["demo"],"publish_features":{"demo":[]},
            "tag_template":"{{ package }}-v{{ version }}",
            "recovery":{"package":"demo","version":"0.1.1","source_sha":"a".repeat(40)}
        }))?)
    }

    #[test]
    fn exact_source_requires_dispatch() -> Result<(), Box<dyn std::error::Error>> {
        let source = "a".repeat(40);
        assert!(source_for_event("push", Some(&source)).is_err());
        assert_eq!(
            source_for_event("workflow_dispatch", Some(&source))?,
            Some(source)
        );
        assert_eq!(source_for_event("push", None)?, None);
        assert!(source_for_event("workflow_dispatch", None).is_err());
        Ok(())
    }

    #[test]
    fn recovery_dispatch_inputs_must_match_approved_intent_exactly()
    -> Result<(), Box<dyn std::error::Error>> {
        let source = "a".repeat(40);
        let intent = recovery_intent(true)?;
        let valid = serde_json::json!({"inputs":{"mode":"release","plan":"release-aaaaaaaaaaaa","source_sha":source,"version":"0.1.1"}});
        assert!(validate_dispatch_inputs(&intent, &source, &valid).is_ok());
        for (field, value) in [
            ("mode", "RELEASE"),
            ("plan", "RELEASE-AAAAAAAAAAAA"),
            ("source_sha", "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"),
            ("version", "0.1.2"),
            ("version", ""),
        ] {
            let mut changed = valid.clone();
            changed["inputs"][field] = value.into();
            assert!(
                validate_dispatch_inputs(&intent, &source, &changed).is_err(),
                "accepted {field}"
            );
        }
        let mut missing = valid.clone();
        missing["inputs"]
            .as_object_mut()
            .ok_or("inputs")?
            .remove("mode");
        assert!(validate_dispatch_inputs(&intent, &source, &missing).is_err());
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn runtime_rejects_invalid_dispatch_files_and_disabled_mode()
    -> Result<(), Box<dyn std::error::Error>> {
        use std::os::unix::fs::symlink;
        if let Ok(expected) = std::env::var("VELNOR_DISPATCH_FILE_TEST") {
            let intent =
                recovery_intent(std::env::var("VELNOR_DISPATCH_QUALIFICATION")? == "true")?;
            let admitted = request_from_environment(&intent, &"b".repeat(40)).is_ok();
            assert_eq!(admitted, expected == "accept");
            return Ok(());
        }
        let temp = tempfile::tempdir()?;
        let valid = temp.path().join("valid.json");
        let disabled = temp.path().join("disabled.json");
        let oversized = temp.path().join("oversized.json");
        let link = temp.path().join("link.json");
        let event = serde_json::json!({"inputs":{"mode":"release","plan":"release-aaaaaaaaaaaa","source_sha":"a".repeat(40),"version":"0.1.1"}});
        std::fs::write(&valid, serde_json::to_vec(&event)?)?;
        let mut no_mode = event.clone();
        no_mode["inputs"]
            .as_object_mut()
            .ok_or("inputs")?
            .remove("mode");
        std::fs::write(&disabled, serde_json::to_vec(&no_mode)?)?;
        std::fs::write(&oversized, vec![b' '; 1_048_577])?;
        symlink(&valid, &link)?;
        for (path, enabled, expected) in [
            (Some(valid.as_path()), true, "accept"),
            (None, true, "reject"),
            (
                Some(temp.path().join("missing.json").as_path()),
                true,
                "reject",
            ),
            (Some(oversized.as_path()), true, "reject"),
            (Some(link.as_path()), true, "reject"),
            (Some(valid.as_path()), false, "reject"),
            (Some(disabled.as_path()), false, "accept"),
        ] {
            let mut child = std::process::Command::new(std::env::current_exe()?);
            child
                .env_clear()
                .env("VELNOR_DISPATCH_FILE_TEST", expected)
                .env("VELNOR_DISPATCH_QUALIFICATION", enabled.to_string())
                .env("GITHUB_REPOSITORY", "owner/project")
                .env("GITHUB_EVENT_NAME", "workflow_dispatch")
                .env("GITHUB_REF", "refs/heads/main")
                .env(
                    "GITHUB_WORKFLOW_REF",
                    "owner/project/.github/workflows/release.yml@refs/heads/main",
                )
                .env("GITHUB_WORKFLOW_SHA", "b".repeat(40))
                .env("GITHUB_RUN_ID", "1")
                .env("GITHUB_RUN_ATTEMPT", "1")
                .args([
                    "--exact",
                    crate::libtest_case::name(concat!(
                        module_path!(),
                        "::runtime_rejects_invalid_dispatch_files_and_disabled_mode"
                    )),
                ]);
            if let Some(path) = path {
                child.env("GITHUB_EVENT_PATH", path);
            }
            let output = child.output()?;
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            crate::libtest_case::assert_one_passed(&output);
        }
        Ok(())
    }
}
