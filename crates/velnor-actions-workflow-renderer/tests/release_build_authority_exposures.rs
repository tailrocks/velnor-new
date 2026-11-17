// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn release_authority_rejects_credential_environment_objects() -> Result<(), Box<dyn Error>> {
    let files = release_files()?;
    let mut missed = Vec::new();
    for expression in ["${{ secrets }}", "${{ fromJSON(secrets.ENVIRONMENT) }}"] {
        for scope in ["workflow", "job", "step", "composite"] {
            let mut workflow = json!({"jobs": {"build": {"steps": [{"run": "mbx build"}]}}});
            match scope {
                "workflow" => workflow["env"] = json!(expression),
                "job" => workflow["jobs"]["build"]["env"] = json!(expression),
                "step" => workflow["jobs"]["build"]["steps"][0]["env"] = json!(expression),
                "composite" => {
                    workflow["env"] = json!(expression);
                    workflow["jobs"]["build"]["steps"][0] = json!({"uses": "./.github/actions/generator-release-build-linux"});
                }
                _ => unreachable!(),
            }
            if violations(&workflow, &files)? != ["build"] {
                missed.push(format!("{scope} {expression}"));
            }
        }
    }
    assert!(missed.is_empty(), "missed {missed:?}");
    let clean = json!({"env": "${{ fromJSON(vars.PUBLIC_ENVIRONMENT) }}", "jobs": {"build": {"env": "${{ fromJSON(vars.JOB_ENVIRONMENT) }}", "steps": [{"uses": "actions/setup-example@pin", "env": "${{ secrets }}"}, {"run": "mbx build", "env": "${{ fromJSON(vars.STEP_ENVIRONMENT) }}"}]}}});
    assert_eq!(violations(&clean, &files)?, Vec::<String>::new());
    Ok(())
}

fn rejects_scope(scope: &str) -> Result<(), Box<dyn Error>> {
    let files = release_files()?;
    let mut missed = Vec::new();
    for expression in [
        "${{ github.token }}",
        "${{ github[format('{0}', 'token')] }}",
        "${{ toJSON(github.*) }}",
        "${{ secrets.RELEASE_CREDENTIAL }}",
        "${{ github['token'] }}",
        "${{ secrets['ANOTHER_PRIVATE_VALUE'] }}",
        "${{ format('}}{0}', github.token) }}",
        "${{ toJSON(github) }}",
    ] {
        let mut workflow = json!({"jobs": {"build": {"permissions": {"contents": "read"}, "steps": [{"run": "env -u GITHUB_TOKEN -u GH_TOKEN mbx build"}]}}});
        match scope {
            "workflow" => workflow["env"] = json!({"PRIVATE_VALUE": expression}),
            "job" => workflow["jobs"]["build"]["env"] = json!({"PRIVATE_VALUE": expression}),
            "step" => {
                workflow["jobs"]["build"]["steps"][0]["env"] = json!({"PRIVATE_VALUE": expression});
            }
            "with" => {
                workflow["jobs"]["build"]["steps"][0] = json!({"uses": "./.github/actions/generator-release-build-linux", "with": {"private-value": expression}});
            }
            "run" => {
                workflow["jobs"]["build"]["steps"][0]["run"] = json!(format!(
                    "env -u GITHUB_TOKEN -u GH_TOKEN mbx build --value '{expression}'"
                ));
            }
            _ => unreachable!(),
        }
        if violations(&workflow, &files)? != ["build"] {
            missed.push(expression);
        }
    }
    assert!(missed.is_empty(), "{scope} missed {missed:?}");
    Ok(())
}

#[test]
fn release_authority_rejects_credentials_in_checked_out_relative_program()
-> Result<(), Box<dyn Error>> {
    let files = release_files()?;
    let workflow = json!({"jobs": {"build": {"permissions": {"contents": "read"}, "steps": [{"uses": CHECKOUT_USES}, {"run": "./build.sh", "env": {"PRIVATE_VALUE": "${{ github.token }}"}}]}}});
    assert_eq!(violations(&workflow, &files)?, ["build"]);
    Ok(())
}

#[test]
fn release_authority_rejects_credentials_passed_to_reusable_repository_jobs()
-> Result<(), Box<dyn Error>> {
    let mut files = release_files()?;
    files.push(RenderedFile {
        path: ".github/workflows/reusable-build.yml".to_owned(),
        bytes: json!({"jobs": {"execute": {"steps": [{"run": "mbx build"}]}}}).to_string(),
    });
    for expression in ["${{ github.token }}", "${{ secrets.PRIVATE_VALUE }}"] {
        let workflow = json!({"jobs": {"build": {"uses": "./.github/workflows/reusable-build.yml", "with": {"private-value": expression}}}});
        assert_eq!(
            violations(&workflow, &files)?,
            ["build/execute"],
            "{expression}"
        );
    }
    Ok(())
}

#[test]
fn release_authority_rejects_workflow_credentials() -> Result<(), Box<dyn Error>> {
    rejects_scope("workflow")
}

#[test]
fn release_authority_rejects_job_credentials() -> Result<(), Box<dyn Error>> {
    rejects_scope("job")
}

#[test]
fn release_authority_rejects_step_credentials() -> Result<(), Box<dyn Error>> {
    rejects_scope("step")
}

#[test]
fn release_authority_rejects_with_credentials() -> Result<(), Box<dyn Error>> {
    rejects_scope("with")
}

#[test]
fn release_authority_rejects_run_credentials() -> Result<(), Box<dyn Error>> {
    rejects_scope("run")
}

#[test]
fn release_authority_allows_credentials_outside_repository_execution() -> Result<(), Box<dyn Error>>
{
    let files = release_files()?;
    let clean = json!({"env": {"PUBLIC_VALUE": "${{ vars.PUBLIC_VALUE }}"}, "jobs": {"build": {"permissions": {"contents": "read"}, "steps": [{"uses": "actions/setup-example@pin", "with": {"token": "${{ github.token }}"}}, {"run": "mbx build"}]}, "publish": {"permissions": {"contents": "write"}, "steps": [{"run": "gh release create release", "env": {"PRIVATE_VALUE": "${{ secrets.RELEASE_CREDENTIAL }}"}}]}}});
    assert_eq!(violations(&clean, &files)?, Vec::<String>::new());
    Ok(())
}

#[test]
fn release_authority_inspects_generated_composite_repository_execution()
-> Result<(), Box<dyn Error>> {
    let files = release_files()?;
    let workflow: Value = serde_saphyr::from_str(find_file(
        &files,
        ".github/workflows/product-release-generator.yml",
    )?)?;
    assert_eq!(violations(&workflow, &files)?, Vec::<String>::new());
    for scope in ["env", "with", "run"] {
        let mut mutated = files.clone();
        let file = mutated
            .iter_mut()
            .find(|file| file.path == ".github/actions/generator-release-build-linux/action.yml")
            .ok_or("missing generated build action")?;
        let mut action: Value = serde_saphyr::from_str(&file.bytes)?;
        let step = action["runs"]["steps"]
            .as_array_mut()
            .ok_or("missing composite steps")?
            .iter_mut()
            .find(|step| {
                step["run"].as_str().is_some_and(|run| {
                    run.contains("scripts/generator-release/stable-source-producer.sh")
                })
            })
            .ok_or("missing stable source command")?;
        if scope == "run" {
            let run = step["run"].as_str().ok_or("missing command")?;
            step["run"] = json!(format!("{run} '${{{{ secrets.PRIVATE_SOURCE_VALUE }}}}'"));
        } else {
            step[scope] = json!({"private-value": "${{ github.token }}"});
        }
        file.bytes = serde_json::to_string(&action)?;
        assert!(
            violations(&workflow, &mutated)?.contains(&"build-linux".to_owned()),
            "composite {scope}"
        );
    }
    Ok(())
}
