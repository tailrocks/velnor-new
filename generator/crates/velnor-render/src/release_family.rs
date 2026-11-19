// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::HelperRelease;
use velnor_release::{BinaryPlan, FamilyPlan, RELEASE_APP_SLUG, ReleasePlan, TAG_JOB_BASE_MINUTES};

use crate::release::{
    FROM_A_PASSED_PUSH, GATE_IS_READY, HOSTED_RUNNER, VERIFIED_COMMIT, after_the_gate,
    newest_check, only_while_newest,
};
use crate::release_assets::asset;
use crate::release_binaries::{
    BUILD_TIMEOUT_MINUTES, DIST, PUBLISH_TIMEOUT_MINUTES, SUMS_FILE, artifact, build_steps,
    command, download, download_to, target, upload,
};
use crate::release_steps::{
    TOOLCHAIN, app_token, checkout, expression, join, permissions, shell_word, variables,
};
use crate::rules::{RenderedJob, RenderedStep, Value};
use crate::steps::install_helper;

const TAG_STEP: &str = "family";
const VERSIONS_JOB: &str = "family-versions";
const VERSIONS_ARTIFACT: &str = "family-versions";
const VERSIONS_DIR: &str = "family-versions";
const TAG_OUTPUT: &str = "needs.family.outputs.tag";
const COMMIT_OUTPUT: &str = "needs.family.outputs.commit";
const BUILD_OUTPUT: &str = "needs.family.outputs.build == 'true'";

pub(crate) fn family_jobs(
    plan: &ReleasePlan,
    family: &FamilyPlan,
    helper: &HelperRelease,
) -> Vec<RenderedJob> {
    let mut jobs = vec![versions_job(plan, helper), tag_job(plan, family, helper)];
    jobs.extend(plan.binaries.iter().flat_map(|binary| {
        binary
            .platforms
            .iter()
            .map(|platform| build(binary, *platform))
    }));
    jobs.push(sums(plan));
    jobs.push(publish(plan, helper));
    jobs
}

fn after_the_family(id: &str, needs: Vec<String>) -> RenderedJob {
    let mut all = vec!["gate".to_owned(), "family".to_owned()];
    all.extend(needs);
    RenderedJob {
        id: id.to_owned(),
        needs: all,
        condition: Some(expression(&format!(
            "{GATE_IS_READY} && {FROM_A_PASSED_PUSH} && {BUILD_OUTPUT}"
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

fn versions_job(plan: &ReleasePlan, helper: &HelperRelease) -> RenderedJob {
    let mut steps = vec![
        checkout(VERIFIED_COMMIT, 1),
        install_helper(helper),
        newest_check(),
        command(
            "Install the Rust toolchain".to_owned(),
            format!("rustup toolchain install {TOOLCHAIN} --profile minimal --no-self-update"),
        ),
        command(
            "Make the versions folder".to_owned(),
            format!("mkdir {VERSIONS_DIR}"),
        ),
    ];
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
    only_while_newest(&mut steps);
    RenderedJob {
        permissions: permissions(&[("contents", "read")]),
        timeout_minutes: TAG_JOB_BASE_MINUTES,
        steps,
        environment: None,
        ..after_the_gate(VERSIONS_JOB)
    }
}

fn tag_job(plan: &ReleasePlan, family: &FamilyPlan, helper: &HelperRelease) -> RenderedJob {
    let files = version_files(plan);
    let mut steps = vec![
        install_helper(helper),
        newest_check(),
        download_to(VERSIONS_ARTIFACT.to_owned(), VERSIONS_DIR),
        app_token(&[("contents", "write")]),
    ];
    let packages: Vec<String> = plan
        .binaries
        .iter()
        .map(|binary| format!("--package={}", shell_word(&binary.package)))
        .collect();
    steps.push(RenderedStep {
        id: Some(TAG_STEP.to_owned()),
        name: "Bind the family tag to the verified commit and open its draft release".to_owned(),
        env: variables(&[
            ("GITHUB_TOKEN", "${{ steps.app.outputs.token }}"),
            ("REPOSITORY", "${{ github.repository }}"),
            ("COMMIT", VERIFIED_COMMIT),
        ]),
        run: Some(format!(
            "velnor-ci release family-tag --repo=\"$REPOSITORY\" --verified-commit=\"$COMMIT\" --tag-pattern={} {} {}",
            shell_word(family.tag_pattern.as_str()),
            packages.join(" "),
            files.join(" ")
        )),
        ..RenderedStep::default()
    });
    only_while_newest(&mut steps);
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
        needs: vec!["gate".to_owned(), VERSIONS_JOB.to_owned()],
        ..after_the_gate("family")
    }
}

fn build(binary: &BinaryPlan, platform: velnor_model::Platform) -> RenderedJob {
    let (runner, _) = target(platform, binary.format);
    let source = format!("refs/tags/${{{{ {TAG_OUTPUT} }}}}");
    let check = RenderedStep {
        env: variables(&[
            ("VERIFIED", VERIFIED_COMMIT),
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
        steps: build_steps(binary, platform, &source, check),
        ..after_the_family(
            &format!("build-{}-{}", binary.package, platform.file_suffix()),
            Vec::new(),
        )
    }
}

fn artifacts(plan: &ReleasePlan) -> Vec<(String, String)> {
    plan.binaries
        .iter()
        .flat_map(|binary| {
            binary
                .platforms
                .iter()
                .map(|platform| (artifact(binary, *platform), asset(binary, *platform)))
        })
        .collect()
}

fn sums(plan: &ReleasePlan) -> RenderedJob {
    let mut steps: Vec<RenderedStep> = artifacts(plan)
        .into_iter()
        .map(|(name, _)| download(name))
        .collect();
    let names: Vec<String> = artifacts(plan).into_iter().map(|(_, file)| file).collect();
    let names = names.join(" ");
    for (tool, on_macos) in [("sha256sum", false), ("shasum -a 256", true)] {
        let comparison = if on_macos { "==" } else { "!=" };
        steps.push(RenderedStep {
            working_directory: Some(DIST.to_owned()),
            condition: Some(expression(&format!("runner.os {comparison} 'macOS'"))),
            ..command(
                format!("Write {SUMS_FILE}"),
                format!("{tool} {names} > {SUMS_FILE}"),
            )
        });
    }
    steps.push(upload(
        "family-sums".to_owned(),
        format!("{DIST}/{SUMS_FILE}"),
    ));
    let builds = plan
        .binaries
        .iter()
        .flat_map(|binary| {
            binary
                .platforms
                .iter()
                .map(|platform| format!("build-{}-{}", binary.package, platform.file_suffix()))
        })
        .collect();
    RenderedJob {
        no_permissions: true,
        runs_on: HOSTED_RUNNER.to_owned(),
        timeout_minutes: PUBLISH_TIMEOUT_MINUTES,
        steps,
        ..after_the_family("sums-family", builds)
    }
}

fn publish(plan: &ReleasePlan, helper: &HelperRelease) -> RenderedJob {
    let mut steps = vec![install_helper(helper)];
    steps.extend(artifacts(plan).into_iter().map(|(name, _)| download(name)));
    steps.push(download("family-sums".to_owned()));
    let files: Vec<String> = artifacts(plan).into_iter().map(|(_, file)| file).collect();
    steps.push(RenderedStep {
        name: "Publish the family release".to_owned(),
        env: variables(&[
            ("GITHUB_TOKEN", "${{ github.token }}"),
            ("REPOSITORY", "${{ github.repository }}"),
            ("TAG", &format!("${{{{ {TAG_OUTPUT} }}}}")),
        ]),
        run: Some(format!(
            "velnor-ci release publish --repo=\"$REPOSITORY\" --tag=\"$TAG\" --app={RELEASE_APP_SLUG} --dir={DIST} {}",
            files.join(" ")
        )),
        ..RenderedStep::default()
    });
    RenderedJob {
        permissions: permissions(&[("contents", "write")]),
        runs_on: HOSTED_RUNNER.to_owned(),
        timeout_minutes: PUBLISH_TIMEOUT_MINUTES,
        steps,
        ..after_the_family("publish-family", vec!["sums-family".to_owned()])
    }
}
