// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::HelperRelease;
use velnor_release::{BinaryPlan, FamilyPlan, RELEASE_APP_SLUG, ReleasePlan};

use crate::release::HOSTED_RUNNER;
use crate::release_assets::{COMMIT_FILE, bind_version, published_files, read_version, word};
use crate::release_attest::{provenance, verification_step, verification_steps};
use crate::release_binaries::{
    DIST, PUBLISH_TIMEOUT_MINUTES, SUMS_FILE, artifact, build_id, download, upload,
};
use crate::release_binary_sums::digests;
use crate::release_checks::family_publish_needs;
use crate::release_family::{COMMIT_OUTPUT, TAG_OUTPUT, after_the_family};
use crate::release_optional::{
    after_the_builds, artifact_download, is_optional, optional_files, when_present,
};
use crate::release_source::Source;
use crate::release_steps::{expression, permissions, shell_word, variables};
use crate::rules::{RenderedJob, RenderedStep};
use crate::steps::install_helper;

const SUMS_ARTIFACT: &str = "family-sums";

pub(crate) fn extras(family: &FamilyPlan) -> Vec<String> {
    family
        .commit_file
        .then(|| COMMIT_FILE.to_owned())
        .into_iter()
        .chain(family.checksums_file.clone())
        .collect()
}

fn artifact_downloads(plan: &ReleasePlan) -> Vec<RenderedStep> {
    plan.binaries
        .iter()
        .flat_map(|binary| {
            binary.platforms.iter().map(|platform| {
                artifact_download(artifact(binary, *platform), is_optional(binary, *platform))
            })
        })
        .collect()
}

fn files(
    plan: &ReleasePlan,
    family: &FamilyPlan,
    prefix: Option<&str>,
    optional: bool,
) -> Vec<(String, String)> {
    let path = |name: &str| prefix.map_or_else(|| name.to_owned(), |dir| format!("{dir}/{name}"));
    let published = plan.binaries.iter().flat_map(|binary: &BinaryPlan| {
        binary
            .platforms
            .iter()
            .filter(move |platform| is_optional(binary, **platform) == optional)
            .flat_map(|platform| published_files(binary, *platform))
            .map(|name| (name.clone(), word(binary, &path(&name))))
    });
    let extra = extras(family)
        .into_iter()
        .filter(|_| !optional)
        .map(|name| (name.clone(), shell_word(&path(&name))));
    published.chain(extra).collect()
}

fn words(
    plan: &ReleasePlan,
    family: &FamilyPlan,
    prefix: Option<&str>,
    optional: bool,
) -> Vec<String> {
    files(plan, family, prefix, optional)
        .into_iter()
        .map(|(_, word)| word)
        .collect()
}

fn read_family_version(plan: &ReleasePlan) -> Option<RenderedStep> {
    let tag = format!("${{{{ {TAG_OUTPUT} }}}}");
    plan.binaries
        .iter()
        .find_map(|binary| read_version(binary, &tag))
}

fn builds(plan: &ReleasePlan) -> Vec<String> {
    plan.binaries
        .iter()
        .flat_map(|binary| {
            binary
                .platforms
                .iter()
                .map(|platform| build_id(&binary.package, *platform))
        })
        .collect()
}

pub(crate) fn sums(plan: &ReleasePlan, family: &FamilyPlan, source: &Source) -> RenderedJob {
    let mut steps = artifact_downloads(plan);
    steps.extend(read_family_version(plan));
    if family.commit_file {
        steps.push(RenderedStep {
            working_directory: Some(DIST.to_owned()),
            env: variables(&[("COMMIT", &format!("${{{{ {COMMIT_OUTPUT} }}}}"))]),
            ..crate::release_binaries::command(
                "Record the commit of the release".to_owned(),
                format!("echo \"$COMMIT\" > {COMMIT_FILE}"),
            )
        });
    }
    if let Some(file) = &family.checksums_file {
        let every: Vec<&BinaryPlan> = plan.binaries.iter().collect();
        steps.extend(digests(&every, file, ""));
    }
    let names = words(plan, family, None, false).join(" ");
    for (tool, on_macos) in [("sha256sum", false), ("shasum -a 256", true)] {
        let comparison = if on_macos { "==" } else { "!=" };
        steps.push(RenderedStep {
            working_directory: Some(DIST.to_owned()),
            condition: Some(expression(&format!("runner.os {comparison} 'macOS'"))),
            ..crate::release_binaries::command(
                format!("Write {SUMS_FILE}"),
                format!("{tool} {names} > {SUMS_FILE}"),
            )
        });
    }
    for name in plan.binaries.iter().flat_map(optional_files) {
        for (tool, on_macos) in [("sha256sum", false), ("shasum -a 256", true)] {
            let comparison = if on_macos { "==" } else { "!=" };
            steps.push(RenderedStep {
                working_directory: Some(DIST.to_owned()),
                condition: Some(when_present(
                    &name,
                    Some(&format!("runner.os {comparison} 'macOS'")),
                )),
                ..crate::release_binaries::command(
                    format!("Add {name} to {SUMS_FILE}"),
                    format!("{tool} {} >> {SUMS_FILE}", shell_word(&name)),
                )
            });
        }
    }
    if family.attest {
        steps.push(provenance(
            "Attest the build provenance of the family".to_owned(),
        ));
    }
    let uploaded: Vec<String> = [SUMS_FILE.to_owned()]
        .into_iter()
        .chain(family.checksums_file.clone())
        .chain(family.commit_file.then(|| COMMIT_FILE.to_owned()))
        .map(|name| format!("{DIST}/{name}"))
        .collect();
    steps.push(upload(SUMS_ARTIFACT.to_owned(), uploaded.join("\n")));
    bind_version(&mut steps);
    RenderedJob {
        no_permissions: !family.attest,
        permissions: if family.attest {
            permissions(&[("attestations", "write"), ("id-token", "write")])
        } else {
            Vec::new()
        },
        runs_on: HOSTED_RUNNER.to_owned(),
        timeout_minutes: PUBLISH_TIMEOUT_MINUTES,
        steps,
        ..after_the_builds("sums-family", plan, builds(plan), source)
    }
}

pub(crate) fn verify(plan: &ReleasePlan, family: &FamilyPlan, source: &Source) -> RenderedJob {
    let mut steps = artifact_downloads(plan);
    steps.push(download(SUMS_ARTIFACT.to_owned()));
    steps.extend(read_family_version(plan));
    let paths = files(plan, family, Some(DIST), false)
        .into_iter()
        .chain([(
            SUMS_FILE.to_owned(),
            shell_word(&format!("{DIST}/{SUMS_FILE}")),
        )])
        .collect();
    steps.extend(verification_steps(paths));
    steps.extend(
        files(plan, family, Some(DIST), true)
            .into_iter()
            .map(|(name, path)| verification_step(&name, &path, Some(when_present(&name, None)))),
    );
    bind_version(&mut steps);
    RenderedJob {
        permissions: permissions(&[("attestations", "read")]),
        runs_on: HOSTED_RUNNER.to_owned(),
        timeout_minutes: PUBLISH_TIMEOUT_MINUTES,
        steps,
        ..after_the_family("verify-family", vec!["sums-family".to_owned()], source)
    }
}

fn publish_files(plan: &ReleasePlan, family: &FamilyPlan) -> String {
    let optional = words(plan, family, None, true)
        .into_iter()
        .map(|word| format!("--optional-asset={word}"));
    words(plan, family, None, false)
        .into_iter()
        .chain(optional)
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn publish(
    plan: &ReleasePlan,
    family: &FamilyPlan,
    helper: &HelperRelease,
    source: &Source,
) -> RenderedJob {
    let mut steps = vec![install_helper(helper)];
    steps.extend(artifact_downloads(plan));
    steps.push(download(SUMS_ARTIFACT.to_owned()));
    steps.extend(read_family_version(plan));
    steps.push(RenderedStep {
        name: "Publish the family release".to_owned(),
        env: variables(&[
            ("GITHUB_TOKEN", "${{ github.token }}"),
            ("REPOSITORY", "${{ github.repository }}"),
            ("TAG", &format!("${{{{ {TAG_OUTPUT} }}}}")),
        ]),
        run: Some(format!(
            "velnor-ci release publish --repo=\"$REPOSITORY\" --tag=\"$TAG\" --app={RELEASE_APP_SLUG} --dir={DIST} {}",
            publish_files(plan, family)
        )),
        ..RenderedStep::default()
    });
    bind_version(&mut steps);
    let gate = if family.attest {
        "verify-family"
    } else {
        "sums-family"
    };
    RenderedJob {
        permissions: permissions(&[("contents", "write")]),
        runs_on: HOSTED_RUNNER.to_owned(),
        timeout_minutes: PUBLISH_TIMEOUT_MINUTES,
        steps,
        ..after_the_family(
            "publish-family",
            [gate.to_owned()]
                .into_iter()
                .chain(family_publish_needs(plan))
                .collect(),
            source,
        )
    }
}
