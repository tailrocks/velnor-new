// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::Platform;
use velnor_release::{BinaryPlan, asset_name_uses_version, expand_asset_name};

use crate::release_binaries::{command, target};
use crate::release_steps::{shell_word, variables};
use crate::rules::RenderedStep;

const VERSION_REFERENCE: &str = "${VERSION}";
pub(crate) const STAGE: &str = "stage";

pub(crate) fn versioned(binary: &BinaryPlan) -> bool {
    binary
        .asset_name
        .as_deref()
        .is_some_and(asset_name_uses_version)
}

pub(crate) fn asset(binary: &BinaryPlan, platform: Platform) -> String {
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

pub(crate) fn word(binary: &BinaryPlan, name: &str) -> String {
    if versioned(binary) {
        format!("\"{name}\"")
    } else {
        shell_word(name)
    }
}

pub(crate) fn read_version(binary: &BinaryPlan) -> Option<RenderedStep> {
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

pub(crate) fn pack(binary: &BinaryPlan, file: &str, release_dir: &str) -> String {
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

pub(crate) fn uploaded(binary: &BinaryPlan, file: &str) -> String {
    if versioned(binary) {
        format!("{STAGE}/*")
    } else {
        file.to_owned()
    }
}

pub(crate) fn assets(binary: &BinaryPlan) -> Vec<String> {
    binary
        .platforms
        .iter()
        .map(|platform| asset(binary, *platform))
        .collect()
}

pub(crate) fn quoted(binary: &BinaryPlan, names: &[String]) -> Vec<String> {
    names.iter().map(|name| word(binary, name)).collect()
}
