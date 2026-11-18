// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{HelperRelease, Platform};
use velnor_release::{BinaryFormat, BinaryPlan, RELEASE_APP_SLUG, ReleasePlan};

use crate::release::{FROM_A_PASSED_PUSH, GATE_IS_READY, HOSTED_RUNNER, VERIFIED_COMMIT};
use crate::release_checks::publish_needs;
use crate::release_outputs::commit_output;
use crate::release_raw::{bin_names, raw_name, raw_packaging};
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
            jobs.push(sums(binary));
            jobs.push(publish(plan, binary, helper));
            jobs
        })
        .collect()
}

fn target(platform: Platform, format: BinaryFormat) -> (&'static str, &'static str) {
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

fn asset(binary: &BinaryPlan, platform: Platform) -> String {
    match binary.format {
        BinaryFormat::Archive => format!("{}.tar.gz", raw_name(&binary.bin, platform)),
        BinaryFormat::Raw => raw_name(&binary.bin, platform),
    }
}

fn published_files(binary: &BinaryPlan, platform: Platform) -> Vec<String> {
    match binary.format {
        BinaryFormat::Archive => vec![asset(binary, platform)],
        BinaryFormat::Raw => bin_names(binary)
            .flat_map(|bin| {
                let name = raw_name(bin, platform);
                [
                    name.clone(),
                    format!("{name}.sha256"),
                    format!("{name}.provenance.json"),
                ]
            })
            .collect(),
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
    let (runner, triple) = target(platform, binary.format);
    let suffix = platform.file_suffix();
    let file = asset(binary, platform);
    let release_dir = shell_word(&format!(
        "{}/{triple}/release",
        join(&binary.root, "target")
    ));
    let linux = triple.ends_with("linux-musl");
    let packaging = match binary.format {
        BinaryFormat::Archive => vec![command(
            format!("Pack {file}"),
            format!("tar -czf {file} -C {release_dir}{}", contents(binary)),
        )],
        BinaryFormat::Raw => raw_packaging(binary, platform, triple, &release_dir),
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
    steps.extend(packaging);
    steps.push(upload(
        artifact(binary, platform),
        published_files(binary, platform).join("\n"),
    ));
    RenderedJob {
        permissions: permissions(&[("contents", "read")]),
        runs_on: runner.to_owned(),
        timeout_minutes: BUILD_TIMEOUT_MINUTES,
        steps,
        ..behind_the_gate(build_id(&binary.package, platform), binary, Vec::new())
    }
}

fn selection(binary: &BinaryPlan) -> String {
    let mut words = vec![
        "--package".to_owned(),
        binary.package.clone(),
        "--bin".to_owned(),
        binary.bin.clone(),
    ];
    let mut packages = vec![binary.package.as_str()];
    for extra in &binary.extra_bins {
        if !packages.contains(&extra.package.as_str()) {
            packages.push(&extra.package);
            words.extend(["--package".to_owned(), extra.package.clone()]);
        }
        words.extend(["--bin".to_owned(), extra.bin.clone()]);
    }
    format!(" {}", words.join(" "))
}

fn contents(binary: &BinaryPlan) -> String {
    let mut words = vec![binary.bin.clone()];
    words.extend(binary.extra_bins.iter().map(|extra| extra.bin.clone()));
    for path in &binary.include {
        let (dir, name) = path.rsplit_once('/').unwrap_or(("", path));
        let base = if dir.is_empty() {
            "$GITHUB_WORKSPACE".to_owned()
        } else {
            format!("$GITHUB_WORKSPACE/{dir}")
        };
        words.extend(["-C".to_owned(), format!("\"{base}\""), name.to_owned()]);
    }
    format!(" {}", words.join(" "))
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
    let names = assets(binary).join(" ");
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
        format!("{}-sums", binary.package),
        format!("{DIST}/{SUMS_FILE}"),
    ));
    RenderedJob {
        no_permissions: true,
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
            assets(binary).join(" ")
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
            [format!("sums-{}", binary.package)]
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

fn assets(binary: &BinaryPlan) -> Vec<String> {
    binary
        .platforms
        .iter()
        .flat_map(|platform| published_files(binary, *platform))
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
