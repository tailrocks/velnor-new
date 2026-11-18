// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{HelperRelease, Platform};
use velnor_release::{BinaryFormat, BinaryPlan, Libc, RELEASE_APP_SLUG, ReleasePlan};

use crate::release::{FROM_A_PASSED_PUSH, GATE_IS_READY, HOSTED_RUNNER, VERIFIED_COMMIT};
use crate::release_assets::{
    COMMIT_FILE, STAGE, asset, assets, pack, quoted, read_version, selection, sidecar_step,
    uploaded, versioned, word,
};
use crate::release_checks::publish_needs;
use crate::release_outputs::commit_output;
use crate::release_raw::{raw_packaging, smoke_steps};
use crate::release_steps::{
    TAGGED_COMMIT_HELPER, checkout, expression, install_tool, join, permissions, shell_word,
    variables,
};
use crate::rules::{ATTEST_ACTION, RenderedJob, RenderedStep, Value};
use crate::steps::install_helper;

const UPLOAD_ARTIFACT: &str = "actions/upload-artifact@cf430e030ddbb5b0abf93d22962f4752f3646cd9";
const ATTEST_COMMIT: &str = "4d101475d8b20a2381f78447822ac1eab6504dd8";
const ATTEST_VERSION: &str = "v4.2.2";
const GH_VERSION: &str = "2.103.3";
const UPLOAD_VERSION: &str = "v7.0.2";
const DOWNLOAD_ARTIFACT: &str =
    "actions/download-artifact@9000827ccba6bdab643e8b6fd33ac0654aef8333";
const DOWNLOAD_VERSION: &str = "v8.0.2";
const BUILD_TIMEOUT_MINUTES: u16 = 60;
const PUBLISH_TIMEOUT_MINUTES: u16 = 10;
const DIST: &str = "dist";
const SUMS_FILE: &str = "SHA256SUMS";

pub(crate) fn binary_jobs(plan: &ReleasePlan, helper: &HelperRelease) -> Vec<RenderedJob> {
    plan.binaries
        .iter()
        .flat_map(|binary| {
            let mut jobs: Vec<RenderedJob> = binary
                .platforms
                .iter()
                .map(|platform| build(binary, *platform, helper))
                .collect();
            jobs.push(sums(binary, helper));
            if binary.attest {
                jobs.push(verify(binary));
            }
            jobs.push(publish(plan, binary, helper));
            jobs
        })
        .collect()
}

pub(crate) fn target(platform: Platform, binary: &BinaryPlan) -> (&'static str, &'static str) {
    match (platform, binary.libc) {
        (Platform::LinuxX64, Libc::Musl) => ("ubuntu-24.04", "x86_64-unknown-linux-musl"),
        (Platform::LinuxX64, Libc::Gnu) => ("ubuntu-24.04", "x86_64-unknown-linux-gnu"),
        (Platform::LinuxArm64, Libc::Musl) => ("ubuntu-24.04-arm", "aarch64-unknown-linux-musl"),
        (Platform::LinuxArm64, Libc::Gnu) => ("ubuntu-24.04-arm", "aarch64-unknown-linux-gnu"),
        (Platform::MacosArm64, _) => ("macos-15", "aarch64-apple-darwin"),
    }
}

fn artifact(binary: &BinaryPlan, platform: Platform) -> String {
    artifact_name(&binary.package, platform)
}

pub(crate) fn artifact_name(package: &str, platform: Platform) -> String {
    format!("{package}-{}", platform.file_suffix())
}

pub(crate) fn build_id(package: &str, platform: Platform) -> String {
    format!("build-{package}-{}", platform.file_suffix())
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

fn build(binary: &BinaryPlan, platform: Platform, helper: &HelperRelease) -> RenderedJob {
    let (runner, triple) = target(platform, binary);
    let suffix = platform.file_suffix();
    let file = asset(binary, platform);
    let release_dir = shell_word(&format!(
        "{}/{triple}/release",
        join(&binary.root, "target")
    ));
    let linux = triple.ends_with("linux-musl");
    let packaging = match binary.format {
        BinaryFormat::Archive => {
            let mut steps = vec![command(
                format!("Pack {file}"),
                pack(binary, &file, &release_dir),
            )];
            steps.extend(sidecar_step(binary, platform, &file));
            steps
        }
        BinaryFormat::Raw => {
            let mut steps = raw_packaging(binary, platform, triple, &release_dir);
            steps.extend(smoke_steps(binary, platform));
            steps
        }
    };
    let mut steps = vec![
        RenderedStep {
            id: Some("source".to_owned()),
            ..checkout(&tagged_source(binary), 1)
        },
        same_commit(binary, helper),
    ];
    if linux {
        steps.push(command(
            "Install the musl C compiler".to_owned(),
            "sudo apt-get install --yes musl-tools".to_owned(),
        ));
    }
    steps.extend([
        install_tool("rust"),
        command(
            format!("Add the {triple} target"),
            format!("rustup target add {triple}"),
        ),
        RenderedStep {
            working_directory: (binary.root != ".").then(|| binary.root.clone()),
            env: if linux {
                vec![("TARGET_CC".to_owned(), Value::Text("musl-gcc".to_owned()))]
            } else {
                Vec::new()
            },
            ..command(
                format!("Build {} for {suffix}", binary.package),
                format!(
                    "cargo build --release --locked --manifest-path Cargo.toml{} --target {triple} --target-dir target",
                    selection(binary)
                ),
            )
        },
    ]);
    steps.extend(read_version(binary));
    if versioned(binary) {
        steps.push(command(
            "Create the stage folder".to_owned(),
            format!("mkdir {STAGE}"),
        ));
    }
    steps.extend(packaging);
    steps.push(upload(
        artifact(binary, platform),
        uploaded(binary, platform),
    ));
    RenderedJob {
        permissions: permissions(&[("contents", "read")]),
        runs_on: runner.to_owned(),
        timeout_minutes: BUILD_TIMEOUT_MINUTES,
        steps,
        ..behind_the_gate(build_id(&binary.package, platform), binary, Vec::new())
    }
}

fn tag_commit(binary: &BinaryPlan, helper: &HelperRelease) -> String {
    if helper.version().at_least(TAGGED_COMMIT_HELPER) {
        format!(
            "${{{{ needs.tag.outputs.{} }}}}",
            commit_output(&binary.package)
        )
    } else {
        VERIFIED_COMMIT.to_owned()
    }
}

fn same_commit(binary: &BinaryPlan, helper: &HelperRelease) -> RenderedStep {
    let expected = tag_commit(binary, helper);
    let (name, test) = if helper.version().at_least(TAGGED_COMMIT_HELPER) {
        (
            "Check that the tag is on the verified commit itself",
            "test -n \"$COMMIT\" -a \"$CHECKED_OUT\" = \"$COMMIT\"",
        )
    } else {
        (
            "Check that the tag is the commit the gate verified",
            "test \"$CHECKED_OUT\" = \"$COMMIT\"",
        )
    };
    RenderedStep {
        env: variables(&[
            ("COMMIT", &expected),
            ("CHECKED_OUT", "${{ steps.source.outputs.commit }}"),
        ]),
        ..command(name.to_owned(), test.to_owned())
    }
}

fn tagged_source(binary: &BinaryPlan) -> String {
    format!("refs/tags/${{{{ needs.tag.outputs.{} }}}}", binary.package)
}

fn sums(binary: &BinaryPlan, helper: &HelperRelease) -> RenderedJob {
    let mut steps: Vec<RenderedStep> = binary
        .platforms
        .iter()
        .map(|platform| download(artifact(binary, *platform)))
        .collect();
    steps.extend(read_version(binary));
    if binary.commit_file {
        steps.push(RenderedStep {
            working_directory: Some(DIST.to_owned()),
            env: variables(&[("COMMIT", &tag_commit(binary, helper))]),
            ..command(
                "Record the commit of the release".to_owned(),
                format!("echo \"$COMMIT\" > {COMMIT_FILE}"),
            )
        });
    }
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
    let sums_files = if binary.commit_file {
        format!("{DIST}/{SUMS_FILE}\n{DIST}/{COMMIT_FILE}")
    } else {
        format!("{DIST}/{SUMS_FILE}")
    };
    steps.push(upload(format!("{}-sums", binary.package), sums_files));
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

fn attest(binary: &BinaryPlan) -> RenderedStep {
    RenderedStep {
        name: format!("Attest the build provenance of {}", binary.package),
        uses: Some(format!("{ATTEST_ACTION}@{ATTEST_COMMIT}")),
        uses_comment: Some(ATTEST_VERSION.to_owned()),
        with: vec![("subject-path".to_owned(), Value::Text(format!("{DIST}/*")))],
        ..RenderedStep::default()
    }
}

fn verifications(binary: &BinaryPlan) -> Vec<RenderedStep> {
    assets(binary)
        .into_iter()
        .chain([SUMS_FILE.to_owned()])
        .map(|name| RenderedStep {
            name: format!("Verify the build provenance of {name}"),
            env: variables(&[("GH_TOKEN", "${{ github.token }}")]),
            run: Some(format!(
                "mise --no-config --no-env --no-hooks exec gh@{GH_VERSION} -- gh attestation verify {} --repo \"$GITHUB_REPOSITORY\" --source-digest \"$GITHUB_SHA\" --source-ref \"$GITHUB_REF\" --signer-workflow \"$GITHUB_REPOSITORY/.github/workflows/release.yml\" --signer-digest \"$GITHUB_SHA\" --deny-self-hosted-runners",
                word(binary, &format!("{DIST}/{name}"))
            )),
            ..RenderedStep::default()
        })
        .collect()
}

fn verify(binary: &BinaryPlan) -> RenderedJob {
    let mut steps: Vec<RenderedStep> = binary
        .platforms
        .iter()
        .map(|platform| download(artifact(binary, *platform)))
        .collect();
    steps.push(download(format!("{}-sums", binary.package)));
    steps.extend(read_version(binary));
    steps.push(install_tool(&format!("gh@{GH_VERSION}")));
    steps.extend(verifications(binary));
    RenderedJob {
        permissions: permissions(&[("attestations", "read")]),
        runs_on: HOSTED_RUNNER.to_owned(),
        timeout_minutes: PUBLISH_TIMEOUT_MINUTES,
        steps,
        ..behind_the_gate(
            format!("verify-{}", binary.package),
            binary,
            vec![format!("sums-{}", binary.package)],
        )
    }
}

fn publish(plan: &ReleasePlan, binary: &BinaryPlan, helper: &HelperRelease) -> RenderedJob {
    let mut steps = vec![install_helper(helper)];
    steps.extend(
        binary
            .platforms
            .iter()
            .map(|platform| download(artifact(binary, *platform))),
    );
    steps.push(download(format!("{}-sums", binary.package)));
    steps.extend(read_version(binary));
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
            "velnor-ci release publish --repo=\"$REPOSITORY\" --tag=\"$TAG\" --app={RELEASE_APP_SLUG} --dir={DIST} {}",
            quoted(binary, &assets(binary)).join(" ")
        )),
        ..RenderedStep::default()
    });
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

fn builds(binary: &BinaryPlan) -> Vec<String> {
    binary
        .platforms
        .iter()
        .map(|platform| build_id(&binary.package, *platform))
        .collect()
}

pub(crate) fn command(name: String, run: String) -> RenderedStep {
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

pub(crate) fn download(name: String) -> RenderedStep {
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
