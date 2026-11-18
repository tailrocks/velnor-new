// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_release::BinaryPlan;

use crate::release_assets::{asset, word};
use crate::release_binaries::{DIST, command};
use crate::release_steps::expression;
use crate::rules::RenderedStep;

pub(crate) const BINARY_SUMS_FILE: &str = "BINARY-SHA256SUMS";
const SUMS_FILE: &str = "SHA256SUMS";
const UNPACKED: &str = "binary";

pub(crate) fn steps(binary: &BinaryPlan) -> Vec<RenderedStep> {
    if binary.member_digests.is_none() {
        return Vec::new();
    }
    let members = std::iter::once(binary.bin.as_str())
        .chain(binary.extra_bins.iter().map(|extra| extra.bin.as_str()))
        .collect::<Vec<_>>();
    let archives: Vec<String> = binary
        .platforms
        .iter()
        .map(|platform| asset(binary, *platform))
        .collect();
    let directories = archives
        .iter()
        .map(|archive| word(binary, &format!("{UNPACKED}/{archive}")))
        .collect::<Vec<_>>();
    let mut steps = vec![command(
        format!(
            "Make the folders to unpack the archives of {} in",
            binary.package
        ),
        format!("mkdir -p {}", directories.join(" ")),
    )];
    steps.extend(
        archives
            .iter()
            .zip(&directories)
            .map(|(archive, directory)| {
                command(
                    format!("Unpack {archive}"),
                    format!(
                        "tar -xzf {} -C {directory} {}",
                        word(binary, &format!("{DIST}/{archive}")),
                        members.join(" ")
                    ),
                )
            }),
    );
    let listed = archives
        .iter()
        .flat_map(|archive| {
            members
                .iter()
                .map(move |member| word(binary, &format!("{archive}/{member}")))
        })
        .collect::<Vec<_>>()
        .join(" ");
    steps.extend(
        [("sha256sum", false), ("shasum -a 256", true)].map(|(tool, on_macos)| {
            let comparison = if on_macos { "==" } else { "!=" };
            RenderedStep {
                working_directory: Some(UNPACKED.to_owned()),
                condition: Some(expression(&format!("runner.os {comparison} 'macOS'"))),
                ..command(
                    format!("Write {BINARY_SUMS_FILE}"),
                    format!("{tool} {listed} > ../{DIST}/{BINARY_SUMS_FILE}"),
                )
            }
        }),
    );
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
