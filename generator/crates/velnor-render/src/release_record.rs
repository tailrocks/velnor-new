// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{HelperRelease, RELEASE_PLZ_FILE};
use velnor_release::{RELEASE_APP_SLUG, ReleasePlan};

use crate::release::VERIFIED_COMMIT;
use crate::release_steps::{
    COMMIT_OUTPUT_SUFFIX, TAGGED_COMMIT_HELPER, TOOLCHAIN, join, shell_word, variables,
};
use crate::rules::{RenderedStep, Value};

pub(crate) fn record(plan: &ReleasePlan, helper: &HelperRelease, files: &[String]) -> RenderedStep {
    let files = files.join(" ");
    if !helper.version().at_least(TAGGED_COMMIT_HELPER) {
        return RenderedStep {
            id: Some("record".to_owned()),
            name: "Record the released packages".to_owned(),
            run: Some(format!("velnor-ci release record {files}")),
            ..RenderedStep::default()
        };
    }
    RenderedStep {
        id: Some("record".to_owned()),
        name: "Record the released packages and the commits their tags are on".to_owned(),
        env: {
            let mut env = variables(&[
                ("GITHUB_TOKEN", "${{ github.token }}"),
                ("REPOSITORY", "${{ github.repository }}"),
                ("COMMIT", VERIFIED_COMMIT),
            ]);
            env.push((
                "RUSTUP_TOOLCHAIN".to_owned(),
                Value::Text(TOOLCHAIN.to_owned()),
            ));
            env
        },
        run: Some(format!(
            "velnor-ci release record --repo \"$REPOSITORY\" --verified-commit \"$COMMIT\" --app {RELEASE_APP_SLUG} {} {files}",
            configs(plan)
        )),
        ..RenderedStep::default()
    }
}

fn configs(plan: &ReleasePlan) -> String {
    plan.roots
        .iter()
        .map(|root| {
            format!(
                "--config {}",
                shell_word(&join(&root.root, RELEASE_PLZ_FILE))
            )
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn commit_outputs(plan: &ReleasePlan, helper: &HelperRelease) -> Vec<(String, String)> {
    if !helper.version().at_least(TAGGED_COMMIT_HELPER) {
        return Vec::new();
    }
    plan.binaries
        .iter()
        .map(|binary| {
            let output = commit_output(&binary.package);
            (
                output.clone(),
                format!("${{{{ steps.record.outputs.{output} }}}}"),
            )
        })
        .collect()
}

pub(crate) fn commit_output(package: &str) -> String {
    format!("{package}{COMMIT_OUTPUT_SUFFIX}")
}
