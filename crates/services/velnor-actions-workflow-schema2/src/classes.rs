//! Qualification classes that each dispatch as their own `inputs.mode`.
//! They are not part of `features`. Outputs and cache keep one scale-set
//! job; the hosted job `needs` it. `ports` runs two scale-set jobs with no
//! `needs` so both can hold the same logical port.

use super::RunnerSpec;
use super::features::{
    checkout_step, gated, lane_base_with_container, local_action_step, redis_service, run_step,
};
use velnor_actions_workflow_tree::job_entries::finish;
use velnor_actions_workflow_tree::yaml::Yaml;

mod more;
mod steps;
pub(crate) mod topology;

const GITHUB_SCRIPT: &str = "actions/github-script@3a2844b7e9c422d3c10d287c895573f7108da1b3";
const CACHE_USES: &str = "actions/cache@55cc8345863c7cc4c66a329aec7e433d2d1c52a9";
const COMPOSITE_ACTION: &str = "./qualification/actions/composite";
const DOCKER_ACTION: &str = "./qualification/actions/docker";
const POST_FAIL_ACTION: &str = "./qualification/actions/post-fail";
const CACHE_KEY: &str = "g4-cache-${{ github.run_id }}";
const WRITE_OUTPUTS: &str = "mkdir -p \"$RUNNER_TEMP/g4-bin\" && printf '%s\\n' '#!/bin/sh' 'echo path-ok' > \"$RUNNER_TEMP/g4-bin/g4-path-ok\" && chmod +x \"$RUNNER_TEMP/g4-bin/g4-path-ok\" && echo proof=outputs-ok >> \"$GITHUB_OUTPUT\" && echo G4_ENV=outputs-ok >> \"$GITHUB_ENV\" && echo \"$RUNNER_TEMP/g4-bin\" >> \"$GITHUB_PATH\"";
const CHECK_ENV_PATH: &str = "test \"$G4_ENV\" = outputs-ok && g4-path-ok | grep -qx path-ok";
const MASK_CANARY: &str = "echo \"::add-mask::g4-mask-canary\" && echo g4-mask-canary";
const CHECK_OIDC: &str = "test -n \"$ACTIONS_ID_TOKEN_REQUEST_URL\"";
const REDIS_DNS: &str = "i=0; while [ \"$i\" -lt 20 ]; do nc -z -w 1 redis 6379 && exit 0; i=$((i+1)); sleep 1; done; exit 1";

struct Lane<'a> {
    mode: &'a str,
    title: &'a str,
    suffix: &'a str,
    kind: &'a str,
    runner: &'a RunnerSpec,
    needs: &'a [&'a str],
    steps: Vec<Yaml>,
}

#[derive(Clone, Default)]
struct Extras {
    permissions: Option<Yaml>,
    container: Option<Yaml>,
    services: Option<Yaml>,
    outputs: Option<Yaml>,
}

/// One mode per class. `features` does not select these jobs.
pub(crate) fn class_jobs(hosted: &RunnerSpec, scale: &RunnerSpec) -> Vec<(String, Yaml)> {
    let mut jobs = action_classes(hosted, scale);
    jobs.extend(state_classes(hosted, scale));
    jobs.extend(more::jobs(hosted, scale));
    jobs
}

fn action_classes(hosted: &RunnerSpec, scale: &RunnerSpec) -> Vec<(String, Yaml)> {
    let mut jobs = Vec::new();
    jobs.extend(both(
        "composite",
        "Composite",
        hosted,
        scale,
        composite_steps(),
        Extras::default(),
    ));
    jobs.extend(both(
        "js-pin",
        "Pinned JavaScript",
        hosted,
        scale,
        js_pin_steps(),
        Extras::default(),
    ));
    jobs.extend(both(
        "docker-action",
        "Docker action",
        hosted,
        scale,
        docker_steps(),
        Extras::default(),
    ));
    jobs.extend(both(
        "container",
        "Container",
        hosted,
        scale,
        container_steps(),
        container_extras(),
    ));
    jobs.extend(output_jobs(hosted, scale));
    jobs
}

fn state_classes(hosted: &RunnerSpec, scale: &RunnerSpec) -> Vec<(String, Yaml)> {
    let mut jobs = Vec::new();
    jobs.extend(both(
        "mask",
        "Mask",
        hosted,
        scale,
        mask_steps(),
        Extras::default(),
    ));
    jobs.extend(cache_jobs(hosted, scale));
    jobs.extend(both(
        "oidc",
        "OIDC",
        hosted,
        scale,
        oidc_steps(),
        oidc_extras(),
    ));
    jobs.extend(both(
        "post-fail",
        "Post failure",
        hosted,
        scale,
        post_fail_steps(),
        Extras::default(),
    ));
    jobs.extend(both(
        "cancel",
        "Cancel",
        hosted,
        scale,
        cancel_steps(),
        Extras::default(),
    ));
    jobs
}

fn both(
    mode: &str,
    title: &str,
    hosted: &RunnerSpec,
    scale: &RunnerSpec,
    steps: Vec<Yaml>,
    extras: Extras,
) -> Vec<(String, Yaml)> {
    vec![
        emit(
            lane(
                mode,
                title,
                "hosted",
                "GitHub hosted",
                hosted,
                &[],
                steps.clone(),
            ),
            extras.clone(),
        ),
        emit(
            lane(
                mode,
                title,
                "scale-set",
                "Velnor Scale Set",
                scale,
                &[],
                steps,
            ),
            extras,
        ),
    ]
}

fn output_jobs(hosted: &RunnerSpec, scale: &RunnerSpec) -> Vec<(String, Yaml)> {
    vec![
        emit(
            lane(
                "outputs",
                "Outputs",
                "scale-set",
                "Velnor Scale Set",
                scale,
                &[],
                vec![
                    steps::run_id("Write output env and path", "emit", WRITE_OUTPUTS),
                    run_step("Check env and path", CHECK_ENV_PATH),
                ],
            ),
            Extras {
                outputs: Some(steps::mapping(&[(
                    "proof",
                    "${{ steps.emit.outputs.proof }}",
                )])),
                ..Extras::default()
            },
        ),
        emit(
            lane(
                "outputs",
                "Outputs",
                "hosted",
                "GitHub hosted",
                hosted,
                &["outputs-scale-set"],
                vec![steps::run_env(
                    "Check job output",
                    &[("PROOF", "${{ needs.outputs-scale-set.outputs.proof }}")],
                    "test \"$PROOF\" = outputs-ok",
                )],
            ),
            Extras::default(),
        ),
    ]
}

fn cache_jobs(hosted: &RunnerSpec, scale: &RunnerSpec) -> Vec<(String, Yaml)> {
    let perms = cache_permissions();
    vec![
        emit(
            lane(
                "cache",
                "Cache",
                "scale-set",
                "Velnor Scale Set",
                scale,
                &[],
                vec![
                    run_step("Write cache file", "echo cache-ok > g4-cache.txt"),
                    cache_step("Save cache", false),
                ],
            ),
            Extras {
                permissions: Some(perms.clone()),
                ..Extras::default()
            },
        ),
        emit(
            lane(
                "cache",
                "Cache",
                "hosted",
                "GitHub hosted",
                hosted,
                &["cache-scale-set"],
                vec![
                    cache_step("Restore cache", true),
                    run_step("Check restore", "grep -qx cache-ok g4-cache.txt"),
                ],
            ),
            Extras {
                permissions: Some(perms),
                ..Extras::default()
            },
        ),
    ]
}

fn lane<'a>(
    mode: &'a str,
    title: &'a str,
    suffix: &'a str,
    kind: &'a str,
    runner: &'a RunnerSpec,
    needs: &'a [&'a str],
    steps: Vec<Yaml>,
) -> Lane<'a> {
    Lane {
        mode,
        title,
        suffix,
        kind,
        runner,
        needs,
        steps,
    }
}

fn emit(job: Lane<'_>, extras: Extras) -> (String, Yaml) {
    let mut fields =
        lane_base_with_container(&job.heading(), job.runner, 20, extras.container.is_some());
    push_needs(&mut fields, job.needs);
    push_opt(&mut fields, "permissions", extras.permissions);
    push_opt(&mut fields, "container", extras.container);
    push_opt(&mut fields, "services", extras.services);
    push_opt(&mut fields, "outputs", extras.outputs);
    let when = format!("inputs.mode == '{}'", job.mode);
    gated(finish(&job.id(), fields, job.steps), &when)
}

fn push_needs(fields: &mut Vec<(String, Yaml)>, needs: &[&str]) {
    if needs.is_empty() {
        return;
    }
    fields.push((
        "needs".to_owned(),
        Yaml::Seq(needs.iter().copied().map(Yaml::str).collect()),
    ));
}

fn push_opt(fields: &mut Vec<(String, Yaml)>, key: &str, value: Option<Yaml>) {
    if let Some(value) = value {
        fields.push((key.to_owned(), value));
    }
}

impl Lane<'_> {
    fn id(&self) -> String {
        format!("{}-{}", self.mode, self.suffix)
    }

    fn heading(&self) -> String {
        format!("{} / {}", self.title, self.kind)
    }
}

fn container_extras() -> Extras {
    Extras {
        container: Some(Yaml::str("alpine:3.22")),
        services: Some(redis_service()),
        ..Extras::default()
    }
}

fn oidc_extras() -> Extras {
    Extras {
        permissions: Some(steps::mapping(&[
            ("id-token", "write"),
            ("contents", "read"),
        ])),
        ..Extras::default()
    }
}

fn cache_permissions() -> Yaml {
    steps::mapping(&[("contents", "read"), ("actions", "write")])
}

fn composite_steps() -> Vec<Yaml> {
    vec![
        checkout_step(),
        local_action_step("Local composite", COMPOSITE_ACTION),
    ]
}

fn js_pin_steps() -> Vec<Yaml> {
    vec![steps::uses_with(
        "Pinned script",
        GITHUB_SCRIPT,
        &[("script", "console.log('js-pin-ok')")],
    )]
}

fn docker_steps() -> Vec<Yaml> {
    vec![
        checkout_step(),
        local_action_step("Local docker", DOCKER_ACTION),
    ]
}

fn container_steps() -> Vec<Yaml> {
    // Alpine has no bash. The probe must open the service DNS name.
    vec![steps::shell_step("Service DNS", "sh", REDIS_DNS)]
}

fn mask_steps() -> Vec<Yaml> {
    vec![run_step("Mask canary", MASK_CANARY)]
}

fn oidc_steps() -> Vec<Yaml> {
    // Presence only. Do not print the request URL or a token.
    vec![run_step("Require OIDC request URL", CHECK_OIDC)]
}

fn post_fail_steps() -> Vec<Yaml> {
    // No `if: always()`: main.js must fail the job. The action post still runs.
    vec![
        checkout_step(),
        local_action_step("Main fails post runs", POST_FAIL_ACTION),
    ]
}

fn cancel_steps() -> Vec<Yaml> {
    vec![run_step("Sleep until cancelled", "sleep 180")]
}

fn cache_step(name: &str, fail_on_miss: bool) -> Yaml {
    let mut with = vec![("path", "g4-cache.txt"), ("key", CACHE_KEY)];
    if fail_on_miss {
        with.push(("fail-on-cache-miss", "true"));
    }
    steps::uses_with(name, CACHE_USES, &with)
}
