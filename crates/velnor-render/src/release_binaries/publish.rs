// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::HelperRelease;
use velnor_release::{BinaryPlan, RELEASE_APP_SLUG, ReleasePlan};

use super::{
    DIST, PUBLISH_TIMEOUT_MINUTES, SUMS_FILE, artifact, behind_the_gate, builds, command, download,
    tag_commit, upload,
};
use crate::release::HOSTED_RUNNER;
use crate::release_assets::{COMMIT_FILE, assets, bind_version, quoted, read_version};
use crate::release_attest::attest;
use crate::release_binary_sums::{steps as binary_sums_steps, upload_paths};
use crate::release_checks::publish_needs;
use crate::release_context::package_tag;
use crate::release_steps::{expression, permissions, variables};
use crate::rules::{RenderedJob, RenderedStep};
use crate::steps::install_helper;

pub(super) fn sums(binary: &BinaryPlan) -> RenderedJob {
    let mut steps: Vec<RenderedStep> = binary
        .platforms
        .iter()
        .map(|platform| download(artifact(binary, *platform)))
        .collect();
    steps.extend(read_version(binary, &package_tag(binary)));
    if binary.commit_file {
        steps.push(RenderedStep {
            working_directory: Some(DIST.to_owned()),
            env: variables(&[("COMMIT", &tag_commit(binary))]),
            ..command(
                "Record the commit of the release".to_owned(),
                format!("echo \"$COMMIT\" > {COMMIT_FILE}"),
            )
        });
    }
    steps.extend(binary_sums_steps(binary));
    let names = quoted(binary, &assets(binary)).join(" ");
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
    if binary.attest {
        steps.push(attest(binary));
    }
    let sums_files = upload_paths(
        binary,
        if binary.commit_file {
            &[COMMIT_FILE]
        } else {
            &[]
        },
    );
    steps.push(upload(format!("{}-sums", binary.package), sums_files));
    bind_version(&mut steps);
    RenderedJob {
        no_permissions: !binary.attest,
        permissions: if binary.attest {
            permissions(&[("attestations", "write"), ("id-token", "write")])
        } else {
            Vec::new()
        },
        runs_on: HOSTED_RUNNER.to_owned(),
        timeout_minutes: PUBLISH_TIMEOUT_MINUTES,
        steps,
        ..behind_the_gate(format!("sums-{}", binary.package), binary, builds(binary))
    }
}

pub(super) fn publish(
    plan: &ReleasePlan,
    binary: &BinaryPlan,
    helper: &HelperRelease,
) -> RenderedJob {
    let mut steps = vec![install_helper(helper)];
    steps.extend(
        binary
            .platforms
            .iter()
            .map(|platform| download(artifact(binary, *platform))),
    );
    steps.push(download(format!("{}-sums", binary.package)));
    steps.extend(read_version(binary, &package_tag(binary)));
    steps.push(RenderedStep {
        name: format!("Publish the release of {}", binary.package),
        env: variables(&[
            ("GITHUB_TOKEN", "${{ github.token }}"),
            ("REPOSITORY", "${{ github.repository }}"),
            ("TAG", &package_tag(binary)),
        ]),
        run: Some(format!(
            "velnor-ci release publish --repo=\"$REPOSITORY\" --tag=\"$TAG\" --app={RELEASE_APP_SLUG} --dir={DIST} {}",
            quoted(binary, &assets(binary)).join(" ")
        )),
        ..RenderedStep::default()
    });
    bind_version(&mut steps);
    RenderedJob {
        permissions: permissions(&[("contents", "write")]),
        runs_on: HOSTED_RUNNER.to_owned(),
        timeout_minutes: PUBLISH_TIMEOUT_MINUTES,
        steps,
        ..behind_the_gate(
            format!("publish-{}", binary.package),
            binary,
            [format!(
                "{}-{}",
                if binary.attest { "verify" } else { "sums" },
                binary.package
            )]
            .into_iter()
            .chain(publish_needs(plan, binary))
            .collect(),
        )
    }
}
