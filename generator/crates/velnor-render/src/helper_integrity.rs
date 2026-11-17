// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{HelperRelease, Job, StepKind};

use crate::gate::all_conditions;
use crate::rules::{RenderedStep, SETUP_VELNOR_ACTION, Value};

const DIRECTORY: &str = "${{ runner.temp }}/velnor-trusted-${{ github.run_id }}-${{ github.run_attempt }}-${{ github.job }}";
const CHECKSUM: &str = "${{ runner.os == 'Linux' && '/usr/bin/sha256sum' || runner.os == 'macOS' && '/usr/bin/shasum' || '' }}";
const CHECKSUM_ARGUMENTS: &str = "${{ runner.os == 'Linux' && '--check --status' || runner.os == 'macOS' && '-a 256 --check --status' || '' }}";

pub(crate) fn secure_steps(job: &Job, steps: Vec<RenderedStep>) -> Vec<RenderedStep> {
    let Some(release) = job.steps.iter().find_map(|step| match &step.kind {
        StepKind::InstallHelper(release) => Some(release),
        _ => None,
    }) else {
        return steps;
    };
    let mut secured = Vec::new();
    for mut step in steps {
        if step
            .uses
            .as_ref()
            .is_some_and(|action| action.starts_with(&format!("{SETUP_VELNOR_ACTION}@")))
        {
            let condition = step.condition.clone();
            secured.push(step);
            secured.extend(install_copy(condition.as_deref()));
            continue;
        }
        if let Some(arguments) = step
            .run
            .as_ref()
            .and_then(|run| run.strip_prefix("velnor-ci "))
        {
            let id = format!("velnor-helper-check-{}", secured.len());
            secured.push(verification(&id, step.condition.clone(), release));
            step.run = Some(format!("\"$VELNOR_HELPER\" {arguments}"));
            step.env.push(helper_path());
            step.condition = all_conditions(
                step.condition,
                Some(format!("${{{{ steps.{id}.outcome == 'success' }}}}")),
            );
        }
        secured.push(step);
    }
    secured
}

fn helper_path() -> (String, Value) {
    (
        "VELNOR_HELPER".to_owned(),
        Value::Expr(format!("{DIRECTORY}/velnor-ci")),
    )
}

fn install_copy(condition: Option<&str>) -> Vec<RenderedStep> {
    [
        (
            "Create the released helper directory",
            "/bin/mkdir -m 700 \"$VELNOR_HELPER_DIRECTORY\"",
        ),
        (
            "Copy the released helper",
            "/bin/cp \"$VELNOR_HELPER_SOURCE\" \"$VELNOR_HELPER\"",
        ),
        (
            "Protect the released helper",
            "/bin/chmod 500 \"$VELNOR_HELPER_DIRECTORY\" \"$VELNOR_HELPER\"",
        ),
    ]
    .into_iter()
    .map(|(name, run)| RenderedStep {
        name: name.to_owned(),
        condition: condition.map(str::to_owned),
        env: vec![
            helper_path(),
            (
                "VELNOR_HELPER_DIRECTORY".to_owned(),
                Value::Expr(DIRECTORY.to_owned()),
            ),
            (
                "VELNOR_HELPER_SOURCE".to_owned(),
                Value::Expr("${{ runner.temp }}/velnor-ci-bin/velnor-ci".to_owned()),
            ),
        ],
        run: Some(run.to_owned()),
        ..RenderedStep::default()
    })
    .collect()
}

fn verification(id: &str, condition: Option<String>, release: &HelperRelease) -> RenderedStep {
    let choices: Vec<String> = release
        .digests()
        .iter()
        .map(|(platform, digest)| {
            let (os, arch) = platform.runner_key().split_once('-').unwrap_or(("", ""));
            format!("runner.os == '{os}' && runner.arch == '{arch}' && '{digest}'")
        })
        .collect();
    RenderedStep {
        id: Some(id.to_owned()),
        name: "Verify the released helper".to_owned(),
        condition,
        env: vec![
            helper_path(),
            ("VELNOR_HELPER_SHA256".to_owned(), Value::Expr(format!("${{{{ {} || '' }}}}", choices.join(" || ")))),
            ("VELNOR_HELPER_CHECKSUM".to_owned(), Value::Expr(CHECKSUM.to_owned())),
            ("VELNOR_HELPER_CHECKSUM_ARGUMENTS".to_owned(), Value::Expr(CHECKSUM_ARGUMENTS.to_owned())),
        ],
        run: Some("\"$VELNOR_HELPER_CHECKSUM\" $VELNOR_HELPER_CHECKSUM_ARGUMENTS <<< \"$VELNOR_HELPER_SHA256  $VELNOR_HELPER\"".to_owned()),
        ..RenderedStep::default()
    }
}
