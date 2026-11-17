// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{HelperRelease, Job, StepKind};

use crate::rules::{RenderedStep, SETUP_VELNOR_ACTION, Value};

const DIRECTORY: &str = "${{ runner.temp }}/velnor-trusted-${{ github.run_id }}-${{ github.run_attempt }}-${{ github.job }}";
const CHECKSUM: &str = "${{ runner.os == 'Linux' && '/usr/bin/sha256sum' || runner.os == 'macOS' && '/usr/bin/shasum' || '' }}";
const CHECKSUM_ARGUMENTS: &str = "${{ runner.os == 'Linux' && '--check --status' || runner.os == 'macOS' && '-a 256 --check --status' || '' }}";

pub(crate) const INVOCATION_SHELL: &str = "/bin/bash --noprofile --norc -p -e -o pipefail {0}";

pub(crate) const INVOCATION_PREFIX: &str =
    "/bin/bash --noprofile --norc -p -e -o pipefail -c \"$VELNOR_HELPER_LAUNCHER\" -- ";

pub(crate) const LAUNCHER: &str = "VELNOR_HELPER_DIRECTORY=$(/usr/bin/mktemp -d \"$VELNOR_HELPER_TEMP/velnor-helper.XXXXXXXXXX\") && VELNOR_HELPER_COPY=$(/usr/bin/mktemp \"$VELNOR_HELPER_DIRECTORY/velnor-ci.XXXXXXXXXX\") && /bin/cp \"$VELNOR_HELPER\" \"$VELNOR_HELPER_COPY\" && /bin/chmod 500 \"$VELNOR_HELPER_COPY\" && { /usr/bin/env -i PATH=/usr/bin:/bin \"$VELNOR_HELPER_CHECKSUM\" $VELNOR_HELPER_CHECKSUM_ARGUMENTS <<< \"$VELNOR_HELPER_SHA256  $VELNOR_HELPER_COPY\" || { /usr/bin/printf '%s\\n' 'velnor-ci integrity check failed' >&2; exit 1; }; } && exec \"$VELNOR_HELPER_COPY\" \"$@\"";

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
            step.run = Some(format!("{INVOCATION_PREFIX}{arguments}"));
            step.shell = Some(INVOCATION_SHELL.to_owned());
            step.env.extend(verification_environment(release));
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
            "/bin/chmod 500 \"$VELNOR_HELPER\"",
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

fn verification_environment(release: &HelperRelease) -> Vec<(String, Value)> {
    let choices: Vec<String> = release
        .digests()
        .iter()
        .map(|(platform, digest)| {
            let (os, arch) = platform.runner_key().split_once('-').unwrap_or(("", ""));
            format!("runner.os == '{os}' && runner.arch == '{arch}' && '{digest}'")
        })
        .collect();
    vec![
        (
            "VELNOR_HELPER_LAUNCHER".to_owned(),
            Value::Text(LAUNCHER.to_owned()),
        ),
        helper_path(),
        (
            "VELNOR_HELPER_TEMP".to_owned(),
            Value::Expr("${{ runner.temp }}".to_owned()),
        ),
        (
            "VELNOR_HELPER_SHA256".to_owned(),
            Value::Expr(format!("${{{{ {} || '' }}}}", choices.join(" || "))),
        ),
        (
            "VELNOR_HELPER_CHECKSUM".to_owned(),
            Value::Expr(CHECKSUM.to_owned()),
        ),
        (
            "VELNOR_HELPER_CHECKSUM_ARGUMENTS".to_owned(),
            Value::Expr(CHECKSUM_ARGUMENTS.to_owned()),
        ),
    ]
}
