use super::action_snapshots::{Actions, action};
use std::error::Error;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

#[derive(Clone, Copy)]
struct TargetJobs {
    target: &'static str,
    build: &'static str,
    qualify: &'static str,
    attest: &'static str,
    qualifier_action: &'static str,
    attester_action: &'static str,
}

pub(super) fn assert_isolated_candidate_execution(
    workflow: &str,
    actions: &Actions,
) -> Result<(), Box<dyn Error>> {
    for target in [
        TargetJobs {
            target: "linux",
            build: "build-linux",
            qualify: "qualify-linux",
            attest: "attest-linux",
            qualifier_action: "generator-release-qualify-linux",
            attester_action: "generator-release-attest-linux",
        },
        TargetJobs {
            target: "macos",
            build: "build-macos",
            qualify: "qualify-macos",
            attest: "attest-macos",
            qualifier_action: "generator-release-qualify-macos",
            attester_action: "generator-release-attest-macos",
        },
        TargetJobs {
            target: "macos-intel",
            build: "build-macos-intel",
            qualify: "qualify-macos-intel",
            attest: "attest-macos-intel",
            qualifier_action: "generator-release-qualify-macos-intel",
            attester_action: "generator-release-attest-macos-intel",
        },
    ] {
        assert_target_boundary(workflow, actions, target)?;
    }
    candidate_environment_writes_cannot_cross_job_boundary()?;
    Ok(())
}

fn assert_target_boundary(
    workflow: &str,
    actions: &Actions,
    target: TargetJobs,
) -> Result<(), Box<dyn Error>> {
    assert_build_artifact_outputs(workflow, target.build)?;
    assert_qualifier_boundary(workflow, actions, target)?;
    assert_attester_boundary(workflow, actions, target)
}

fn assert_build_artifact_outputs(workflow: &str, build: &str) -> Result<(), Box<dyn Error>> {
    let build_job = super::super::job_body(workflow, build)?;
    assert!(
        build_job.contains("outputs:\n      artifact_id:"),
        "{build_job}"
    );
    assert!(!build_job.contains("artifact_digest:"), "{build_job}");
    assert!(build_job.contains("id: upload\n"), "{build_job}");
    assert!(build_job.contains("artifact-id"), "{build_job}");
    Ok(())
}

fn assert_qualifier_boundary(
    workflow: &str,
    actions: &Actions,
    target: TargetJobs,
) -> Result<(), Box<dyn Error>> {
    let qualifier = super::super::job_body(workflow, target.qualify)?;
    assert_qualifier_job_permissions(qualifier, target);
    assert_qualifier_job_inputs(qualifier, target)?;
    let qualifier_body = action(actions, target.qualifier_action)?;
    assert_qualifier_action_boundary(qualifier_body)?;
    assert_job_action(workflow, target.qualify, target.qualifier_action)?;
    Ok(())
}

fn assert_qualifier_job_permissions(qualifier: &str, target: TargetJobs) {
    assert!(qualifier.contains("permissions: {}"), "{qualifier}");
    for permission in [
        "actions: write",
        "contents: write",
        "id-token: write",
        "attestations: write",
    ] {
        assert!(
            !qualifier.contains(permission),
            "{}: {qualifier}",
            target.target
        );
    }
    assert!(
        qualifier.contains(&format!("- {}\n", target.build)),
        "{qualifier}"
    );
    assert!(!qualifier.contains("outputs:"), "{qualifier}");
    assert!(!qualifier.contains("persist-credentials:"), "{qualifier}");
    assert!(!qualifier.contains("actions/checkout@"), "{qualifier}");
}

fn assert_qualifier_job_inputs(qualifier: &str, target: TargetJobs) -> Result<(), Box<dyn Error>> {
    assert!(
        qualifier.contains(&format!(
            "artifact_id: ${{{{ needs.{}.outputs.artifact_id }}}}",
            target.build
        )),
        "{qualifier}"
    );
    assert_eq!(qualifier.matches("- name:").count(), 2, "{qualifier}");
    let first_step = qualifier
        .split("- name:")
        .nth(1)
        .ok_or("qualifier has no first step")?
        .split("- name:")
        .next()
        .ok_or("qualifier first step is missing")?;
    assert!(first_step.contains("shell: bash"), "{first_step}");
    assert!(first_step.contains("run:"), "{first_step}");
    assert!(first_step.contains("python3"), "{first_step}");
    assert!(first_step.contains("credential.helper="), "{first_step}");
    assert!(first_step.contains("GITHUB_SHA"), "{first_step}");
    assert!(!first_step.contains("uses:"), "{first_step}");
    let source_prepare = qualifier
        .find("Fetch exact public source without an action post hook")
        .ok_or("missing token-free source preparation")?;
    let composite_call = qualifier
        .find(&format!(
            "uses: ./.github/actions/{}",
            target.qualifier_action
        ))
        .ok_or("missing qualification composite call")?;
    assert!(source_prepare < composite_call, "{qualifier}");
    Ok(())
}

fn assert_qualifier_action_boundary(qualifier_body: &str) -> Result<(), Box<dyn Error>> {
    assert_action_input(qualifier_body);
    assert!(
        qualifier_body.contains("artifact-ids: ${{ inputs.artifact_id }}"),
        "{qualifier_body}"
    );
    assert!(!qualifier_body.contains("${{ needs."), "{qualifier_body}");
    assert!(!qualifier_body.contains("post:"), "{qualifier_body}");
    assert!(!qualifier_body.contains("github_token"), "{qualifier_body}");
    assert_eq!(
        qualifier_body.matches("uses:").count(),
        3,
        "{qualifier_body}"
    );
    assert!(
        qualifier_body.contains("actions/download-artifact@"),
        "{qualifier_body}"
    );
    assert!(
        qualifier_body.contains("artifact-ids: ${{ inputs.manifest_artifact_id }}"),
        "{qualifier_body}"
    );
    assert!(
        qualifier_body.contains("Qualify downloaded candidate"),
        "{qualifier_body}"
    );
    assert_eq!(
        qualifier_body
            .matches("Qualify downloaded candidate")
            .count(),
        1
    );
    assert_exact_empty_verifier_credentials(
        qualifier_body,
        &["Verify Rust channel manifest", "Verify jactionlint"],
    )?;
    assert!(
        !qualifier_body.contains("Attest built artifacts"),
        "{qualifier_body}"
    );
    assert!(
        !qualifier_body.contains("actions/upload-artifact@"),
        "{qualifier_body}"
    );
    let candidate = qualifier_body
        .find("Qualify downloaded candidate")
        .ok_or("missing candidate execution")?;
    assert!(
        !qualifier_body[candidate..].contains("- name:"),
        "{qualifier_body}"
    );
    Ok(())
}

fn assert_attester_boundary(
    workflow: &str,
    actions: &Actions,
    target: TargetJobs,
) -> Result<(), Box<dyn Error>> {
    let attester = super::super::job_body(workflow, target.attest)?;
    assert!(
        attester.contains(&format!("- {}\n", target.build)),
        "{attester}"
    );
    assert!(
        attester.contains(&format!("- {}\n", target.qualify)),
        "{attester}"
    );
    assert!(
        attester.contains(&format!(
            "artifact_id: ${{{{ needs.{}.outputs.artifact_id }}}}",
            target.build
        )),
        "{attester}"
    );
    assert!(attester.contains("id-token: write"), "{attester}");
    assert_job_action(workflow, target.attest, target.attester_action)?;
    let attester_body = action(actions, target.attester_action)?;
    assert_action_input(attester_body);
    assert!(
        attester_body.contains("artifact-ids: ${{ inputs.artifact_id }}"),
        "{attester_body}"
    );
    assert!(!attester_body.contains("${{ needs."), "{attester_body}");
    assert!(
        attester_body.contains("Verify candidate provenance record"),
        "{attester_body}"
    );
    assert!(
        attester_body.contains("Verify downloaded checksum sidecar"),
        "{attester_body}"
    );
    assert!(
        attester_body.contains("Attest built artifacts"),
        "{attester_body}"
    );
    assert!(
        attester_body.contains("Fetch and verify candidate attestation bundles"),
        "{attester_body}"
    );
    assert!(
        attester_body.contains("Upload verified candidate attestation bundles"),
        "{attester_body}"
    );
    assert!(
        !attester_body.contains("Qualify downloaded candidate"),
        "{attester_body}"
    );
    assert!(!attester_body.contains("--version"), "{attester_body}");
    assert!(
        !attester_body.contains("capture-opentofu-goldens.sh"),
        "{attester_body}"
    );
    assert!(
        !attester_body.contains(&format!("needs.{}.outputs", target.qualify)),
        "{attester_body}"
    );
    Ok(())
}

fn assert_action_input(action: &str) {
    assert!(
        action.contains("inputs:\n  artifact_id:\n    description:"),
        "{action}"
    );
    assert!(action.contains("    required: true\n"), "{action}");
}

fn assert_job_action(workflow: &str, id: &str, action_name: &str) -> Result<(), Box<dyn Error>> {
    let job = super::super::job_body(workflow, id)?;
    assert!(
        job.contains(&format!("uses: ./.github/actions/{action_name}")),
        "{job}"
    );
    Ok(())
}

fn candidate_environment_writes_cannot_cross_job_boundary() -> Result<(), Box<dyn Error>> {
    let qualifier = tempfile::TempDir::new()?;
    let attester = tempfile::TempDir::new()?;
    let qualifier_bin = qualifier.path().join("bin");
    let attester_bin = attester.path().join("bin");
    fs::create_dir_all(&qualifier_bin)?;
    fs::create_dir_all(&attester_bin)?;
    let malicious_gh = qualifier_bin.join("gh");
    write_executable(
        &malicious_gh,
        "#!/bin/sh\nprintf '%s\\n' leaked > \"$QUALIFIER_TOKEN_CAPTURE\"\n",
    )?;
    let candidate = qualifier.path().join("candidate.sh");
    write_executable(
        &candidate,
        "#!/bin/sh\nprintf '%s\\n' \"$QUALIFIER_BIN\" >> \"$GITHUB_PATH\"\nprintf '%s\\n' 'INJECTED=1' >> \"$GITHUB_ENV\"\n(sleep 0.05; : > \"$QUALIFIER_BACKGROUND\") >/dev/null 2>&1 &\n",
    )?;
    let qualifier_path = qualifier.path().join("github-path");
    let qualifier_env = qualifier.path().join("github-env");
    let background = qualifier.path().join("background-finished");
    fs::write(&qualifier_path, "")?;
    fs::write(&qualifier_env, "")?;
    let candidate_status = Command::new(&candidate)
        .env("QUALIFIER_BIN", &qualifier_bin)
        .env("GITHUB_PATH", &qualifier_path)
        .env("GITHUB_ENV", &qualifier_env)
        .env("QUALIFIER_BACKGROUND", &background)
        .status()?;
    assert!(candidate_status.success());

    let trusted_gh = attester_bin.join("gh");
    write_executable(
        &trusted_gh,
        "#!/bin/sh\nprintf '%s\\n' \"$GH_TOKEN\" > \"$ATTEST_TOKEN_CAPTURE\"\n",
    )?;
    let attest_path = attester.path().join("github-path");
    let attest_env = attester.path().join("github-env");
    let token_capture = attester.path().join("token-capture");
    fs::write(&attest_path, "")?;
    fs::write(&attest_env, "")?;
    let gh_status = Command::new("gh")
        .arg("api")
        .env("PATH", &attester_bin)
        .env("GITHUB_PATH", &attest_path)
        .env("GITHUB_ENV", &attest_env)
        .env("GH_TOKEN", "attest-job-token")
        .env("ATTEST_TOKEN_CAPTURE", &token_capture)
        .status()?;
    assert!(gh_status.success());
    assert_eq!(fs::read_to_string(&token_capture)?, "attest-job-token\n");
    assert!(!qualifier.path().join("token-capture").exists());
    assert_eq!(fs::read_to_string(&qualifier_env)?, "INJECTED=1\n");
    assert!(
        fs::read_to_string(&qualifier_path)?
            .contains(qualifier_bin.to_str().ok_or("non-UTF8 path")?)
    );
    assert_eq!(fs::read_to_string(&attest_path)?, "");
    assert_eq!(fs::read_to_string(&attest_env)?, "");
    for _ in 0..50 {
        if background.exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(
        background.exists(),
        "candidate background process did not run"
    );
    assert_eq!(fs::read_to_string(&attest_path)?, "");
    assert_eq!(fs::read_to_string(&attest_env)?, "");
    Ok(())
}

fn write_executable(path: &Path, content: &str) -> Result<(), Box<dyn Error>> {
    fs::write(path, content)?;
    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)?;
    Ok(())
}

type SnapshotStep = (String, Option<Vec<(String, String)>>);

fn finish_snapshot_step(
    steps: &mut Vec<SnapshotStep>,
    name: &mut Option<String>,
    env: &mut Option<Vec<(String, String)>>,
) {
    if let Some(name) = name.take() {
        steps.push((name, env.take()));
    }
}

fn parse_snapshot_steps(body: &str) -> Result<Vec<SnapshotStep>, Box<dyn Error>> {
    let mut steps = Vec::new();
    let mut step_name: Option<String> = None;
    let mut step_indent: Option<usize> = None;
    let mut env_entries: Option<Vec<(String, String)>> = None;
    let mut in_env = false;

    for line in body.lines() {
        let indent = line.len() - line.trim_start().len();
        let trimmed = line.trim_start();
        if let Some(name) = trimmed.strip_prefix("- name: ") {
            finish_snapshot_step(&mut steps, &mut step_name, &mut env_entries);
            step_name = Some(name.to_owned());
            step_indent = Some(indent);
            in_env = false;
            continue;
        }

        let env_indent = step_indent.map(|level| level + 2);
        let values_indent = step_indent.map(|level| level + 4);
        if step_name.is_some() && Some(indent) == env_indent && trimmed == "env:" {
            let name = step_name.as_ref().ok_or("step name disappeared")?;
            if env_entries.is_some() {
                return Err(format!("duplicate env mapping in step {name:?}").into());
            }
            env_entries = Some(Vec::new());
            in_env = true;
            continue;
        }

        if let Some(name) = step_name.as_ref()
            && in_env
            && !trimmed.is_empty()
        {
            if Some(indent) == values_indent {
                let Some((key, value)) = trimmed.split_once(": ") else {
                    return Err(format!("malformed env entry in step {name:?}: {trimmed:?}").into());
                };
                env_entries
                    .as_mut()
                    .ok_or("env entry without env mapping")?
                    .push((key.to_owned(), value.to_owned()));
                continue;
            }
            if Some(indent) > values_indent {
                return Err(format!(
                    "unsupported nested env mapping in step {name:?}: {trimmed:?}"
                )
                .into());
            }
            in_env = false;
        }

        if let Some((key, value)) = trimmed.split_once(':') {
            let normalized_key = key.trim().trim_matches(['"', '\'']).to_ascii_uppercase();
            if normalized_key.contains("TOKEN")
                || matches!(normalized_key.as_str(), "GH_HOST" | "GH_CONFIG_DIR")
            {
                return Err(format!(
                    "credential or endpoint key outside a verifier env mapping: {line:?}"
                )
                .into());
            }
            if normalized_key == "ENV" && value.to_ascii_uppercase().contains("TOKEN") {
                return Err(format!("inline token environment is not accepted: {line:?}").into());
            }
        }
    }
    finish_snapshot_step(&mut steps, &mut step_name, &mut env_entries);
    Ok(steps)
}

fn expected_credential_keys() -> std::collections::BTreeSet<&'static str> {
    velnor_actions_workflow_renderer::toolchain_env::STEP_CREDENTIAL_DENYLIST
        .into_iter()
        .chain(velnor_actions_workflow_renderer::toolchain_env::STEP_ENDPOINT_DENYLIST)
        .collect()
}

fn assert_verifier_envs(
    steps: &[SnapshotStep],
    verifier_steps: &[&str],
    expected_keys: &std::collections::BTreeSet<&'static str>,
) -> Result<(), Box<dyn Error>> {
    for expected_name in verifier_steps {
        let matching = steps
            .iter()
            .filter(|(name, _)| name == expected_name)
            .collect::<Vec<_>>();
        if matching.len() != 1 {
            return Err(format!(
                "expected exactly one {expected_name:?} step, found {}",
                matching.len()
            )
            .into());
        }
        let Some(entries) = matching[0].1.as_ref() else {
            return Err(format!("missing env mapping in verifier step {expected_name:?}").into());
        };
        let mut observed = std::collections::BTreeMap::new();
        for (key, value) in entries {
            if observed.insert(key.as_str(), value.as_str()).is_some() {
                return Err(format!("duplicate {key:?} env binding in {expected_name:?}").into());
            }
            if !expected_keys.contains(key.as_str()) {
                return Err(format!("unexpected env binding {key:?} in {expected_name:?}").into());
            }
            if value != "\"\"" {
                return Err(format!("{key:?} must be exactly empty in {expected_name:?}").into());
            }
        }
        if observed.len() != expected_keys.len()
            || expected_keys.iter().any(|key| !observed.contains_key(key))
        {
            return Err(format!("verifier credential env set differs in {expected_name:?}").into());
        }
    }
    Ok(())
}

fn assert_no_misplaced_credentials(
    steps: &[SnapshotStep],
    verifier_steps: &[&str],
    expected_keys: &std::collections::BTreeSet<&'static str>,
) -> Result<(), Box<dyn Error>> {
    for (name, env) in steps {
        if verifier_steps.contains(&name.as_str()) {
            continue;
        }
        if let Some(entries) = env
            && let Some((key, _)) = entries.iter().find(|(key, _)| {
                expected_keys.contains(key.as_str()) || key.to_ascii_uppercase().contains("TOKEN")
            })
        {
            return Err(format!("credential binding {key:?} is misplaced in step {name:?}").into());
        }
    }
    Ok(())
}

pub(super) fn assert_exact_empty_verifier_credentials(
    body: &str,
    verifier_steps: &[&str],
) -> Result<(), Box<dyn Error>> {
    let expected_keys = expected_credential_keys();
    let steps = parse_snapshot_steps(body)?;
    assert_verifier_envs(&steps, verifier_steps, &expected_keys)?;
    assert_no_misplaced_credentials(&steps, verifier_steps, &expected_keys)
}

pub(super) fn assert_release_workflow_verifier(
    workflow: &str,
    step_name: &str,
    globally_scoped_key: &str,
) -> Result<(), Box<dyn Error>> {
    let step = named_step(workflow, step_name)?;
    assert_exact_empty_verifier_credentials(&step, &[step_name])?;
    let key = format!("{globally_scoped_key}:");
    let occurrences = workflow.matches(&key).count();
    if occurrences != 1 {
        return Err(format!(
            "{globally_scoped_key} must occur exactly once in the workflow, found {occurrences}"
        )
        .into());
    }
    let exact_empty_mapping = format!("{globally_scoped_key}: \"\"");
    if !step.lines().any(|line| line.trim() == exact_empty_mapping) {
        return Err(format!("{globally_scoped_key} must be exactly empty in {step_name:?}").into());
    }
    Ok(())
}

fn named_step(body: &str, expected_name: &str) -> Result<String, Box<dyn Error>> {
    let mut current_name: Option<String> = None;
    let mut current = String::new();
    let mut matches = Vec::new();
    for line in body.lines() {
        if let Some(name) = line.trim_start().strip_prefix("- name: ") {
            if current_name.as_deref() == Some(expected_name) {
                matches.push(current.clone());
            }
            current_name = Some(name.to_owned());
            current.clear();
        }
        if current_name.is_some() {
            current.push_str(line);
            current.push('\n');
        }
    }
    if current_name.as_deref() == Some(expected_name) {
        matches.push(current);
    }
    if matches.len() != 1 {
        return Err(format!(
            "expected exactly one step named {expected_name:?}, found {}",
            matches.len()
        )
        .into());
    }
    Ok(matches.pop().ok_or("step disappeared")?)
}

#[cfg(test)]
mod credential_environment_tests {
    use super::{assert_exact_empty_verifier_credentials, assert_release_workflow_verifier};

    const VALID: &str = include_str!("snapshots/generator-release-qualify-linux.yml");

    #[test]
    fn exact_empty_credentials_are_limited_to_both_verifier_steps() {
        assert!(
            assert_exact_empty_verifier_credentials(
                VALID,
                &["Verify Rust channel manifest", "Verify jactionlint"],
            )
            .is_ok()
        );
    }

    #[test]
    fn rejects_live_or_expression_credentials() {
        for value in ["\"literal\"", "\"${{ github.token }}\""] {
            let tampered = VALID.replacen("GH_TOKEN: \"\"", &format!("GH_TOKEN: {value}"), 1);
            assert!(
                assert_exact_empty_verifier_credentials(
                    &tampered,
                    &["Verify Rust channel manifest", "Verify jactionlint"],
                )
                .is_err()
            );
        }
    }

    #[test]
    fn rejects_duplicate_missing_and_alternate_token_bindings() {
        let duplicate = VALID.replacen(
            "        GH_TOKEN: \"\"\n",
            "        GH_TOKEN: \"\"\n        GH_TOKEN: \"\"\n",
            1,
        );
        assert!(
            assert_exact_empty_verifier_credentials(
                &duplicate,
                &["Verify Rust channel manifest", "Verify jactionlint"],
            )
            .is_err()
        );

        let missing = VALID.replacen("        GH_TOKEN: \"\"\n", "", 1);
        assert!(
            assert_exact_empty_verifier_credentials(
                &missing,
                &["Verify Rust channel manifest", "Verify jactionlint"],
            )
            .is_err()
        );

        let alternate = VALID.replacen(
            "        GH_TOKEN: \"\"\n",
            "        GH_TOKEN: \"\"\n        EXTRA_TOKEN: \"\"\n",
            1,
        );
        assert!(
            assert_exact_empty_verifier_credentials(
                &alternate,
                &["Verify Rust channel manifest", "Verify jactionlint"],
            )
            .is_err()
        );
    }

    #[test]
    fn rejects_credentials_outside_verifier_steps() {
        let misplaced = VALID.replacen(
            "    - name: Setup Mise\n",
            "    - name: Setup Mise\n      env:\n        GH_TOKEN: \"\"\n",
            1,
        );
        assert!(
            assert_exact_empty_verifier_credentials(
                &misplaced,
                &["Verify Rust channel manifest", "Verify jactionlint"],
            )
            .is_err()
        );

        let root_level = VALID.replacen("runs:\n", "runs:\n  env:\n    GH_TOKEN: \"\"\n", 1);
        assert!(
            assert_exact_empty_verifier_credentials(
                &root_level,
                &["Verify Rust channel manifest", "Verify jactionlint"],
            )
            .is_err()
        );

        let inline = VALID.replacen(
            "    - name: Setup Mise\n",
            "    - name: Setup Mise\n      env: { GH_TOKEN: \"\" }\n",
            1,
        );
        assert!(
            assert_exact_empty_verifier_credentials(
                &inline,
                &["Verify Rust channel manifest", "Verify jactionlint"],
            )
            .is_err()
        );
    }

    #[test]
    fn release_workflow_keeps_registry_token_empty_only_on_verifier() {
        const WORKFLOW: &str = include_str!("snapshots/product-release-images.yml");
        assert!(
            assert_release_workflow_verifier(
                WORKFLOW,
                "Verify Rust channel manifest",
                "CARGO_REGISTRY_TOKEN",
            )
            .is_ok()
        );

        let live = WORKFLOW.replacen(
            "CARGO_REGISTRY_TOKEN: \"\"",
            "CARGO_REGISTRY_TOKEN: ${{ secrets.REGISTRY_TOKEN }}",
            1,
        );
        assert!(
            assert_release_workflow_verifier(
                &live,
                "Verify Rust channel manifest",
                "CARGO_REGISTRY_TOKEN",
            )
            .is_err()
        );

        let misplaced = WORKFLOW.replacen(
            "      - name: Build runner and DinD images\n",
            "      - name: Build runner and DinD images\n        env:\n          CARGO_REGISTRY_TOKEN: \"\"\n",
            1,
        );
        assert!(
            assert_release_workflow_verifier(
                &misplaced,
                "Verify Rust channel manifest",
                "CARGO_REGISTRY_TOKEN",
            )
            .is_err()
        );
    }
}
