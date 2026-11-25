// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt::Write;

use velnor_model::{ImageConfig, ImageOpenssl, ImagePrBuild, Step, StepKind};

use crate::base_steps::sources_locks;
use crate::cache::cache_step;
use crate::cargo::offline_env;
use crate::helper_steps::{mbx_private_step, mbx_seed_step, mbx_store_step, mirror_step};
use crate::image::{
    Events, IMAGE_BUILDKIT, IMAGE_SYSROOT_LOCK, IMAGE_TARGET, ImageBuild, ImageMbx, ImageWorkflow,
};
use crate::image_changes::{CHANGES_JOB, selected, selective};
use crate::image_group::Compile;
use crate::image_publish::{TIP_STEP, host_guard, private_source_guard, tip};
use crate::image_stage::{member_steps, stage_step, sums_output};
use crate::install_steps::{install_helper, install_tools};
use crate::mbx_out_dirs::mbx_out_dirs_step;
use crate::pinned_actions::DOCKER_SETUP_BUILDX;
use crate::private_steps::{CHECK_STEP, queued_phase, release_mint};
use crate::release_binaries::command;
use crate::release_steps::{expression, join, permissions, shell_word};
use crate::rules::{RenderedConcurrency, RenderedJob, RenderedStep, Value};
use crate::steps::{CACHE_RESTORE, CHECKOUT, RUN};

pub(crate) const ARTIFACT_RETENTION_DAYS: u64 = 7;
pub(crate) const LIVE_OUTPUT: &str = "live";
const RECHECK_STEP: &str = "recheck";

pub(crate) fn job_id(image: &ImageConfig, part: &str) -> String {
    format!("image-{}-{part}", image.name)
}

pub(crate) fn artifact(image: &ImageConfig) -> String {
    format!("image-{}", image.name)
}

pub(crate) fn staged(image: &ImageConfig) -> String {
    if image.context == "." {
        image.stage.clone()
    } else {
        join(&image.context, &image.stage)
    }
}

pub(crate) fn labels() -> Value {
    Value::Expr(
        "org.opencontainers.image.source=${{ github.server_url }}/${{ github.repository }}\norg.opencontainers.image.revision=${{ github.sha }}"
            .to_owned(),
    )
}

pub(crate) fn setup_buildx() -> RenderedStep {
    RenderedStep {
        with: vec![(
            "driver-opts".to_owned(),
            Value::Text(format!("image={IMAGE_BUILDKIT}")),
        )],
        ..DOCKER_SETUP_BUILDX.step("Set up Docker Buildx".to_owned())
    }
}

pub(crate) fn checkout() -> RenderedStep {
    RenderedStep {
        name: "Check out".to_owned(),
        uses: Some(CHECKOUT.to_owned()),
        with: vec![
            ("fetch-depth".to_owned(), Value::Int(1)),
            ("persist-credentials".to_owned(), Value::Bool(false)),
        ],
        ..RenderedStep::default()
    }
}

fn sources_restores(steps: &[Step]) -> Vec<RenderedStep> {
    steps
        .iter()
        .filter_map(|step| match &step.kind {
            StepKind::SourcesLocks(locks) => Some(sources_locks(locks)),
            StepKind::CacheRestore(key) => Some(cache_step(CACHE_RESTORE, "Restore", key, true)),
            _ => None,
        })
        .collect()
}

fn and_condition(existing: Option<String>, extra: Option<&str>) -> Option<String> {
    let inner = |text: &str| {
        text.trim_start_matches("${{ ")
            .trim_end_matches(" }}")
            .to_owned()
    };
    match (existing, extra) {
        (Some(condition), Some(extra)) => Some(format!(
            "${{{{ ({}) && ({}) }}}}",
            inner(&condition),
            inner(extra)
        )),
        (condition, None) => condition,
        (None, Some(extra)) => Some(extra.to_owned()),
    }
}

pub(crate) fn sha_scope() -> &'static str {
    "${{ github.event_name == 'push' && github.sha || github.run_id }}"
}

pub(crate) fn stands_down() -> RenderedStep {
    tip(
        TIP_STEP,
        Some(expression(
            "github.event_name == 'push' && github.run_attempt == 1",
        )),
    )
}

fn rechecks() -> RenderedStep {
    only_live(tip(
        RECHECK_STEP,
        Some(expression(
            "github.event_name == 'push' && github.run_attempt == 1",
        )),
    ))
}

pub(crate) fn only_live(step: RenderedStep) -> RenderedStep {
    live_after(step, &[TIP_STEP])
}

fn live_after(mut step: RenderedStep, checks: &[&str]) -> RenderedStep {
    let ready = checks
        .iter()
        .map(|id| format!("steps.{id}.outputs.ready == 'true'"))
        .collect::<Vec<_>>()
        .join(" && ");
    let live = format!("github.event_name != 'push' || github.run_attempt != 1 || ({ready})");
    let inner = step.condition.as_deref().map(|text| {
        text.strip_prefix("${{ ")
            .and_then(|rest| rest.strip_suffix(" }}"))
            .unwrap_or(text)
            .to_owned()
    });
    step.condition = Some(expression(&match inner {
        Some(inner) => format!("({inner}) && ({live})"),
        None => live,
    }));
    step
}

pub(crate) fn build_job(
    workflow: &ImageWorkflow<'_>,
    group: &Compile<'_>,
    events: &Events<'_>,
) -> RenderedJob {
    let selective = selective(workflow);
    let reaching: Vec<&ImageBuild<'_>> = group
        .members
        .iter()
        .copied()
        .filter(|build| build.image.pr_build != ImagePrBuild::Off)
        .collect();
    let reached = if reaching.is_empty() {
        events.branch_push()
    } else if selective {
        let selected: Vec<String> = reaching
            .iter()
            .filter_map(|build| selected(build.image))
            .collect();
        format!(
            "({}) || (({}) && ({}))",
            events.branch_push(),
            events.checked(),
            selected.join(" || ")
        )
    } else {
        format!("({}) || ({})", events.branch_push(), events.checked())
    };
    let id = group.id();
    RenderedJob {
        concurrency: Some(RenderedConcurrency {
            group: format!("{id}-{}", sha_scope()),
            cancel_in_progress: true,
        }),
        outputs: [(
            LIVE_OUTPUT.to_owned(),
            format!(
                "${{{{ github.run_attempt != 1 || (steps.{TIP_STEP}.outputs.ready == 'true' && steps.{RECHECK_STEP}.outputs.ready == 'true') }}}}"
            ),
        )]
        .into_iter()
        .chain(group.members.iter().map(|build| {
            (
                sums_output(build.image),
                format!(
                    "${{{{ steps.{}.outputs.sums }}}}",
                    stage_step(build.image)
                ),
            )
        }))
        .collect(),
        id,
        condition: Some(expression(&reached)),
        needs: if selective && !reaching.is_empty() {
            vec![CHANGES_JOB.to_owned()]
        } else {
            Vec::new()
        },
        permissions: permissions(&build_scopes(workflow)),
        runs_on: workflow.runner.to_string(),
        timeout_minutes: group
            .members
            .iter()
            .map(|build| build.image.timeout_minutes)
            .max()
            .unwrap_or_default(),
        steps: build_steps(workflow, group),
        ..RenderedJob::default()
    }
}

fn build_scopes(workflow: &ImageWorkflow<'_>) -> Vec<(&'static str, &'static str)> {
    let mut scopes = vec![("contents", "read")];
    if workflow.private.is_some() {
        scopes.push(("pull-requests", "read"));
    }
    scopes
}

fn build_steps(workflow: &ImageWorkflow<'_>, group: &Compile<'_>) -> Vec<RenderedStep> {
    let first = group.members[0];
    let mut steps = vec![checkout(), install_helper(workflow.helper), stands_down()];
    steps.extend(host_guard());
    if workflow.private.is_some() {
        steps.extend(private_source_guard());
    }
    match workflow.mbx {
        ImageMbx::Off => {}
        ImageMbx::Private => steps.extend([mbx_private_step(), mbx_out_dirs_step()]),
        ImageMbx::Shared => steps.extend([mbx_store_step(), mbx_seed_step()]),
    }
    steps.extend(workflow.mirror.map(mirror_step));
    let restores = sources_restores(&workflow.sources);
    let hit_id = restores
        .iter()
        .find(|step| step.uses.is_some())
        .and_then(|step| step.id.clone());
    steps.extend(restores);
    if let Some(sources) = workflow.private {
        let unless_hit =
            hit_id.map(|id| expression(&format!("steps.{id}.outputs.cache-hit != 'true'")));
        steps.extend(
            queued_phase(
                sources,
                &join(first.root, "Cargo.toml"),
                release_mint(workflow.helper),
            )
            .into_iter()
            .map(|step| RenderedStep {
                condition: if step.id.as_deref() == Some(CHECK_STEP) {
                    step.condition
                } else {
                    and_condition(step.condition, unless_hit.as_deref())
                },
                ..step
            }),
        );
    }
    steps.push(install_tools(None));
    steps.extend(workflow.mirror.map(mirror_step));
    steps.extend([command(
        format!("Add the {IMAGE_TARGET} target"),
        format!("rustup target add {IMAGE_TARGET}"),
    )]);
    if first.image.openssl == ImageOpenssl::Sysroot {
        steps.push(command(
            "Unpack the pinned OpenSSL sysroot".to_owned(),
            format!(
                "velnor-ci image sysroot --lock {IMAGE_SYSROOT_LOCK} --out \"$RUNNER_TEMP/image-sysroot\""
            ),
        ));
    }
    let before_compile = steps.len();
    steps.push(compile(workflow, group));
    for build in &group.members {
        steps.extend(member_steps(workflow, build));
    }
    let after = steps.split_off(before_compile);
    let between = steps.split_off(3);
    steps.extend(between.into_iter().map(only_live));
    steps.push(rechecks());
    steps.extend(
        after
            .into_iter()
            .map(|step| live_after(step, &[TIP_STEP, RECHECK_STEP])),
    );
    steps
}

fn compile(workflow: &ImageWorkflow<'_>, group: &Compile<'_>) -> RenderedStep {
    let build = group.members[0];
    let image = build.image;
    let offline = workflow.private.is_some();
    let profile = if image.profile == "release" {
        "--release".to_owned()
    } else {
        format!("--profile {}", image.profile)
    };
    let mut run = format!(
        "{RUN} cargo zigbuild {profile} --locked{} --target-dir target --target {IMAGE_TARGET}.{}",
        if offline { " --offline" } else { "" },
        image.glibc
    );
    for package in group.packages() {
        let _ = write!(run, " --package {}", shell_word(package));
    }
    for bin in group.bins() {
        let _ = write!(run, " --bin {}", shell_word(bin));
    }
    if !image.default_features {
        run.push_str(" --no-default-features");
    }
    if !image.features.is_empty() {
        let _ = write!(run, " --features {}", shell_word(&image.features.join(",")));
    }
    let mut env = vec![
        ("CARGO_INCREMENTAL".to_owned(), Value::Text("0".to_owned())),
        (
            "CARGO_TERM_COLOR".to_owned(),
            Value::Text("never".to_owned()),
        ),
        ("SOURCE_DATE_EPOCH".to_owned(), Value::Text("0".to_owned())),
    ];
    env.extend(offline.then(offline_env));
    let name = group
        .members
        .iter()
        .map(|member| member.image.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    RenderedStep {
        working_directory: (build.root != ".").then(|| build.root.to_owned()),
        env,
        ..command(format!("Compile {name} with zigbuild"), run)
    }
}
