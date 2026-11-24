// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_release::BinaryPlan;

use crate::release_assets::{asset, word};
use crate::release_binaries::{DIST, command};
use crate::release_optional::{is_optional, when_present};
use crate::release_steps::expression;
use crate::rules::RenderedStep;

pub(crate) const BINARY_SUMS_FILE: &str = "BINARY-SHA256SUMS";
const SUMS_FILE: &str = "SHA256SUMS";
const UNPACKED: &str = "binary";

pub(crate) fn steps(binary: &BinaryPlan) -> Vec<RenderedStep> {
    if binary.member_digests.is_none() {
        return Vec::new();
    }
    digests(
        &[binary],
        BINARY_SUMS_FILE,
        &format!(" of {}", binary.package),
    )
}

pub(crate) fn digests(binaries: &[&BinaryPlan], file: &str, of: &str) -> Vec<RenderedStep> {
    let members = |binary: &BinaryPlan| {
        std::iter::once(binary.bin.clone())
            .chain(binary.extra_bins.iter().map(|extra| extra.bin.clone()))
            .collect::<Vec<_>>()
    };
    let archives = |binary: &BinaryPlan| -> Vec<String> {
        binary
            .platforms
            .iter()
            .map(|platform| asset(binary, *platform))
            .collect()
    };
    let optional = |binary: &BinaryPlan, archive: &str| {
        binary
            .platforms
            .iter()
            .any(|platform| is_optional(binary, *platform) && asset(binary, *platform) == archive)
    };
    let directories: Vec<String> = binaries
        .iter()
        .flat_map(|binary| {
            archives(binary)
                .into_iter()
                .map(|archive| word(binary, &format!("{UNPACKED}/{archive}")))
        })
        .collect();
    let mut steps = vec![command(
        format!("Make the folders to unpack the archives{of} in"),
        format!("mkdir -p {}", directories.join(" ")),
    )];
    for binary in binaries {
        for archive in archives(binary) {
            let directory = word(binary, &format!("{UNPACKED}/{archive}"));
            steps.push(RenderedStep {
                condition: optional(binary, &archive).then(|| when_present(&archive, None)),
                ..command(
                    format!("Unpack {archive}"),
                    format!(
                        "tar -xzf {} -C {directory} {}",
                        word(binary, &format!("{DIST}/{archive}")),
                        members(binary).join(" ")
                    ),
                )
            });
        }
    }
    let listed = |optional_archives: bool| {
        binaries
            .iter()
            .flat_map(|binary| {
                archives(binary)
                    .into_iter()
                    .filter(move |archive| optional(binary, archive) == optional_archives)
                    .flat_map(move |archive| {
                        members(binary)
                            .into_iter()
                            .map(move |member| word(binary, &format!("{archive}/{member}")))
                    })
            })
            .collect::<Vec<_>>()
    };
    let tools = [("sha256sum", false), ("shasum -a 256", true)];
    let required = listed(false).join(" ");
    steps.extend(tools.map(|(tool, on_macos)| {
        let comparison = if on_macos { "==" } else { "!=" };
        RenderedStep {
            working_directory: Some(UNPACKED.to_owned()),
            condition: Some(expression(&format!("runner.os {comparison} 'macOS'"))),
            ..command(
                format!("Write {file}"),
                format!("{tool} {required} > ../{DIST}/{file}"),
            )
        }
    }));
    for binary in binaries {
        for archive in archives(binary).into_iter().filter(|a| optional(binary, a)) {
            let names = members(binary)
                .into_iter()
                .map(|member| word(binary, &format!("{archive}/{member}")))
                .collect::<Vec<_>>()
                .join(" ");
            for (tool, on_macos) in tools {
                let comparison = if on_macos { "==" } else { "!=" };
                steps.push(RenderedStep {
                    working_directory: Some(UNPACKED.to_owned()),
                    condition: Some(when_present(
                        &archive,
                        Some(&format!("runner.os {comparison} 'macOS'")),
                    )),
                    ..command(
                        format!("Add {archive} to {file}"),
                        format!("{tool} {names} >> ../{DIST}/{file}"),
                    )
                });
            }
        }
    }
    steps
}

pub(crate) fn upload_paths(binary: &BinaryPlan, extra: &[&str]) -> String {
    let mut names = vec![SUMS_FILE];
    names.extend(binary.member_digests.map(|_| BINARY_SUMS_FILE));
    names.extend(extra);
    names
        .iter()
        .map(|name| format!("{DIST}/{name}"))
        .collect::<Vec<_>>()
        .join("\n")
}
