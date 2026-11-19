// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{HelperRelease, Platform};
use velnor_release::{BinaryFormat, BinaryPlan, RELEASE_APP_SLUG, ReleasePlan};

use crate::release::{FROM_A_PASSED_PUSH, GATE_IS_READY, HOSTED_RUNNER, VERIFIED_COMMIT};
use crate::release_assets::{
    STAGE, asset, assets, bind_version, pack, quoted, read_version, selection, uploaded, versioned,
};
use crate::release_attest::{attest, verify};
use crate::release_checks::publish_needs;
use crate::release_outputs::commit_output;
use crate::release_raw::raw_packaging;
use crate::release_steps::{
    TAGGED_COMMIT_HELPER, checkout, expression, install_tool, join, permissions, shell_word,
    variables,
};
use crate::rules::{RenderedJob, RenderedStep, Value};
use crate::steps::install_helper;

const UPLOAD_ARTIFACT: &str = "actions/upload-artifact@cf430e030ddbb5b0abf93d22962f4752f3646cd9";
const UPLOAD_VERSION: &str = "v7.0.2";
const DOWNLOAD_ARTIFACT: &str =
    "actions/download-artifact@9000827ccba6bdab643e8b6fd33ac0654aef8333";
const DOWNLOAD_VERSION: &str = "v8.0.2";
pub(crate) const BUILD_TIMEOUT_MINUTES: u16 = 60;
pub(crate) const PUBLISH_TIMEOUT_MINUTES: u16 = 10;
pub(crate) const DIST: &str = "dist";
pub(crate) const SUMS_FILE: &str = "SHA256SUMS";

pub(crate) fn binary_jobs(plan: &ReleasePlan, helper: &HelperRelease) -> Vec<RenderedJob> {
    plan.binaries
        .iter()
        .flat_map(|binary| {
            let mut jobs: Vec<RenderedJob> = binary
                .platforms
                .iter()
                .map(|platform| build(binary, *platform, helper))
                .collect();
            jobs.push(sums(binary));
            if binary.attest {
                jobs.push(verify(binary));
            }
            jobs.push(publish(plan, binary, helper));
            jobs
        })
        .collect()
}

pub(crate) fn target(platform: Platform, format: BinaryFormat) -> (&'static str, &'static str) {
    match (platform, format) {
        (Platform::LinuxX64, BinaryFormat::Archive) => {
            ("ubuntu-24.04", "x86_64-unknown-linux-musl")
        }
        (Platform::LinuxX64, BinaryFormat::Raw) => ("ubuntu-24.04", "x86_64-unknown-linux-gnu"),
        (Platform::LinuxArm64, BinaryFormat::Archive) => {
            ("ubuntu-24.04-arm", "aarch64-unknown-linux-musl")
        }
        (Platform::LinuxArm64, BinaryFormat::Raw) => {
            ("ubuntu-24.04-arm", "aarch64-unknown-linux-gnu")
        }
        (Platform::MacosArm64, _) => ("macos-15", "aarch64-apple-darwin"),
    }
}

pub(crate) fn artifact(binary: &BinaryPlan, platform: Platform) -> String {
    artifact_name(&binary.package, platform)
}

pub(crate) fn artifact_name(package: &str, platform: Platform) -> String {
    format!("{package}-{}", platform.file_suffix())
}

pub(crate) fn build_id(package: &str, platform: Platform) -> String {
    format!("build-{package}-{}", platform.file_suffix())
}

pub(crate) fn behind_the_gate(id: String, binary: &BinaryPlan, needs: Vec<String>) -> RenderedJob {
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
    let (runner, _) = target(platform, binary.format);
    RenderedJob {
        permissions: permissions(&[("contents", "read")]),
        runs_on: runner.to_owned(),
        timeout_minutes: BUILD_TIMEOUT_MINUTES,
        steps: build_steps(
            binary,
            platform,
            &tagged_source(binary),
            same_commit(binary, helper),
        ),
        ..behind_the_gate(build_id(&binary.package, platform), binary, Vec::new())
    }
}

pub(crate) fn build_steps(
    binary: &BinaryPlan,
    platform: Platform,
    source: &str,
    same_commit: RenderedStep,
) -> Vec<RenderedStep> {
    let (_, triple) = target(platform, binary.format);
    let suffix = platform.file_suffix();
    let file = asset(binary, platform);
    let profile = binary.profile.as_deref();
    let release_dir = shell_word(&format!(
        "{}/{triple}/{}",
        join(&binary.root, "target"),
        profile.unwrap_or("release")
    ));
    let profile_flag = profile.map_or_else(
        || "--release".to_owned(),
        |name| format!("--profile {name}"),
    );
    let linux = triple.ends_with("linux-musl");
    let packaging = match binary.format {
        BinaryFormat::Archive => vec![command(
            format!("Pack {file}"),
            pack(binary, &file, &release_dir),
        )],
        BinaryFormat::Raw => raw_packaging(binary, platform, triple, &release_dir),
    };
    let mut steps = vec![
        RenderedStep {
            id: Some("source".to_owned()),
            ..checkout(source, 1)
        },
        same_commit,
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
                    "cargo build {profile_flag} --locked --manifest-path Cargo.toml{} --target {triple} --target-dir target",
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
    bind_version(&mut steps);
    steps
}

fn same_commit(binary: &BinaryPlan, helper: &HelperRelease) -> RenderedStep {
    let (name, expected, test) = if helper.version().at_least(TAGGED_COMMIT_HELPER) {
        (
            "Check that the tag is on the verified commit itself",
            format!(
                "${{{{ needs.tag.outputs.{} }}}}",
                commit_output(&binary.package)
            ),
            "test -n \"$COMMIT\" -a \"$CHECKED_OUT\" = \"$COMMIT\"",
        )
    } else {
        (
            "Check that the tag is the commit the gate verified",
            VERIFIED_COMMIT.to_owned(),
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

fn sums(binary: &BinaryPlan) -> RenderedJob {
    let mut steps: Vec<RenderedStep> = binary
        .platforms
        .iter()
        .map(|platform| download(artifact(binary, *platform)))
        .collect();
    steps.extend(read_version(binary));
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
    steps.push(upload(
        format!("{}-sums", binary.package),
        format!("{DIST}/{SUMS_FILE}"),
    ));
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

pub(crate) fn upload(name: String, path: String) -> RenderedStep {
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
    download_to(name, DIST)
}

pub(crate) fn download_to(name: String, path: &str) -> RenderedStep {
    RenderedStep {
        name: format!("Download {name}"),
        uses: Some(DOWNLOAD_ARTIFACT.to_owned()),
        uses_comment: Some(DOWNLOAD_VERSION.to_owned()),
        with: vec![
            ("name".to_owned(), Value::Text(name)),
            ("path".to_owned(), Value::Text(path.to_owned())),
        ],
        ..RenderedStep::default()
    }
}
