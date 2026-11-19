// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{HelperRelease, Job, StepKind};

use crate::rules::{RenderedJob, RenderedStep, SETUP_VELNOR_ACTION, Value};
use crate::yaml::{Yaml, entry, text};

pub(crate) const DEFAULT_SHELL: &str = "bash -p {0}";

pub(crate) const SHELLCHECK_IGNORE: &str = "jactionlint ignore=shellcheck";

pub(crate) const INVOCATION_PREFIX: &str = ". \"$VELNOR_EXEC\" ";

pub(crate) fn defaults(jobs: &[RenderedJob]) -> Option<Yaml> {
    jobs.iter()
        .flat_map(|job| &job.steps)
        .any(|step| {
            step.run
                .as_deref()
                .is_some_and(|run| run.starts_with(INVOCATION_PREFIX))
        })
        .then(|| entry("run", Yaml::Map(vec![entry("shell", text(DEFAULT_SHELL))])))
        .map(|run| Yaml::Map(vec![run]))
}

const LAUNCHER: &str = "directory=$(\"$VELNOR_HELPER_MKTEMP\" -d \"$RUNNER_TEMP/velnor-helper.XXXXXXXXXX\") && copy=$(\"$VELNOR_HELPER_MKTEMP\" \"$directory/velnor-ci.XXXXXXXXXX\") && \"$VELNOR_HELPER_CP\" \"$VELNOR_HELPER\" \"$copy\" && \"$VELNOR_HELPER_CHMOD\" 500 \"$copy\" && { \"$VELNOR_HELPER_ENV\" -i PATH=/usr/bin:/bin \"$VELNOR_HELPER_CHECKSUM\" $VELNOR_HELPER_CHECKSUM_ARGUMENTS <<< \"$VELNOR_HELPER_SHA256  $copy\" || { printf '%s\\n' 'velnor-ci integrity check failed' >&2; exit 1; }; } && exec \"$copy\" \"$@\"";

const INSTALL_TOOLS: &str = "fail() { printf '%s\\n' \"::error title=setup-velnor::$1\"; exit 1; }; resolve_tool() { local path; path=\"$(command -v \"$2\")\" && [[ \"$path\" = /* && -f \"$path\" && -x \"$path\" ]] || fail \"required tool '$2' was not found on PATH\"; printf '%s=%s\\n' \"$1\" \"$path\" >> \"$GITHUB_ENV\"; }; for binding in MKTEMP:mktemp CP:cp CHMOD:chmod MKDIR:mkdir ENV:env; do resolve_tool \"VELNOR_HELPER_${binding%:*}\" \"${binding#*:}\"; done; if command -v sha256sum > /dev/null; then checksum=sha256sum; arguments='--check --status'; else checksum=shasum; arguments='-a 256 --check --status'; fi; resolve_tool VELNOR_HELPER_CHECKSUM \"$checksum\"; printf '%s=%s\\n' VELNOR_HELPER_CHECKSUM_ARGUMENTS \"$arguments\" >> \"$GITHUB_ENV\"";

const INSTALL_LAUNCHER: &str = "directory=\"$RUNNER_TEMP/velnor-trusted-$GITHUB_RUN_ID-$GITHUB_RUN_ATTEMPT-$GITHUB_JOB\" && \"$VELNOR_HELPER_MKDIR\" -m 700 \"$directory\" && \"$VELNOR_HELPER_CP\" \"$RUNNER_TEMP/velnor-ci-bin/velnor-ci\" \"$directory/velnor-ci\" && \"$VELNOR_HELPER_CHMOD\" 500 \"$directory/velnor-ci\" && printf '%s\\n' \"$VELNOR_HELPER_LAUNCHER\" > \"$directory/velnor-ci-exec\" && \"$VELNOR_HELPER_CHMOD\" 500 \"$directory/velnor-ci-exec\" && printf '%s=%s\\n' VELNOR_HELPER \"$directory/velnor-ci\" VELNOR_EXEC \"$directory/velnor-ci-exec\" VELNOR_HELPER_SHA256 \"$VELNOR_HELPER_SHA256\" >> \"$GITHUB_ENV\"";

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
            secured.push(install_launcher(condition.as_deref(), release));
            continue;
        }
        if let Some(arguments) = step
            .run
            .as_ref()
            .and_then(|run| run.strip_prefix("velnor-ci "))
        {
            step.run = Some(format!("{INVOCATION_PREFIX}{arguments}"));
        }
        secured.push(step);
    }
    secured
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

fn install_launcher(condition: Option<&str>, release: &HelperRelease) -> RenderedStep {
    RenderedStep {
        name: "Install the released helper launcher".to_owned(),
        condition: condition.map(str::to_owned),
        run: Some("eval \"$VELNOR_HELPER_INSTALL_LAUNCHER\"".to_owned()),
        env: vec![
            (
                "VELNOR_HELPER_INSTALL_LAUNCHER".to_owned(),
                Value::Text(INSTALL_LAUNCHER.to_owned()),
            ),
            (
                "VELNOR_HELPER_LAUNCHER".to_owned(),
                Value::Text(LAUNCHER.to_owned()),
            ),
            (
                "VELNOR_HELPER_SHA256".to_owned(),
                Value::Expr(digest_choice(release)),
            ),
        ],
        ..RenderedStep::default()
    }
}

fn digest_choice(release: &HelperRelease) -> String {
    let choices: Vec<String> = release
        .digests()
        .iter()
        .map(|(platform, digest)| {
            let (os, arch) = platform.runner_key().split_once('-').unwrap_or(("", ""));
            format!("runner.os == '{os}' && runner.arch == '{arch}' && '{digest}'")
        })
        .collect();
    format!("${{{{ {} || '' }}}}", choices.join(" || "))
}
