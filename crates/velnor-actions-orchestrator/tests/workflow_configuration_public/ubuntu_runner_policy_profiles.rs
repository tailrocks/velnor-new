// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_actions_contract::{ExecutionConfig, ExecutionProfile};
use velnor_actions_orchestrator::{prepare, render_staged_tree};

use super::{check_generated_workflow_runners, rendered_files, routed_release_repo_with};

#[test]
fn hosted_execution_profiles_reject_arm_label_and_platform_pairs() -> Result<(), Box<dyn Error>> {
    for (platform, field, problem) in [
        (
            "linux/amd64",
            "execution.profiles.hosted.label",
            "unsupported_label",
        ),
        (
            "linux/arm64",
            "execution.profiles.platform",
            "platform_mismatch",
        ),
    ] {
        let repo = routed_release_repo_with("ubuntu-26.04-arm", platform)?;
        let error = match prepare(repo.path()) {
            Ok(_) => {
                return Err(format!(
                    "prepare accepted ubuntu-26.04-arm with hosted platform {platform}"
                )
                .into());
            }
            Err(error) => error.to_string(),
        };
        for expected in [".velnor/config.toml", field, problem] {
            if !error.contains(expected) {
                return Err(
                    format!("hosted ARM profile diagnostic omitted {expected}: {error}").into(),
                );
            }
        }
    }

    let repo = routed_release_repo_with("ubuntu-26.04", "linux/amd64")?;
    let prepared = prepare(repo.path())?;
    let files = rendered_files(&render_staged_tree(&prepared)?)?;
    check_generated_workflow_runners(&files)?;

    for result in [
        ExecutionProfile::hosted("ubuntu-26.04").map(|profile| profile.platform),
        ExecutionConfig::hosted_default("ubuntu-26.04")
            .map(|config| config.profiles["hosted"].platform.clone()),
    ] {
        if result? != "linux/amd64" {
            return Err("hosted constructor did not select linux/amd64".into());
        }
    }
    for error in [
        ExecutionProfile::hosted("ubuntu-26.04-arm").err(),
        ExecutionConfig::hosted_default("ubuntu-26.04-arm").err(),
    ] {
        let error = error.ok_or("hosted constructor accepted ubuntu-26.04-arm")?;
        for expected in ["execution.profiles.hosted.label", "unsupported_label"] {
            if !error.to_string().contains(expected) {
                return Err(
                    format!("hosted constructor diagnostic omitted {expected}: {error}").into(),
                );
            }
        }
    }
    Ok(())
}
