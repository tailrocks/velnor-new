// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{HelperRelease, Platform, PrivateSources};
use velnor_release::{BinaryFormat, BinaryPlan, Libc, RELEASE_APP_SLUG, ReleasePlan};

use crate::private_steps::release_phase as private_phase;
use crate::release::{FROM_A_PASSED_PUSH, GATE_IS_READY, HOSTED_RUNNER};
use crate::release_assets::{
    COMMIT_FILE, STAGE_SHELL, archive_packaging, asset, assets, bind_version, quoted, read_version,
    selection, uploaded, versioned,
};
use crate::release_attest::{attest, verify};
use crate::release_binary_sums::{steps as binary_sums_steps, upload_paths};
use crate::release_checks::publish_needs;
use crate::release_outputs::commit_output;
use crate::release_raw::{raw_packaging, smoke_steps};
use crate::release_steps::{
    checkout, expression, install_tool, join, permissions, shell_word, variables,
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

pub(crate) struct Private<'a> {
    pub(crate) helper: &'a HelperRelease,
    pub(crate) sources: &'a PrivateSources,
}

pub(crate) struct BuildContext<'a> {
    pub(crate) source: &'a str,
    pub(crate) same_commit: RenderedStep,
    pub(crate) tag: &'a str,
    pub(crate) private: Option<Private<'a>>,
}

pub(crate) fn package_tag(binary: &BinaryPlan) -> String {
    format!("${{{{ needs.tag.outputs.{} }}}}", binary.package)
}

pub(crate) fn binary_jobs(
    plan: &ReleasePlan,
    helper: &HelperRelease,
    private: Option<&PrivateSources>,
) -> Vec<RenderedJob> {
    plan.binaries
        .iter()
        .flat_map(|binary| {
            let mut jobs: Vec<RenderedJob> = binary
                .platforms
                .iter()
                .map(|platform| build(binary, *platform, helper, private))
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

pub(crate) fn target(platform: Platform, binary: &BinaryPlan) -> (&'static str, &'static str) {
    match (platform, binary.libc) {
        (Platform::LinuxX64, Libc::Musl) => ("ubuntu-24.04", "x86_64-unknown-linux-musl"),
        (Platform::LinuxX64, Libc::Gnu) => ("ubuntu-24.04", "x86_64-unknown-linux-gnu"),
        (Platform::LinuxArm64, Libc::Musl) => ("ubuntu-24.04-arm", "aarch64-unknown-linux-musl"),
        (Platform::LinuxArm64, Libc::Gnu) => ("ubuntu-24.04-arm", "aarch64-unknown-linux-gnu"),
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

fn build(
    binary: &BinaryPlan,
    platform: Platform,
    helper: &HelperRelease,
    private: Option<&PrivateSources>,
) -> RenderedJob {
    let (runner, _) = target(platform, binary);
    RenderedJob {
        permissions: permissions(&[("contents", "read")]),
        runs_on: runner.to_owned(),
        timeout_minutes: BUILD_TIMEOUT_MINUTES,
        steps: build_steps(
            binary,
            platform,
            BuildContext {
                source: &tagged_source(binary),
                same_commit: same_commit(binary),
                tag: &package_tag(binary),
                private: private.map(|sources| Private { helper, sources }),
            },
        ),
        ..behind_the_gate(build_id(&binary.package, platform), binary, Vec::new())
    }
}

pub(crate) fn build_steps(
    binary: &BinaryPlan,
    platform: Platform,
    context: BuildContext<'_>,
) -> Vec<RenderedStep> {
    let BuildContext {
        source,
        same_commit,
        tag,
        private,
    } = context;
    let (_, triple) = target(platform, binary);
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
        BinaryFormat::Archive => archive_packaging(binary, platform, &file, &release_dir),
        BinaryFormat::Raw => {
            let mut steps = raw_packaging(binary, platform, triple, &release_dir, tag);
            steps.extend(smoke_steps(binary, platform, tag));
            steps
        }
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
    ]);
    if let Some(private) = &private {
        steps.push(install_helper(private.helper));
        steps.extend(private_phase(
            private.sources,
            &join(&binary.root, "Cargo.toml"),
        ));
    }
    let offline = if private.is_some() { " --offline" } else { "" };
    steps.extend([
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
                    "cargo build {profile_flag} --locked{offline} --manifest-path Cargo.toml{} --target {triple} --target-dir target",
                    selection(binary)
                ),
            )
        },
    ]);
    steps.extend(read_version(binary, tag));
    if versioned(binary) {
        steps.push(command(
            "Create the stage folder".to_owned(),
            format!("mkdir \"{STAGE_SHELL}\""),
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

fn tag_commit(binary: &BinaryPlan) -> String {
    format!(
        "${{{{ needs.tag.outputs.{} }}}}",
        commit_output(&binary.package)
    )
}

fn same_commit(binary: &BinaryPlan) -> RenderedStep {
    let expected = tag_commit(binary);
    let name = "Check that the tag is on the verified commit itself";
    let test = "test -n \"$COMMIT\" -a \"$CHECKED_OUT\" = \"$COMMIT\"";
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

fn publish(plan: &ReleasePlan, binary: &BinaryPlan, helper: &HelperRelease) -> RenderedJob {
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
            (
                "path".to_owned(),
                if path.contains("${{") {
                    Value::Expr(path)
                } else {
                    Value::Text(path)
                },
            ),
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
