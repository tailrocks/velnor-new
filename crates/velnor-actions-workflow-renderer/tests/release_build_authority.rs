// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;
use serde_json::{Value, json};

fn release_files() -> Result<Vec<RenderedFile>, Box<dyn Error>> {
    let mut request = request()?;
    request.workflows.extend([
        RoutingWorkflow::MacosBinaryRelease,
        RoutingWorkflow::GeneratorRelease,
    ]);
    Ok(render_schema2_workflows(&request)?)
}

fn repository_steps(steps: &Value) -> bool {
    steps.as_array().is_some_and(|steps| {
        steps.iter().any(|step| {
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
                })
        })
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

fn violations(workflow: &Value, files: &[RenderedFile]) -> Result<Vec<String>, Box<dyn Error>> {
    inspect_jobs(workflow, files, &Value::Null, &mut BTreeSet::new())
}

fn inspect_jobs(
    workflow: &Value,
    files: &[RenderedFile],
    inherited: &Value,
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
                inspect_jobs(&callee, files, permissions, visiting)?
                    .into_iter()
                    .map(|child| format!("{name}/{child}")),
            );
            visiting.remove(path);
        } else if (repository_steps(&job["steps"]) && write_permission(permissions))
            || (checkout_steps(&job["steps"])
                && (permissions == "write-all" || permissions["actions"] == "write"))
        {
            found.push(name.clone());
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
        assert!(repository_steps(&action["runs"]["steps"]), "{path}");
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
