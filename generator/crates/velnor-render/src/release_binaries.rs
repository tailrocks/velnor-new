// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{HelperRelease, Platform};
use velnor_release::{BinaryFormat, BinaryPlan, RELEASE_APP_SLUG, ReleasePlan};

use crate::release::{FROM_A_PASSED_PUSH, GATE_IS_READY, HOSTED_RUNNER, VERIFIED_COMMIT};
use crate::release_outputs::commit_output;
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
            jobs.push(publish(binary, helper));
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

fn raw_name(binary: &BinaryPlan, platform: Platform) -> String {
    format!("{}-{}", binary.bin, platform.file_suffix())
}

fn asset(binary: &BinaryPlan, platform: Platform) -> String {
    match binary.format {
        BinaryFormat::Archive => format!("{}.tar.gz", raw_name(binary, platform)),
        BinaryFormat::Raw => raw_name(binary, platform),
    }
}

fn published_files(binary: &BinaryPlan, platform: Platform) -> Vec<String> {
    let name = raw_name(binary, platform);
    match binary.format {
        BinaryFormat::Archive => vec![asset(binary, platform)],
        BinaryFormat::Raw => vec![
            name.clone(),
            format!("{name}.sha256"),
            format!("{name}.provenance.json"),
        ],
    }
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
            format!("tar -czf {file} -C {release_dir} {}", binary.bin),
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
                    "cargo build --release --locked --manifest-path Cargo.toml --package {} --bin {} --target {triple} --target-dir target",
                    binary.package, binary.bin
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
        ..behind_the_gate(
            format!("build-{}-{suffix}", binary.package),
            binary,
            Vec::new(),
        )
    }
}

fn raw_packaging(
    binary: &BinaryPlan,
    platform: Platform,
    triple: &str,
    release_dir: &str,
) -> Vec<RenderedStep> {
    let name = raw_name(binary, platform);
    let checksum = if platform == Platform::MacosArm64 {
        "shasum -a 256"
    } else {
        "sha256sum"
    };
    let (before, after) = binary
        .tag_pattern
        .as_str()
        .split_once("{{ version }}")
        .unwrap_or_default();
    let version = if after.is_empty() {
        format!("$tag[{}:]", before.len())
    } else {
        format!("$tag[{}:-{}]", before.len(), after.len())
    };
    let record = format!(
        "jq -e -R -s --arg repository \"$REPOSITORY\" --arg commit \"$COMMIT\" --arg tag \"$TAG\" --arg target {triple} --arg asset {name} --rawfile rust {name}.rustc '{{schema: 1}} + capture(\"\\\\A(?<sha256>[0-9a-f]{{64}})  {name}\\\\n\\\\z\") + {{tag: $tag, version: {version}, repository: $repository, commit: $commit, target: $target, asset: $asset, toolchain: {{rust: $rust[0:-1]}}}}' {name}.sha256 > {name}.provenance.json"
    );
    vec![
        command(
            format!("Name {} for {}", binary.bin, platform.file_suffix()),
            format!("cp {release_dir}/{} {name}", binary.bin),
        ),
        command(
            format!("Checksum {name}"),
            format!("{checksum} {name} > {name}.sha256"),
        ),
        command(
            format!("Record the rustc version for {name}"),
            format!("rustc --version > {name}.rustc"),
        ),
        RenderedStep {
            env: variables(&[
                ("REPOSITORY", "${{ github.repository }}"),
                ("COMMIT", "${{ steps.source.outputs.commit }}"),
                (
                    "TAG",
                    &format!("${{{{ needs.tag.outputs.{} }}}}", binary.package),
                ),
            ]),
            ..command(format!("Record provenance of {name}"), record)
        },
    ]
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

fn publish(binary: &BinaryPlan, helper: &HelperRelease) -> RenderedJob {
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
        .flat_map(|platform| published_files(binary, *platform))
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
