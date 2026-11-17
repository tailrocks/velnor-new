// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{HelperRelease, Job, StepKind};

use crate::rules::{RenderedStep, SETUP_VELNOR_ACTION, Value};

const DIRECTORY: &str = "${{ runner.temp }}/velnor-trusted-${{ github.run_id }}-${{ github.run_attempt }}-${{ github.job }}";

pub(crate) const INVOCATION_SHELL: &str = "bash --noprofile --norc -p -e -o pipefail {0}";

pub(crate) const INVOCATION_PREFIX: &str = "\"$VELNOR_HELPER_BASH\" --noprofile --norc -p -e -o pipefail -c \"$VELNOR_HELPER_LAUNCHER\" -- ";

pub(crate) const LAUNCHER: &str = "VELNOR_HELPER_DIRECTORY=$(\"$VELNOR_HELPER_MKTEMP\" -d \"$VELNOR_HELPER_TEMP/velnor-helper.XXXXXXXXXX\") && VELNOR_HELPER_COPY=$(\"$VELNOR_HELPER_MKTEMP\" \"$VELNOR_HELPER_DIRECTORY/velnor-ci.XXXXXXXXXX\") && \"$VELNOR_HELPER_CP\" \"$VELNOR_HELPER\" \"$VELNOR_HELPER_COPY\" && \"$VELNOR_HELPER_CHMOD\" 500 \"$VELNOR_HELPER_COPY\" && { \"$VELNOR_HELPER_ENV\" -i PATH=/usr/bin:/bin \"$VELNOR_HELPER_CHECKSUM\" $VELNOR_HELPER_CHECKSUM_ARGUMENTS <<< \"$VELNOR_HELPER_SHA256  $VELNOR_HELPER_COPY\" || { printf '%s\\n' 'velnor-ci integrity check failed' >&2; exit 1; }; } && exec \"$VELNOR_HELPER_COPY\" \"$@\"";

const INSTALL_TOOLS: &str = "fail() { printf '%s\n' \"::error title=setup-velnor::$1\"; exit 1; }; resolve_tool() { local path; path=\"$(command -v \"$2\")\" && [[ \"$path\" = /* && -f \"$path\" && -x \"$path\" ]] || fail \"required tool '$2' was not found on PATH\"; printf '%s=%s\n' \"$1\" \"$path\" >> \"$GITHUB_ENV\"; }; for binding in BASH:bash MKTEMP:mktemp CP:cp CHMOD:chmod MKDIR:mkdir ENV:env; do resolve_tool \"VELNOR_HELPER_${binding%:*}\" \"${binding#*:}\"; done; if command -v sha256sum > /dev/null; then checksum=sha256sum; arguments='--check --status'; else checksum=shasum; arguments='-a 256 --check --status'; fi; resolve_tool VELNOR_HELPER_CHECKSUM \"$checksum\"; printf '%s=%s\n' VELNOR_HELPER_CHECKSUM_ARGUMENTS \"$arguments\" >> \"$GITHUB_ENV\"";

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
            secured.push(resolve_tools(condition.as_deref()));
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

fn resolve_tools(condition: Option<&str>) -> RenderedStep {
    RenderedStep {
        name: "Resolve released helper tools".to_owned(),
        condition: condition.map(str::to_owned),
        run: Some("eval \"$VELNOR_HELPER_INSTALL_TOOLS\"".to_owned()),
        env: vec![(
            "VELNOR_HELPER_INSTALL_TOOLS".to_owned(),
            Value::Text(INSTALL_TOOLS.to_owned()),
        )],
        ..RenderedStep::default()
    }
}

fn install_copy(condition: Option<&str>) -> Vec<RenderedStep> {
    [
        (
            "Create the released helper directory",
            "\"$VELNOR_HELPER_MKDIR\" -m 700 \"$VELNOR_HELPER_DIRECTORY\"",
        ),
        (
            "Copy the released helper",
            "\"$VELNOR_HELPER_CP\" \"$VELNOR_HELPER_SOURCE\" \"$VELNOR_HELPER\"",
        ),
        (
            "Protect the released helper",
            "\"$VELNOR_HELPER_CHMOD\" 500 \"$VELNOR_HELPER\"",
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
            Value::Expr("${{ env.VELNOR_HELPER_CHECKSUM }}".to_owned()),
        ),
        (
            "VELNOR_HELPER_CHECKSUM_ARGUMENTS".to_owned(),
            Value::Expr("${{ env.VELNOR_HELPER_CHECKSUM_ARGUMENTS }}".to_owned()),
        ),
    ]
}
