// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::Value;
use std::error::Error;
use std::path::Path;
use velnor_actions_orchestrator::{prepare, render_staged_tree};
use velnor_actions_workflow_renderer::render::RenderedTree;

use super::seed_off::seed_repo;
use super::waste_oracle::{Files, files_of};
use super::workflow_configuration_fixture;

#[path = "ubuntu_runner_policy_arm.rs"]
mod arm;
#[path = "ubuntu_runner_policy_profiles.rs"]
mod profiles;

const WORKFLOWS: [&str; 7] = [
    ".github/workflows/qualification.yml",
    ".github/workflows/product-release.yml",
    ".github/workflows/product-release-images.yml",
    ".github/workflows/product-release-binary.yml",
    ".github/workflows/product-release-generator.yml",
    ".github/workflows/source-qualification.yml",
    ".github/workflows/job-image-candidate.yml",
];

fn parsed(text: &str, path: &str) -> Result<Value, Box<dyn Error>> {
    serde_saphyr::from_str(text).map_err(|error| format!("{path}: {error}").into())
}

fn matrix_values(job: &Value, key: &str) -> Result<Vec<String>, Box<dyn Error>> {
    let matrix = job
        .get("strategy")
        .and_then(|strategy| strategy.get("matrix"))
        .ok_or_else(|| format!("matrix.{key} has no finite strategy domain"))?;
    let mut values = Vec::new();
    if let Some(value) = matrix.get(key) {
        match value {
            Value::String(value) => values.push(value.clone()),
            Value::Array(items) => {
                for item in items {
                    values.push(
                        item.as_str()
                            .ok_or_else(|| format!("matrix.{key} contains a non-string"))?
                            .to_owned(),
                    );
                }
            }
            _ => return Err(format!("matrix.{key} has an unknown domain shape").into()),
        }
    }
    if let Some(rows) = matrix.get("include").and_then(Value::as_array) {
        for row in rows {
            if let Some(value) = row.get(key) {
                values.push(
                    value
                        .as_str()
                        .ok_or_else(|| format!("matrix.include.{key} contains a non-string"))?
                        .to_owned(),
                );
            }
        }
    }
    values.sort();
    values.dedup();
    if values.is_empty() {
        return Err(format!("matrix.{key} has no finite values").into());
    }
    Ok(values)
}

fn expression_values(expression: &str, job: &Value) -> Result<Vec<String>, Box<dyn Error>> {
    let mut values = Vec::new();
    for alternative in expression.split("||") {
        let alternative = alternative.trim();
        if let Some(key) = alternative.strip_prefix("matrix.") {
            if !key
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
            {
                return Err(format!("unrecognized matrix selector {alternative}").into());
            }
            values.extend(matrix_values(job, key)?);
        } else if (alternative.starts_with('\'') && alternative.ends_with('\''))
            || (alternative.starts_with('"') && alternative.ends_with('"'))
        {
            values.push(alternative[1..alternative.len() - 1].to_owned());
        } else {
            return Err(format!("unresolved runner expression {alternative}").into());
        }
    }
    values.sort();
    values.dedup();
    Ok(values)
}

fn selector_values(selector: &str, job: &Value) -> Result<Vec<String>, Box<dyn Error>> {
    let mut selections = vec![String::new()];
    let mut rest = selector;
    while let Some(start) = rest.find("${{") {
        let prefix = &rest[..start];
        for selection in &mut selections {
            selection.push_str(prefix);
        }
        let expression_start = start + 3;
        let end = rest[expression_start..]
            .find("}}")
            .ok_or("unterminated runner expression")?
            + expression_start;
        let candidates = expression_values(rest[expression_start..end].trim(), job)?;
        let mut expanded = Vec::new();
        for selection in selections {
            for candidate in &candidates {
                expanded.push(format!("{selection}{candidate}"));
            }
        }
        selections = expanded;
        rest = &rest[end + 2..];
    }
    for selection in &mut selections {
        selection.push_str(rest);
    }
    Ok(selections)
}

fn check_runner_value(
    value: &str,
    job: &Value,
    location: &str,
    ubuntu: &mut Vec<String>,
) -> Result<(), Box<dyn Error>> {
    for resolved in selector_values(value, job)? {
        if !resolved.contains("${{") && resolved.starts_with("ubuntu-") {
            let arm_task = location == ".github/workflows/ci.yml job task-arm-image";
            if !(matches!(resolved.as_str(), "ubuntu-26.04" | "ubuntu-26.04-scale-set")
                || resolved == "ubuntu-26.04-arm" && arm_task)
            {
                return Err(
                    format!("{location} selects forbidden Ubuntu runner {resolved}").into(),
                );
            }
            ubuntu.push(resolved);
        }
    }
    Ok(())
}

fn check_generated_workflow_runners(files: &Files) -> Result<Vec<String>, Box<dyn Error>> {
    let mut ubuntu = Vec::new();
    let mut workflow_count = 0;
    for (path, text) in files.iter().filter(|(path, _)| {
        path.starts_with(".github/workflows/")
            && Path::new(path)
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("yml"))
    }) {
        workflow_count += 1;
        let workflow = parsed(text, path)?;
        let jobs = workflow
            .get("jobs")
            .and_then(Value::as_object)
            .ok_or_else(|| format!("{path} has no jobs map"))?;
        for (id, job) in jobs {
            let location = format!("{path} job {id}");
            let Some(runs_on) = job.get("runs-on") else {
                if job.get("uses").and_then(Value::as_str).is_some() {
                    continue;
                }
                return Err(format!("{location} has no runner or reusable workflow").into());
            };
            match runs_on {
                Value::String(value) => {
                    check_runner_value(value, job, &location, &mut ubuntu)?;
                }
                Value::Array(values) => {
                    if values.is_empty() {
                        return Err(format!("{location} has an empty runs-on list").into());
                    }
                    for value in values {
                        let value = value
                            .as_str()
                            .ok_or_else(|| format!("{location} has a non-string runner label"))?;
                        check_runner_value(value, job, &location, &mut ubuntu)?;
                    }
                }
                _ => return Err(format!("{location} has an unresolved runs-on shape").into()),
            }
        }
    }
    if workflow_count == 0 || ubuntu.is_empty() {
        return Err("staged tree contains no inspectable Ubuntu workflow jobs".into());
    }
    Ok(ubuntu)
}

fn routed_release_repo_with(
    hosted_label: &str,
    hosted_platform: &str,
) -> Result<tempfile::TempDir, Box<dyn Error>> {
    let config = format!(
        "schema = 2\n\n[workflow]\nname = \"CI\"\npolicy = \"consumer-v1\"\ndefault_branch = \"testmain\"\n\n[execution]\ndefault_profile = \"hosted\"\nhosted_profile = \"hosted\"\nscale_set_profile = \"local\"\nworkflows = [\"qualification\", \"image_release\", \"macos_binary_release\", \"generator_release\"]\n\n[execution.profiles.hosted]\nkind = \"github-hosted\"\nlabel = \"{hosted_label}\"\nplatform = \"{hosted_platform}\"\n\n[execution.profiles.local]\nkind = \"github-scale-set\"\nname = \"ubuntu-26.04-scale-set\"\nlabels = [\"velnor\", \"ubuntu-26.04-scale-set\"]\nplatform = \"linux/amd64\"\n"
    );
    workflow_configuration_fixture::make_repo(&config)
}

fn routed_release_repo() -> Result<tempfile::TempDir, Box<dyn Error>> {
    routed_release_repo_with("ubuntu-26.04", "linux/amd64")
}

fn rendered_files(tree: &RenderedTree) -> Result<Files, Box<dyn Error>> {
    files_of(tree)
}

fn assert_job(workflow: &Value, path: &str, id: &str) -> Result<(), Box<dyn Error>> {
    if workflow.get("jobs").and_then(|jobs| jobs.get(id)).is_some() {
        Ok(())
    } else {
        Err(format!("{path} did not render job {id}").into())
    }
}

#[test]
fn staged_consumer_and_conditional_release_workflows_select_only_ubuntu_26_04()
-> Result<(), Box<dyn Error>> {
    let consumer = seed_repo("")?;
    let consumer_files = rendered_files(&render_staged_tree(&prepare(consumer.path())?)?)?;
    let ci_path = ".github/workflows/ci.yml";
    let ci = parsed(
        consumer_files
            .get(ci_path)
            .ok_or("consumer CI workflow was not staged")?,
        ci_path,
    )?;
    if !ci
        .get("jobs")
        .and_then(Value::as_object)
        .is_some_and(|jobs| {
            jobs.values()
                .any(|job| job.get("name") == Some(&Value::String("Required".to_owned())))
        })
    {
        return Err("consumer CI did not render the Required gate".into());
    }
    if !consumer_files.contains_key(".github/workflows/state-seal.yml")
        || !consumer_files.contains_key(".github/workflows/cache-prune.yml")
    {
        return Err("consumer fixture did not render state and cache producer workflows".into());
    }
    if !consumer_files
        .get(ci_path)
        .is_some_and(|text| text.contains("./.github/actions/velnor-tools-prelude-u26"))
    {
        return Err("consumer fixture did not render its cache identity lane".into());
    }
    check_generated_workflow_runners(&consumer_files)?;

    let releases = routed_release_repo()?;
    let release_files = rendered_files(&render_staged_tree(&prepare(releases.path())?)?)?;
    check_conditional_release_workflows(&release_files)?;
    Ok(())
}

fn check_conditional_release_workflows(release_files: &Files) -> Result<(), Box<dyn Error>> {
    for path in WORKFLOWS {
        if !release_files.contains_key(path) {
            return Err(format!("conditional producer workflow {path} was not staged").into());
        }
    }
    let images = parsed(
        release_files[".github/workflows/product-release-images.yml"].as_str(),
        ".github/workflows/product-release-images.yml",
    )?;
    assert_job(
        &images,
        ".github/workflows/product-release-images.yml",
        "build-images",
    )?;
    assert_job(
        &images,
        ".github/workflows/product-release-images.yml",
        "attest-images",
    )?;
    assert_job(
        &images,
        ".github/workflows/product-release-images.yml",
        "publish-images",
    )?;
    let binary = parsed(
        release_files[".github/workflows/product-release-binary.yml"].as_str(),
        ".github/workflows/product-release-binary.yml",
    )?;
    assert_job(
        &binary,
        ".github/workflows/product-release-binary.yml",
        "build-binary",
    )?;
    assert_job(
        &binary,
        ".github/workflows/product-release-binary.yml",
        "attest-binary",
    )?;
    assert_job(
        &binary,
        ".github/workflows/product-release-binary.yml",
        "publish-binary",
    )?;
    let macos = release_files
        .get(".github/workflows/product-release-binary.yml")
        .ok_or("binary release workflow disappeared")?;
    if !macos.contains("macos-") {
        return Err("binary release lost its explicit macOS platform jobs".into());
    }
    check_generated_workflow_runners(release_files)?;
    Ok(())
}

#[test]
fn prepare_rejects_older_floating_and_unversioned_ubuntu_overrides() -> Result<(), Box<dyn Error>> {
    for label in ["ubuntu-24.04", "ubuntu-22.04", "ubuntu-latest"] {
        let repo = seed_repo(&format!("runner_label = \"{label}\"\n"))?;
        let error = match prepare(repo.path()) {
            Ok(_) => return Err(format!("prepare accepted forbidden runner {label}").into()),
            Err(error) => error.to_string(),
        };
        for expected in [
            ".velnor/config.toml",
            "workflow.runner_label",
            "unsupported_label",
        ] {
            if !error.contains(expected) {
                return Err(format!("{label} diagnostic omitted {expected}: {error}").into());
            }
        }
    }
    Ok(())
}

#[test]
fn prepare_accepts_an_explicit_ubuntu_26_04_override() -> Result<(), Box<dyn Error>> {
    let repo = seed_repo("runner_label = \"ubuntu-26.04\"\n")?;
    let prepared = prepare(repo.path())?;
    let files = rendered_files(&render_staged_tree(&prepared)?)?;
    check_generated_workflow_runners(&files)?;
    Ok(())
}

#[test]
fn prepare_rejects_arm_as_an_ordinary_workflow_runner_override() -> Result<(), Box<dyn Error>> {
    let repo = seed_repo("runner_label = \"ubuntu-26.04-arm\"\n")?;
    let error = match prepare(repo.path()) {
        Ok(_) => return Err("prepare accepted ARM as an ordinary workflow runner".into()),
        Err(error) => error.to_string(),
    };
    for expected in [
        ".velnor/config.toml",
        "workflow.runner_label",
        "unsupported_label",
        "ubuntu-26.04-arm",
    ] {
        if !error.contains(expected) {
            return Err(
                format!("ordinary ARM override diagnostic omitted {expected}: {error}").into(),
            );
        }
    }
    Ok(())
}
