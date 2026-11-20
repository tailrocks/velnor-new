// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{HelperRelease, PrivateSources};
use velnor_release::{BinaryPlan, FamilyPlan, ReleasePlan, TAG_JOB_BASE_MINUTES};

use crate::install_steps::install_tools;
use crate::release::{GATE_IS_READY, after_the_gate_with, newest_check, only_while_newest};
use crate::release_binaries::{
    BUILD_TIMEOUT_MINUTES, build_steps, command, download_to, target, upload,
};
use crate::release_checks::tag_needs;
use crate::release_context::{BuildContext, Private};
use crate::release_family_publish::{publish, sums, verify};
use crate::release_source::{Source, source_of};
use crate::release_steps::{
    TOOLCHAIN, app_token, checkout, expression, join, permissions, shell_word, variables,
};
use crate::rules::{RenderedJob, RenderedStep, Value};
use crate::steps::install_helper;

const TAG_STEP: &str = "family";
const VERSIONS_JOB: &str = "family-versions";
const VERSIONS_ARTIFACT: &str = "family-versions";
const VERSIONS_DIR: &str = "family-versions";
pub(crate) const TAG_OUTPUT: &str = "needs.family.outputs.tag";
pub(crate) const COMMIT_OUTPUT: &str = "needs.family.outputs.commit";
pub(crate) const BUILD_OUTPUT: &str = "needs.family.outputs.build == 'true'";

pub(crate) fn family_jobs(
    plan: &ReleasePlan,
    family: &FamilyPlan,
    helper: &HelperRelease,
    private: Option<&PrivateSources>,
) -> Vec<RenderedJob> {
    let source = source_of(plan);
    let mut jobs = vec![
        versions_job(plan, helper, &source),
        tag_job(plan, family, helper, &source),
    ];
    jobs.extend(plan.binaries.iter().flat_map(|binary| {
        binary
            .platforms
            .iter()
            .map(|platform| build(binary, *platform, helper, private, &source, plan.mbx))
    }));
    jobs.push(sums(plan, family, &source));
    if family.attest {
        jobs.push(verify(plan, family, &source));
    }
    jobs.push(publish(plan, family, helper, &source));
    jobs
}

pub(crate) fn after_the_family(id: &str, needs: Vec<String>, source: &Source) -> RenderedJob {
    let mut all = vec!["gate".to_owned(), "family".to_owned()];
    all.extend(needs);
    RenderedJob {
        id: id.to_owned(),
        needs: all,
        condition: Some(expression(&format!(
            "{GATE_IS_READY} && {} && {BUILD_OUTPUT}",
            source.passed
        ))),
        ..RenderedJob::default()
    }
}

fn family_roots(plan: &ReleasePlan) -> Vec<&str> {
    plan.binaries
        .iter()
        .map(|binary| binary.root.as_str())
        .fold(Vec::new(), |mut seen, root| {
            if !seen.contains(&root) {
                seen.push(root);
            }
            seen
        })
}

fn version_files(plan: &ReleasePlan) -> Vec<String> {
    (0..family_roots(plan).len())
        .map(|index| format!("{VERSIONS_DIR}/family-{index}.json"))
        .collect()
}

fn versions_job(plan: &ReleasePlan, helper: &HelperRelease, source: &Source) -> RenderedJob {
    let mut steps = vec![checkout(source.verified, 1), install_helper(helper)];
    if !source.tag_push {
        steps.push(newest_check());
    }
    steps.extend([
        command(
            "Install the Rust toolchain".to_owned(),
            format!("rustup toolchain install {TOOLCHAIN} --profile minimal --no-self-update"),
        ),
        command(
            "Make the versions folder".to_owned(),
            format!("mkdir {VERSIONS_DIR}"),
        ),
    ]);
    // cargo metadata runs under the mise wrapper when MBX is declared.
    if plan.mbx {
        steps.push(install_tools(None));
    }
    steps.extend(family_roots(plan).into_iter().zip(version_files(plan)).map(
        |(root, file)| RenderedStep {
            env: vec![(
                "RUSTUP_TOOLCHAIN".to_owned(),
                Value::Text(TOOLCHAIN.to_owned()),
            )],
            ..command(
                format!("Read the versions of the binaries of {root}"),
                format!(
                    "cargo metadata --no-deps --locked --format-version 1 --manifest-path {} > {file}",
                    shell_word(&join(root, "Cargo.toml"))
                ),
            )
        },
    ));
    steps.push(upload(
        VERSIONS_ARTIFACT.to_owned(),
        VERSIONS_DIR.to_owned(),
    ));
    if !source.tag_push {
        only_while_newest(&mut steps);
    }
    RenderedJob {
        permissions: permissions(&[("contents", "read")]),
        timeout_minutes: TAG_JOB_BASE_MINUTES,
        steps,
        environment: None,
        ..after_the_gate_with(VERSIONS_JOB, source.passed)
    }
}

fn tag_job(
    plan: &ReleasePlan,
    family: &FamilyPlan,
    helper: &HelperRelease,
    source: &Source,
) -> RenderedJob {
    let files = version_files(plan);
    let mut steps = vec![install_helper(helper)];
    if !source.tag_push {
        steps.push(newest_check());
    }
    steps.extend([
        download_to(VERSIONS_ARTIFACT.to_owned(), VERSIONS_DIR),
        app_token(&[("contents", "write")]),
    ]);
    let mut env = variables(&[
        ("GITHUB_TOKEN", "${{ steps.app.outputs.token }}"),
        ("REPOSITORY", "${{ github.repository }}"),
        ("COMMIT", source.verified),
    ]);
    let mut pushed = "";
    if source.tag_push {
        env.extend(variables(&[("PUSHED_TAG", "${{ github.ref_name }}")]));
        pushed = " --pushed-tag=\"$PUSHED_TAG\"";
    }
    let packages: Vec<String> = plan
        .binaries
        .iter()
        .map(|binary| format!("--package={}", shell_word(&binary.package)))
        .collect();
    steps.push(RenderedStep {
        id: Some(TAG_STEP.to_owned()),
        name: "Bind the family tag to the verified commit and open its draft release".to_owned(),
        env,
        run: Some(format!(
            "velnor-ci release family-tag --repo=\"$REPOSITORY\" --verified-commit=\"$COMMIT\"{pushed} --tag-pattern={} {} {}",
            shell_word(family.tag_pattern.as_str()),
            packages.join(" "),
            files.join(" ")
        )),
        ..RenderedStep::default()
    });
    if !source.tag_push {
        only_while_newest(&mut steps);
    }
    RenderedJob {
        permissions: permissions(&[("contents", "read")]),
        outputs: ["tag", "commit", "build"]
            .into_iter()
            .map(|name| {
                (
                    name.to_owned(),
                    format!("${{{{ steps.{TAG_STEP}.outputs.{name} }}}}"),
                )
            })
            .collect(),
        timeout_minutes: TAG_JOB_BASE_MINUTES,
        steps,
        needs: ["gate".to_owned(), VERSIONS_JOB.to_owned()]
            .into_iter()
            .chain(tag_needs(plan))
            .collect(),
        environment_note: source.tag_push.then(|| {
            let (before, after) = family.tag_affixes();
            format!(
                "A tag push deploys from the tag ref: the release environment must allow {} in its deployment branch and tag policy, or this job never starts.",
                format_args!("{before}*{after}")
            )
        }),
        ..after_the_gate_with("family", source.passed)
    }
}

fn build(
    binary: &BinaryPlan,
    platform: velnor_model::Platform,
    helper: &HelperRelease,
    private: Option<&PrivateSources>,
    source: &Source,
    mbx: bool,
) -> RenderedJob {
    let (runner, _) = target(platform, binary);
    let reference = format!("refs/tags/${{{{ {TAG_OUTPUT} }}}}");
    let tag = format!("${{{{ {TAG_OUTPUT} }}}}");
    let check = RenderedStep {
        env: variables(&[
            ("VERIFIED", source.verified),
            ("FAMILY", &format!("${{{{ {COMMIT_OUTPUT} }}}}")),
            ("CHECKED_OUT", "${{ steps.source.outputs.commit }}"),
        ]),
        ..command(
            "Check that the family tag is on the verified commit itself".to_owned(),
            "test -n \"$VERIFIED\" -a \"$CHECKED_OUT\" = \"$VERIFIED\" -a \"$FAMILY\" = \"$VERIFIED\""
                .to_owned(),
        )
    };
    RenderedJob {
        permissions: permissions(&[("contents", "read")]),
        runs_on: runner.to_owned(),
        timeout_minutes: BUILD_TIMEOUT_MINUTES,
        steps: build_steps(
            binary,
            platform,
            BuildContext {
                source: &reference,
                same_commit: check,
                tag: &tag,
                private: private.map(|sources| Private { helper, sources }),
                mbx,
            },
        ),
        ..after_the_family(
            &format!("build-{}-{}", binary.package, platform.file_suffix()),
            Vec::new(),
            source,
        )
    }
}
