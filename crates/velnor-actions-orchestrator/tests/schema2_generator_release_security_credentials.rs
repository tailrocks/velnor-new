//! Exact verifier credential mapping and environment checks for release snapshots.

use std::error::Error;

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
        let mut trusted_path_found = false;
        for (key, value) in entries {
            if observed.insert(key.as_str(), value.as_str()).is_some() {
                return Err(format!("duplicate {key:?} env binding in {expected_name:?}").into());
            }
            if !expected_keys.contains(key.as_str()) {
                if key == "PATH" && value == velnor_actions_workflow_renderer::VERIFY_PATH {
                    trusted_path_found = true;
                    continue;
                }
                return Err(format!("unexpected env binding {key:?} in {expected_name:?}").into());
            }
            if value != "\"\"" {
                return Err(format!("{key:?} must be exactly empty in {expected_name:?}").into());
            }
        }
        if observed.len() != expected_keys.len() + 1
            || !trusted_path_found
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
