// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{HelperRelease, Platform};
use velnor_release::{BinaryPlan, RELEASE_APP_SLUG, ReleasePlan};

use crate::release::{FROM_A_PASSED_PUSH, GATE_IS_READY, VERIFIED_COMMIT};
use crate::release_steps::{checkout, expression, install_tool, join, permissions, variables};
use crate::rules::{RenderedJob, RenderedStep, Value};
use crate::steps::install_helper;

const UPLOAD_ARTIFACT: &str = "actions/upload-artifact@cf430e030ddbb5b0abf93d22962f4752f3646cd9";
const UPLOAD_VERSION: &str = "v7.0.2";
const DOWNLOAD_ARTIFACT: &str =
    "actions/download-artifact@9000827ccba6bdab643e8b6fd33ac0654aef8333";
const DOWNLOAD_VERSION: &str = "v8.0.2";
const BUILD_TIMEOUT_MINUTES: u16 = 60;
const PUBLISH_TIMEOUT_MINUTES: u16 = 10;
const DIST: &str = "dist";
const SUMS_FILE: &str = "SHA256SUMS";

pub(crate) fn binary_jobs(
    plan: &ReleasePlan,
    runner: &str,
    helper: &HelperRelease,
) -> Vec<RenderedJob> {
    plan.binaries
        .iter()
        .flat_map(|binary| {
            let mut jobs: Vec<RenderedJob> = binary
                .platforms
                .iter()
                .map(|platform| build(binary, *platform))
                .collect();
            jobs.push(sums(binary, runner));
            jobs.push(publish(binary, runner, helper));
            jobs
        })
        .collect()
}

fn target(platform: Platform) -> (&'static str, &'static str) {
    match platform {
        Platform::LinuxX64 => ("ubuntu-24.04", "x86_64-unknown-linux-musl"),
        Platform::LinuxArm64 => ("ubuntu-24.04-arm", "aarch64-unknown-linux-musl"),
        Platform::MacosArm64 => ("macos-15", "aarch64-apple-darwin"),
    }
}

fn asset(binary: &BinaryPlan, platform: Platform) -> String {
    format!("{}-{}.tar.gz", binary.bin, platform.file_suffix())
}

fn artifact(binary: &BinaryPlan, platform: Platform) -> String {
    format!("{}-{}", binary.package, platform.file_suffix())
}

fn behind_the_gate(id: String, binary: &BinaryPlan, needs: Vec<String>) -> RenderedJob {
    let mut all = vec!["gate".to_owned(), "tag".to_owned()];
    all.extend(needs);
    RenderedJob {
        id,
        needs: all,
        condition: Some(expression(&format!(
            "{GATE_IS_READY} && {FROM_A_PASSED_PUSH} && needs.tag.outputs.{} != ''",
            binary.package
        ))),
        ..RenderedJob::default()
    }
}

fn build(binary: &BinaryPlan, platform: Platform) -> RenderedJob {
    let (runner, triple) = target(platform);
    let suffix = platform.file_suffix();
    let file = asset(binary, platform);
    let release_dir = format!("{}/{triple}/release", join(&binary.root, "target"));
    RenderedJob {
        permissions: permissions(&[("contents", "read")]),
        runs_on: runner.to_owned(),
        timeout_minutes: BUILD_TIMEOUT_MINUTES,
        steps: vec![
            checkout(VERIFIED_COMMIT, 1),
            install_tool("rust"),
            command(
                format!("Add the {triple} target"),
                format!("rustup target add {triple}"),
            ),
            command(
                format!("Build {} for {suffix}", binary.package),
                format!(
                    "cargo build --release --locked --manifest-path {} --package {} --target {triple}",
                    join(&binary.root, "Cargo.toml"),
                    binary.package
                ),
            ),
            command(
                format!("Pack {file}"),
                format!("tar -czf {file} -C {release_dir} {}", binary.bin),
            ),
            upload(artifact(binary, platform), file),
        ],
        ..behind_the_gate(
            format!("build-{}-{suffix}", binary.package),
            binary,
            Vec::new(),
        )
    }
}

fn sums(binary: &BinaryPlan, runner: &str) -> RenderedJob {
    let mut steps: Vec<RenderedStep> = binary
        .platforms
        .iter()
        .map(|platform| download(artifact(binary, *platform)))
        .collect();
    steps.push(RenderedStep {
        working_directory: Some(DIST.to_owned()),
        ..command(
            format!("Write {SUMS_FILE}"),
            format!("sha256sum {} > {SUMS_FILE}", assets(binary).join(" ")),
        )
    });
    steps.push(upload(
        format!("{}-sums", binary.package),
        format!("{DIST}/{SUMS_FILE}"),
    ));
    RenderedJob {
        no_permissions: true,
        runs_on: runner.to_owned(),
        timeout_minutes: PUBLISH_TIMEOUT_MINUTES,
        steps,
        ..behind_the_gate(format!("sums-{}", binary.package), binary, builds(binary))
    }
}

fn publish(binary: &BinaryPlan, runner: &str, helper: &HelperRelease) -> RenderedJob {
    let mut steps = vec![install_helper(helper)];
    steps.extend(
        binary
            .platforms
            .iter()
            .map(|platform| download(artifact(binary, *platform))),
    );
    steps.push(download(format!("{}-sums", binary.package)));
    steps.push(RenderedStep {
        name: format!("Publish the release of {}", binary.package),
        env: variables(&[
            ("GITHUB_TOKEN", "${{ github.token }}"),
            ("REPOSITORY", "${{ github.repository }}"),
            (
                "TAG",
                &format!("${{{{ needs.tag.outputs.{} }}}}", binary.package),
            ),
        ]),
        run: Some(format!(
            "velnor-ci release publish --repo \"$REPOSITORY\" --tag \"$TAG\" --app {RELEASE_APP_SLUG} --dir {DIST} {}",
            assets(binary).join(" ")
        )),
        ..RenderedStep::default()
    });
    RenderedJob {
        permissions: permissions(&[("contents", "write")]),
        runs_on: runner.to_owned(),
        timeout_minutes: PUBLISH_TIMEOUT_MINUTES,
        steps,
        ..behind_the_gate(
            format!("publish-{}", binary.package),
            binary,
            vec![format!("sums-{}", binary.package)],
        )
    }
}

fn builds(binary: &BinaryPlan) -> Vec<String> {
    binary
        .platforms
        .iter()
        .map(|platform| format!("build-{}-{}", binary.package, platform.file_suffix()))
        .collect()
}

fn assets(binary: &BinaryPlan) -> Vec<String> {
    binary
        .platforms
        .iter()
        .map(|platform| asset(binary, *platform))
        .collect()
}

fn command(name: String, run: String) -> RenderedStep {
    RenderedStep {
        name,
        run: Some(run),
        ..RenderedStep::default()
    }
}

fn upload(name: String, path: String) -> RenderedStep {
    RenderedStep {
        name: format!("Upload {name}"),
        uses: Some(UPLOAD_ARTIFACT.to_owned()),
        uses_comment: Some(UPLOAD_VERSION.to_owned()),
        with: vec![
            ("name".to_owned(), Value::Text(name)),
            ("path".to_owned(), Value::Text(path)),
            (
                "if-no-files-found".to_owned(),
                Value::Text("error".to_owned()),
            ),
            ("retention-days".to_owned(), Value::Int(1)),
        ],
        ..RenderedStep::default()
    }
}

fn download(name: String) -> RenderedStep {
    RenderedStep {
        name: format!("Download {name}"),
        uses: Some(DOWNLOAD_ARTIFACT.to_owned()),
        uses_comment: Some(DOWNLOAD_VERSION.to_owned()),
        with: vec![
            ("name".to_owned(), Value::Text(name)),
            ("path".to_owned(), Value::Text(DIST.to_owned())),
        ],
        ..RenderedStep::default()
    }
}
