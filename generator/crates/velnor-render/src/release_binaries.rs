// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{HelperRelease, Platform};
use velnor_release::{
    BinaryPlan, RELEASE_APP_SLUG, ReleasePlan, asset_name_uses_version, expand_asset_name,
};

use crate::release::{FROM_A_PASSED_PUSH, GATE_IS_READY, HOSTED_RUNNER, VERIFIED_COMMIT};
use crate::release_outputs::commit_output;
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
            jobs.push(sums(binary));
            jobs.push(publish(binary, helper));
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

const VERSION_REFERENCE: &str = "${VERSION}";
const STAGE: &str = "stage";

fn versioned(binary: &BinaryPlan) -> bool {
    binary
        .asset_name
        .as_deref()
        .is_some_and(asset_name_uses_version)
}

fn asset(binary: &BinaryPlan, platform: Platform) -> String {
    match &binary.asset_name {
        None => format!("{}-{}.tar.gz", binary.bin, platform.file_suffix()),
        Some(template) => format!(
            "{}.tar.gz",
            expand_asset_name(
                template,
                &binary.bin,
                platform.file_suffix(),
                target(platform).1,
                VERSION_REFERENCE,
            )
        ),
    }
}

fn word(binary: &BinaryPlan, name: &str) -> String {
    if versioned(binary) {
        format!("\"{name}\"")
    } else {
        shell_word(name)
    }
}

fn artifact(binary: &BinaryPlan, platform: Platform) -> String {
    format!("{}-{}", binary.package, platform.file_suffix())
}

fn read_version(binary: &BinaryPlan) -> Option<RenderedStep> {
    versioned(binary).then(|| {
        let pattern = binary
            .tag_pattern
            .as_str()
            .replace("{{ package }}", &binary.package);
        let prefix = pattern.split("{{ version }}").next().unwrap_or_default();
        RenderedStep {
            env: variables(&[(
                "TAG",
                &format!("${{{{ needs.tag.outputs.{} }}}}", binary.package),
            )]),
            ..command(
                "Read the version of the tag".to_owned(),
                format!("echo \"VERSION=${{TAG#{prefix}}}\" >> \"$GITHUB_ENV\""),
            )
        }
    })
}

fn pack(binary: &BinaryPlan, file: &str, release_dir: &str) -> String {
    if versioned(binary) {
        format!(
            "tar -czf \"{STAGE}/{file}\" -C {release_dir} {}",
            binary.bin
        )
    } else {
        format!(
            "tar -czf {} -C {release_dir} {}",
            shell_word(file),
            binary.bin
        )
    }
}

fn uploaded(binary: &BinaryPlan, file: &str) -> String {
    if versioned(binary) {
        format!("{STAGE}/*")
    } else {
        file.to_owned()
    }
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
    let (runner, triple) = target(platform);
    let suffix = platform.file_suffix();
    let file = asset(binary, platform);
    let release_dir = shell_word(&format!(
        "{}/{triple}/release",
        join(&binary.root, "target")
    ));
    let linux = triple.ends_with("linux-musl");
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
    steps.extend(read_version(binary));
    if versioned(binary) {
        steps.push(command(
            "Create the stage folder".to_owned(),
            format!("mkdir {STAGE}"),
        ));
    }
    steps.extend([
        command(format!("Pack {file}"), pack(binary, &file, &release_dir)),
        upload(artifact(binary, platform), uploaded(binary, &file)),
    ]);
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
    let names = quoted(binary, assets(binary)).join(" ");
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
                "mise --no-config --no-env --no-hooks exec gh@{GH_VERSION} -- gh attestation verify {} --repo \"$GITHUB_REPOSITORY\" --source-digest \"$GITHUB_SHA\" --source-ref \"$GITHUB_REF\" --signer-workflow \"$GITHUB_REPOSITORY/.github/workflows/release.yml\" --signer-digest \"$GITHUB_SHA\"",
                word(binary, &format!("{DIST}/{name}"))
            )),
            ..RenderedStep::default()
        })
        .collect()
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
    steps.extend(read_version(binary));
    if binary.attest {
        steps.push(install_tool(&format!("gh@{GH_VERSION}")));
        steps.extend(verifications(binary));
    }
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
            quoted(binary, assets(binary)).join(" ")
        )),
        ..RenderedStep::default()
    });
    RenderedJob {
        permissions: if binary.attest {
            permissions(&[("attestations", "read"), ("contents", "write")])
        } else {
            permissions(&[("contents", "write")])
        },
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
        .map(|platform| asset(binary, *platform))
        .collect()
}

fn quoted(binary: &BinaryPlan, names: Vec<String>) -> Vec<String> {
    names.iter().map(|name| word(binary, name)).collect()
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
