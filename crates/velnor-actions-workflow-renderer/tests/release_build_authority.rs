// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;
use serde_json::{Value, json};

#[path = "release_build_authority_exposures.rs"]
mod exposures;

fn release_files() -> Result<Vec<RenderedFile>, Box<dyn Error>> {
    let mut request = request()?;
    request.workflows.extend([
        RoutingWorkflow::MacosBinaryRelease,
        RoutingWorkflow::GeneratorRelease,
    ]);
    Ok(render_schema2_workflows(&request)?)
}

fn repository_steps(steps: &Value) -> bool {
    steps
        .as_array()
        .is_some_and(|steps| steps.iter().any(repository_step))
}

fn repository_step(step: &Value) -> bool {
    step["uses"]
        .as_str()
        .is_some_and(|uses| uses.starts_with("./"))
        || step["run"].as_str().is_some_and(|run| {
            [
                "mbx build",
                "mbx nextest",
                "cargo build",
                "cargo nextest",
                "cargo test",
                "scripts/",
                "images/",
                "./linux-assets/",
                "./macos-assets/",
                "./macos-intel-assets/",
                "./homebrew-assets/",
            ]
            .iter()
            .any(|command| run.contains(command))
                || run
                    .split_whitespace()
                    .any(|word| word.trim_start_matches(['\'', '"']).starts_with("./"))
        })
}

fn checkout_steps(steps: &Value) -> bool {
    steps.as_array().is_some_and(|steps| {
        steps.iter().any(|step| {
            step["uses"]
                .as_str()
                .is_some_and(|uses| uses.starts_with("actions/checkout@"))
        })
    })
}

fn write_permission(permissions: &Value) -> bool {
    permissions == "write-all"
        || permissions
            .as_object()
            .is_some_and(|permissions| permissions.values().any(|permission| permission == "write"))
}

fn credential_expression(text: &str) -> bool {
    let mut text = text;
    while let Some(start) = text.find("${{") {
        let (expression, tail) = expression_body(&text[start + 3..]);
        text = tail;
        let expression = expression.to_lowercase();
        let mut remaining = expression.as_str();
        let mut property = false;
        while let Some(first) = remaining.chars().next() {
            if matches!(first, '\'' | '"') {
                remaining = &remaining[first.len_utf8()..];
                remaining = remaining
                    .find(first)
                    .map_or("", |end| &remaining[end + first.len_utf8()..]);
                property = false;
            } else if first.is_ascii_alphabetic() || first == '_' {
                let end = remaining
                    .find(|character: char| !character.is_ascii_alphanumeric() && character != '_')
                    .unwrap_or(remaining.len());
                let identifier = &remaining[..end];
                remaining = &remaining[end..];
                let access = remaining.trim_start();
                let dot = access.strip_prefix('.').map(str::trim_start);
                let bracket = access.strip_prefix('[').map(str::trim_start);
                let dynamic = dot.is_some_and(|member| member.starts_with('*'))
                    || bracket.is_some_and(|member| !member.starts_with(['\'', '"']));
                let token = dot.is_some_and(|member| {
                    member.strip_prefix("token").is_some_and(|tail| {
                        !tail.starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_')
                    })
                }) || bracket.is_some_and(|member| {
                    ["'token'", "\"token\""].iter().any(|key| {
                        member
                            .strip_prefix(key)
                            .is_some_and(|tail| tail.trim_start().starts_with(']'))
                    })
                });
                if !property
                    && (identifier == "secrets"
                        || (identifier == "github"
                            && (token || dynamic || (dot.is_none() && bracket.is_none()))))
                {
                    return true;
                }
                property = false;
            } else {
                if !first.is_whitespace() {
                    property = first == '.';
                }
                remaining = &remaining[first.len_utf8()..];
            }
        }
    }
    false
}

fn expression_body(expression: &str) -> (&str, &str) {
    let mut quote = None;
    let mut characters = expression.char_indices().peekable();
    while let Some((index, character)) = characters.next() {
        if let Some(delimiter) = quote {
            if character == delimiter {
                if characters
                    .peek()
                    .is_some_and(|(_, next)| *next == delimiter)
                {
                    characters.next();
                } else {
                    quote = None;
                }
            }
        } else if matches!(character, '\'' | '"') {
            quote = Some(character);
        } else if character == '}' && characters.peek().is_some_and(|(_, next)| *next == '}') {
            return (&expression[..index], &expression[index + 2..]);
        }
    }
    (expression, "")
}

fn credential_value(value: &Value) -> bool {
    match value {
        Value::String(text) => credential_expression(text),
        Value::Array(values) => values.iter().any(credential_value),
        Value::Object(values) => values.values().any(credential_value),
        _ => false,
    }
}

#[derive(Default)]
struct Environment {
    bindings: Value,
    dynamic_credentials: bool,
}

fn effective_env(parent: &Environment, scope: &Value) -> Environment {
    let mut env = parent.bindings.as_object().cloned().unwrap_or_default();
    if let Some(local) = scope["env"].as_object() {
        env.extend(local.clone());
    }
    Environment {
        bindings: Value::Object(env),
        dynamic_credentials: parent.dynamic_credentials
            || (!scope["env"].is_object() && credential_value(&scope["env"])),
    }
}

fn exposed_steps(
    steps: &Value,
    files: &[RenderedFile],
    inherited_env: &Environment,
    visiting: &mut BTreeSet<String>,
) -> Result<bool, Box<dyn Error>> {
    let Some(steps) = steps.as_array() else {
        return Ok(false);
    };
    let mut found = false;
    for step in steps {
        let env = effective_env(inherited_env, step);
        if repository_step(step) {
            found |= env.dynamic_credentials
                || credential_value(&env.bindings)
                || credential_value(&step["with"])
                || credential_value(&step["run"]);
        }
        if let Some(path) = step["uses"]
            .as_str()
            .and_then(|uses| uses.strip_prefix("./"))
        {
            let path = format!("{path}/action.yml");
            assert!(visiting.insert(path.clone()), "cyclic local action {path}");
            let action: Value = serde_saphyr::from_str(find_file(files, &path)?)?;
            found |= exposed_steps(&action["runs"]["steps"], files, &env, visiting)?;
            visiting.remove(&path);
        }
    }
    Ok(found)
}

fn violations(workflow: &Value, files: &[RenderedFile]) -> Result<Vec<String>, Box<dyn Error>> {
    inspect_jobs(workflow, files, &Value::Null, false, &mut BTreeSet::new())
}

fn inspect_jobs(
    workflow: &Value,
    files: &[RenderedFile],
    inherited: &Value,
    input_credentials: bool,
    visiting: &mut BTreeSet<String>,
) -> Result<Vec<String>, Box<dyn Error>> {
    let jobs = workflow["jobs"].as_object().ok_or("workflow has no jobs")?;
    let defaults = workflow.get("permissions").unwrap_or(inherited);
    let mut found = Vec::new();
    for (name, job) in jobs {
        let permissions = job.get("permissions").unwrap_or(defaults);
        if let Some(path) = job["uses"]
            .as_str()
            .and_then(|uses| uses.strip_prefix("./"))
        {
            assert!(
                visiting.insert(path.to_owned()),
                "cyclic local workflow {path}"
            );
            let callee: Value = serde_saphyr::from_str(find_file(files, path)?)?;
            found.extend(
                inspect_jobs(
                    &callee,
                    files,
                    permissions,
                    input_credentials || credential_value(&job["with"]),
                    visiting,
                )?
                .into_iter()
                .map(|child| format!("{name}/{child}")),
            );
            visiting.remove(path);
        } else {
            let env = effective_env(&effective_env(&Environment::default(), workflow), job);
            if (repository_steps(&job["steps"])
                && (write_permission(permissions) || input_credentials))
                || (checkout_steps(&job["steps"])
                    && (permissions == "write-all" || permissions["actions"] == "write"))
                || exposed_steps(&job["steps"], files, &env, visiting)?
            {
                found.push(name.clone());
            }
        }
    }
    Ok(found)
}

#[test]
fn release_repository_code_has_read_only_job_permissions() -> Result<(), Box<dyn Error>> {
    let files = release_files()?;
    for path in [
        ".github/workflows/product-release.yml",
        ".github/workflows/product-release-images.yml",
        ".github/workflows/product-release-binary.yml",
        ".github/workflows/product-release-generator.yml",
    ] {
        let workflow: Value = serde_saphyr::from_str(find_file(&files, path)?)?;
        assert_eq!(
            violations(&workflow, &files)?,
            Vec::<String>::new(),
            "{path}"
        );
    }
    for platform in ["linux", "macos", "macos-intel"] {
        let path = format!(".github/actions/generator-release-build-{platform}/action.yml");
        let action: Value = serde_saphyr::from_str(find_file(&files, &path)?)?;
        assert!(
            !exposed_steps(
                &action["runs"]["steps"],
                &files,
                &Environment::default(),
                &mut BTreeSet::new()
            )?,
            "{path}"
        );
    }
    Ok(())
}

#[test]
fn release_authority_check_rejects_writers_even_when_tokens_are_stripped()
-> Result<(), Box<dyn Error>> {
    let files = release_files()?;
    for (family, jobs) in [
        ("images", &["build-images"][..]),
        ("binary", &["build-binary"][..]),
        (
            "generator",
            &[
                "build-linux",
                "build-macos",
                "build-macos-intel",
                "candidate-manifest",
                "qualify-linux",
                "qualify-macos",
                "qualify-macos-intel",
                "qualify-homebrew-macos",
            ][..],
        ),
    ] {
        let path = format!(".github/workflows/product-release-{family}.yml");
        let workflow: Value = serde_saphyr::from_str(find_file(&files, &path)?)?;
        for job in jobs {
            let mut mutated = workflow.clone();
            assert!(mutated["jobs"][*job].is_object(), "{job}");
            mutated["jobs"][*job]["permissions"]["actions"] = json!("write");
            assert!(
                violations(&mutated, &files)?.contains(&(*job).to_owned()),
                "{path} {job}"
            );
        }
    }
    let stripped = json!({"jobs": {"build": {"steps": [{"run": "env -u GITHUB_TOKEN -u GH_TOKEN -u MISE_GITHUB_TOKEN -u ACTIONS_RUNTIME_TOKEN -u ACTIONS_ID_TOKEN_REQUEST_TOKEN mbx build"}]}}});
    for permissions in [json!("write-all"), json!({"actions": "write"})] {
        let mut inherited = stripped.clone();
        inherited["permissions"] = permissions;
        assert_eq!(violations(&inherited, &files)?, ["build"]);
        inherited["jobs"]["build"]["permissions"] = json!({"contents": "read"});
        assert_eq!(violations(&inherited, &files)?, Vec::<String>::new());
    }
    for run in [
        "./scripts/build.sh",
        "cargo test",
        "bun scripts/build.ts",
        "./images/build.sh",
    ] {
        let workflow = json!({"jobs": {"build": {"permissions": {"contents": "write"}, "steps": [{"run": run}]}}});
        assert_eq!(violations(&workflow, &files)?, ["build"], "{run}");
    }
    for permissions in [json!("write-all"), json!({"actions": "write"})] {
        let workflow = json!({"jobs": {"build": {"permissions": permissions, "steps": [{"uses": CHECKOUT_USES}, {"run": "./unrecognized-program"}]}}});
        assert_eq!(violations(&workflow, &files)?, ["build"]);
    }
    let publisher = json!({"jobs": {"publish": {"permissions": {"actions": "read", "contents": "write"}, "steps": [{"run": "gh release create release dist/*"}]}}});
    assert_eq!(violations(&publisher, &files)?, Vec::<String>::new());
    Ok(())
}
