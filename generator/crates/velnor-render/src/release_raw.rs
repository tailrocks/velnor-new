// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::Platform;
use velnor_release::BinaryPlan;

use crate::release_binaries::command;
use crate::release_steps::variables;
use crate::rules::RenderedStep;

pub(crate) fn raw_name(bin: &str, platform: Platform) -> String {
    format!("{bin}-{}", platform.file_suffix())
}

pub(crate) fn bin_names(binary: &BinaryPlan) -> impl Iterator<Item = &str> {
    std::iter::once(binary.bin.as_str())
        .chain(binary.extra_bins.iter().map(|extra| extra.bin.as_str()))
}

pub(crate) fn raw_packaging(
    binary: &BinaryPlan,
    platform: Platform,
    triple: &str,
    release_dir: &str,
) -> Vec<RenderedStep> {
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
    bin_names(binary)
        .flat_map(|bin| {
            let name = raw_name(bin, platform);
            let record = format!(
                "jq -e -R -s --arg repository \"$REPOSITORY\" --arg commit \"$COMMIT\" --arg tag \"$TAG\" --arg target {triple} --arg asset {name} --rawfile rust {name}.rustc '{{schema: 1}} + capture(\"\\\\A(?<sha256>[0-9a-f]{{64}})  {name}\\\\n\\\\z\") + {{tag: $tag, version: {version}, repository: $repository, commit: $commit, target: $target, asset: $asset, toolchain: {{rust: $rust[0:-1]}}}}' {name}.sha256 > {name}.provenance.json"
            );
            [
                command(
                    format!("Name {bin} for {}", platform.file_suffix()),
                    format!("cp {release_dir}/{bin} {name}"),
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
        })
        .collect()
}
