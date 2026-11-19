// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::Platform;
use velnor_release::BinaryPlan;

use crate::release_assets::{STAGE, STAGE_SHELL, stem, versioned, word};
use crate::release_binaries::command;
use crate::release_steps::variables;
use crate::rules::RenderedStep;

pub(crate) fn bin_names(binary: &BinaryPlan) -> impl Iterator<Item = &str> {
    std::iter::once(binary.bin.as_str())
        .chain(binary.extra_bins.iter().map(|extra| extra.bin.as_str()))
}

const SIDECAR_PATTERN: &str =
    "\"\\\\A(?<sha256>[0-9a-f]{64})  \\\\Q\" + $asset + \"\\\\E\\\\n\\\\z\"";

pub(crate) fn raw_packaging(
    binary: &BinaryPlan,
    platform: Platform,
    triple: &str,
    release_dir: &str,
    tag: &str,
) -> Vec<RenderedStep> {
    let checksum = if platform == Platform::MacosArm64 {
        "shasum -a 256"
    } else {
        "sha256sum"
    };
    let staged = versioned(binary);
    let folder = staged.then(|| STAGE.to_owned());
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
    bin_names(binary)
        .flat_map(|bin| {
            let name = stem(binary, bin, platform);
            let file = word(binary, &name);
            let sum = word(binary, &format!("{name}.sha256"));
            let rustc = word(binary, &format!("{name}.rustc"));
            let provenance = word(binary, &format!("{name}.provenance.json"));
            let copy_to = word(
                binary,
                &if staged {
                    format!("{STAGE_SHELL}/{name}")
                } else {
                    name.clone()
                },
            );
            let record = format!(
                "jq -e -R -s --arg repository \"$REPOSITORY\" --arg commit \"$COMMIT\" --arg tag \"$TAG\" --arg target {triple} --arg asset {file} --rawfile rust {rustc} '{{schema: 1}} + capture({SIDECAR_PATTERN}) + {{tag: $tag, version: {version}, repository: $repository, commit: $commit, target: $target, asset: $asset, toolchain: {{rust: $rust[0:-1]}}}}' {sum} > {provenance}"
            );
            let in_folder = |step: RenderedStep| RenderedStep {
                working_directory: folder.clone(),
                ..step
            };
            [
                command(
                    format!("Name {bin} for {}", platform.file_suffix()),
                    format!("cp {release_dir}/{bin} {copy_to}"),
                ),
                in_folder(command(
                    format!("Checksum {name}"),
                    format!("{checksum} {file} > {sum}"),
                )),
                in_folder(command(
                    format!("Record the rustc version for {name}"),
                    format!("rustc --version > {rustc}"),
                )),
                RenderedStep {
                    env: variables(&[
                        ("REPOSITORY", "${{ github.repository }}"),
                        ("COMMIT", "${{ steps.source.outputs.commit }}"),
                        ("TAG", tag),
                    ]),
                    ..in_folder(command(format!("Record provenance of {name}"), record))
                },
            ]
        })
        .collect()
}

pub(crate) fn smoke_steps(binary: &BinaryPlan, platform: Platform, tag: &str) -> Vec<RenderedStep> {
    if !binary.smoke {
        return Vec::new();
    }
    let name = stem(binary, &binary.bin, platform);
    let executable = word(binary, &format!("./{name}"));
    let in_folder = |step: RenderedStep| RenderedStep {
        working_directory: versioned(binary).then(|| STAGE.to_owned()),
        ..step
    };
    vec![
        in_folder(command(
            format!("Run {name}"),
            format!("{executable} --version"),
        )),
        RenderedStep {
            env: variables(&[("TAG", tag)]),
            ..in_folder(command(
                format!("Check the tag against the version of {name}"),
                format!("{executable} check-tag \"$TAG\""),
            ))
        },
    ]
}
