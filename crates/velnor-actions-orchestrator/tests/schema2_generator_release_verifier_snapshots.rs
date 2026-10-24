use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;

const VERIFY_SHELL: &str = "/bin/bash --noprofile --norc -p -e -o pipefail {0}";
const VERIFY_PATH: &str = "/usr/bin:/bin:/usr/sbin:/sbin";
const VERIFY_RUN_PREFIX: &str = r#"function_env_unset=(); function_env_scan_complete=0; while IFS= read -r -d '' env_entry; do case \"$env_entry\" in BASH_FUNC_*=*) function_env_unset+=(-u \"${env_entry%%=*}\");; VELNOR_VERIFIER_ENV_SCAN_COMPLETE) function_env_scan_complete=$((function_env_scan_complete + 1));; esac; done < <({ /usr/bin/env -0 && printf '%s\\000' 'VELNOR_VERIFIER_ENV_SCAN_COMPLETE'; }); [ \"$function_env_scan_complete\" -eq 1 ] || exit 1; exec /usr/bin/env \"${function_env_unset[@]}\" -u BASH_ENV -u ENV -u ACTIONS_ID_TOKEN_REQUEST_TOKEN -u ACTIONS_ID_TOKEN_REQUEST_URL -u ACTIONS_RUNTIME_TOKEN -u GITHUB_TOKEN -u MISE_GITHUB_TOKEN -u GH_TOKEN -u GH_HOST -u GH_CONFIG_DIR /bin/bash --noprofile --norc -p -e -o pipefail -c 'unset ACTIONS_ID_TOKEN_REQUEST_TOKEN ACTIONS_ID_TOKEN_REQUEST_URL ACTIONS_RUNTIME_TOKEN GITHUB_TOKEN MISE_GITHUB_TOKEN GH_TOKEN GH_HOST GH_CONFIG_DIR; "#;
pub(super) type AuthenticatedBinding<'a> = (&'a str, &'a str, &'a str);

struct CredentialPolicy<'a, 'b> {
    verifier_steps: &'a [&'b str],
    authenticated_bindings: &'a [AuthenticatedBinding<'b>],
}

#[derive(Default)]
struct Step {
    name: String,
    indent: usize,
    env: Option<BTreeMap<String, String>>,
    shell: Option<String>,
    run: Option<String>,
}

pub(super) fn assert_exact_empty_verifier_credentials(
    body: &str,
    verifier_steps: &[&str],
) -> Result<(), Box<dyn Error>> {
    assert_exact_empty_verifier_credentials_internal(body, verifier_steps, &[])
}

pub(super) fn assert_exact_empty_verifier_credentials_with_authenticated_binding(
    body: &str,
    verifier_steps: &[&str],
    step_name: &str,
    key: &str,
    expected_value: &str,
) -> Result<(), Box<dyn Error>> {
    assert_exact_empty_verifier_credentials_internal(
        body,
        verifier_steps,
        &[(step_name, key, expected_value)],
    )
}

pub(super) fn assert_exact_empty_verifier_credentials_with_bindings(
    body: &str,
    verifier_steps: &[&str],
    authenticated_bindings: &[AuthenticatedBinding<'_>],
) -> Result<(), Box<dyn Error>> {
    assert_exact_empty_verifier_credentials_internal(body, verifier_steps, authenticated_bindings)
}

fn assert_exact_empty_verifier_credentials_internal(
    body: &str,
    verifier_steps: &[&str],
    authenticated_bindings: &[AuthenticatedBinding<'_>],
) -> Result<(), Box<dyn Error>> {
    let steps = parse_steps(body, verifier_steps, authenticated_bindings)?;
    for name in verifier_steps {
        let matching = steps
            .iter()
            .filter(|step| step.name == *name)
            .collect::<Vec<_>>();
        if matching.len() != 1 {
            return Err(format!("expected one {name:?} step, found {}", matching.len()).into());
        }
        assert_verifier_env(matching[0])?;
    }
    for step in &steps {
        if verifier_steps.contains(&step.name.as_str()) {
            continue;
        }
        if let Some(env) = &step.env {
            for (key, value) in env {
                if !is_sensitive_key(key) {
                    continue;
                }
                let is_expected_authenticated_binding = authenticated_bindings.iter().any(
                    |(expected_step, expected_key, expected_value)| {
                        step.name.as_str() == *expected_step
                            && key.as_str() == *expected_key
                            && unquote(value) == *expected_value
                    },
                );
                if !is_expected_authenticated_binding {
                    return Err(format!(
                        "credential binding {key:?} is misplaced in {:?}",
                        step.name
                    )
                    .into());
                }
            }
        }
    }
    for (step_name, key, expected_value) in authenticated_bindings {
        let matching = steps
            .iter()
            .filter(|step| step.name.as_str() == *step_name)
            .collect::<Vec<_>>();
        if matching.len() != 1 {
            return Err(format!(
                "expected one authenticated {step_name:?} step, found {}",
                matching.len()
            )
            .into());
        }
        let value = matching[0]
            .env
            .as_ref()
            .and_then(|env| env.get(*key))
            .map(|value| unquote(value));
        if value.as_deref() != Some(*expected_value) {
            return Err(format!("expected {key}={expected_value:?} only on {step_name:?}").into());
        }
    }
    Ok(())
}

pub(super) fn assert_authenticated_binding(
    body: &str,
    step_name: &str,
    key: &str,
    expected_value: &str,
) -> Result<(), Box<dyn Error>> {
    assert_exact_empty_verifier_credentials_internal(body, &[], &[(step_name, key, expected_value)])
}

fn parse_steps(
    body: &str,
    verifier_steps: &[&str],
    authenticated_bindings: &[AuthenticatedBinding<'_>],
) -> Result<Vec<Step>, Box<dyn Error>> {
    let policy = CredentialPolicy {
        verifier_steps,
        authenticated_bindings,
    };
    let mut steps = Vec::new();
    let mut current: Option<Step> = None;
    let mut in_env = false;
    let mut permissions_indent = None;
    for line in body.lines() {
        let indent = line.len() - line.trim_start().len();
        let trimmed = line.trim_start();
        if trimmed == "permissions:" {
            permissions_indent = Some(indent);
        } else if permissions_indent.is_some_and(|permission_indent| indent <= permission_indent) {
            permissions_indent = None;
        }
        if let Some(name) = trimmed.strip_prefix("- name: ") {
            if let Some(step) = current.take() {
                steps.push(step);
            }
            current = Some(Step {
                name: unquote(name),
                indent,
                ..Step::default()
            });
            in_env = false;
            continue;
        }

        let step = current.as_mut();
        if let Some(step) = step {
            if indent == step.indent + 2 && trimmed == "env:" {
                if step.env.replace(BTreeMap::new()).is_some() {
                    return Err(format!("duplicate env mapping in {:?}", step.name).into());
                }
                in_env = true;
                continue;
            }
            if in_env && indent == step.indent + 4 {
                let (key, value) = trimmed.split_once(": ").ok_or_else(|| {
                    format!("malformed env binding in {:?}: {trimmed:?}", step.name)
                })?;
                let env = step.env.as_mut().ok_or("env entry without mapping")?;
                if env.insert(unquote(key), value.trim().to_owned()).is_some() {
                    return Err(format!("duplicate env binding {key:?} in {:?}", step.name).into());
                }
            } else if indent <= step.indent + 2 {
                in_env = false;
                if let Some(value) = trimmed.strip_prefix("shell: ") {
                    step.shell = Some(unquote(value));
                } else if let Some(value) = trimmed.strip_prefix("run: ") {
                    step.run = Some(unquote_run(value)?);
                }
            }
        }

        validate_sensitive_line(
            line,
            trimmed,
            indent,
            current.as_ref(),
            in_env,
            permissions_indent,
            &policy,
        )?;
    }
    if let Some(step) = current {
        steps.push(step);
    }
    Ok(steps)
}

fn validate_sensitive_line(
    line: &str,
    trimmed: &str,
    indent: usize,
    current: Option<&Step>,
    in_env: bool,
    permissions_indent: Option<usize>,
    policy: &CredentialPolicy<'_, '_>,
) -> Result<(), Box<dyn Error>> {
    if has_inline_sensitive_env(trimmed) {
        return Err(format!("inline credential environment is not accepted: {line:?}").into());
    }
    let key = yaml_key(trimmed);
    let id_token_permission = key == Some("id-token")
        && permissions_indent.is_some_and(|permission_indent| indent > permission_indent);
    if let Some(key) = key
        && is_sensitive_key(key)
        && !id_token_permission
    {
        let verifier_binding = current
            .is_some_and(|step| policy.verifier_steps.contains(&step.name.as_str()) && in_env);
        let expected_auth_binding = current.is_some_and(|step| {
            in_env
                && policy
                    .authenticated_bindings
                    .iter()
                    .any(|(step_name, auth_key, expected)| {
                        step.name.as_str() == *step_name
                            && key == *auth_key
                            && trimmed
                                .split_once(": ")
                                .is_some_and(|(_, value)| unquote(value) == *expected)
                    })
        });
        if !verifier_binding && !expected_auth_binding {
            return Err(format!("credential binding outside verifier env: {line:?}").into());
        }
    }
    Ok(())
}

fn assert_verifier_env(step: &Step) -> Result<(), Box<dyn Error>> {
    let env = step
        .env
        .as_ref()
        .ok_or_else(|| format!("missing env mapping in {:?}", step.name))?;
    let expected = expected_env_keys();
    if env.keys().map(String::as_str).collect::<BTreeSet<_>>() != expected {
        return Err(format!("verifier environment differs in {:?}: {env:?}", step.name).into());
    }
    for key in credential_keys() {
        if env.get(key).map(String::as_str) != Some("\"\"") {
            return Err(format!("{key} must be exactly empty in {:?}", step.name).into());
        }
    }
    if env.get("PATH").map(|value| unquote(value)) != Some(VERIFY_PATH.to_owned()) {
        return Err(format!("verifier PATH is not trusted in {:?}", step.name).into());
    }
    if step.shell.as_deref() != Some(VERIFY_SHELL) {
        return Err(format!("verifier shell is not startup-safe in {:?}", step.name).into());
    }
    let run = step
        .run
        .as_deref()
        .ok_or("verifier run command is missing")?;
    if !run.starts_with(VERIFY_RUN_PREFIX) {
        return Err(format!(
            "verifier command lacks the credential/startup boundary in {:?}",
            step.name
        )
        .into());
    }
    Ok(())
}

fn expected_env_keys() -> BTreeSet<&'static str> {
    credential_keys().chain(std::iter::once("PATH")).collect()
}

fn credential_keys() -> impl Iterator<Item = &'static str> {
    velnor_actions_workflow_renderer::toolchain_env::STEP_CREDENTIAL_DENYLIST
        .into_iter()
        .chain(velnor_actions_workflow_renderer::toolchain_env::STEP_ENDPOINT_DENYLIST)
}

fn is_sensitive_key(key: &str) -> bool {
    let key = key.trim_matches(['"', '\'']).to_ascii_uppercase();
    key.contains("TOKEN") || matches!(key.as_str(), "GH_HOST" | "GH_CONFIG_DIR")
}

fn has_inline_sensitive_env(line: &str) -> bool {
    line.starts_with("env:")
        && line.contains('{')
        && (line.to_ascii_uppercase().contains("TOKEN")
            || line.to_ascii_uppercase().contains("GH_HOST")
            || line.to_ascii_uppercase().contains("GH_CONFIG_DIR"))
}

fn yaml_key(line: &str) -> Option<&str> {
    line.split_once(':').map(|(key, _)| key.trim())
}

fn unquote(value: &str) -> String {
    value
        .trim()
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .or_else(|| {
            value
                .trim()
                .strip_prefix('\'')
                .and_then(|value| value.strip_suffix('\''))
        })
        .unwrap_or(value.trim())
        .to_owned()
}

fn unquote_run(value: &str) -> Result<String, Box<dyn Error>> {
    const ANNOTATION: &str = " # zizmor: ignore[github-env]";
    let trimmed = value.trim();
    if !trimmed.contains("zizmor: ignore[github-env]") {
        return Ok(unquote(trimmed));
    }
    let scalar = trimmed
        .strip_suffix(ANNOTATION)
        .ok_or("malformed zizmor run annotation")?;
    let scalar = scalar.trim_end();
    let quoted = scalar.starts_with('"') && scalar.ends_with('"') && scalar.len() >= 2;
    if !quoted {
        return Err("zizmor run annotation must follow a quoted scalar".into());
    }
    Ok(unquote(scalar))
}

#[cfg(test)]
#[path = "schema2_generator_release_verifier_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "schema2_generator_release_family_workflow_tests.rs"]
mod family_workflow_tests;

#[cfg(test)]
#[path = "schema2_generator_release_verifier_policy_tests.rs"]
mod policy;

#[cfg(test)]
pub(super) use policy::assert_release_workflow_verifier;
