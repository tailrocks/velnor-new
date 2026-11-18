// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::Platform;
use velnor_release::{BinaryFormat, BinaryPlan, asset_name_uses_version, expand_asset_name};

use crate::release_binaries::{command, target};
use crate::release_raw::bin_names;
use crate::release_steps::{shell_word, variables};
use crate::rules::{RenderedStep, Value};

const VERSION_REFERENCE: &str = "${VERSION}";
const VERSION_STEP: &str = "version";
pub(crate) const STAGE: &str = "stage";

pub(crate) fn versioned(binary: &BinaryPlan) -> bool {
    binary
        .asset_name
        .as_deref()
        .is_some_and(asset_name_uses_version)
}

pub(crate) fn stem(binary: &BinaryPlan, bin: &str, platform: Platform) -> String {
    match &binary.asset_name {
        None => format!("{bin}-{}", platform.file_suffix()),
        Some(template) => expand_asset_name(
            template,
            bin,
            platform.file_suffix(),
            target(platform, binary).1,
            VERSION_REFERENCE,
        ),
    }
}

pub(crate) fn asset(binary: &BinaryPlan, platform: Platform) -> String {
    let stem = stem(binary, &binary.bin, platform);
    match binary.format {
        BinaryFormat::Archive => format!("{stem}.tar.gz"),
        BinaryFormat::Raw => stem,
    }
}

pub(crate) fn published_files(binary: &BinaryPlan, platform: Platform) -> Vec<String> {
    match binary.format {
        BinaryFormat::Archive => vec![asset(binary, platform)],
        BinaryFormat::Raw => bin_names(binary)
            .flat_map(|bin| {
                let name = stem(binary, bin, platform);
                [
                    name.clone(),
                    format!("{name}.sha256"),
                    format!("{name}.provenance.json"),
                ]
            })
            .collect(),
    }
}

pub(crate) fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

pub(crate) fn file_names(binary: &BinaryPlan) -> impl Iterator<Item = String> + '_ {
    binary.files.iter().map(|path| file_name(path).to_owned())
}

pub(crate) fn carries_files(binary: &BinaryPlan, platform: Platform) -> bool {
    binary.platforms.first() == Some(&platform)
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
            id: Some(VERSION_STEP.to_owned()),
            env: variables(&[(
                "TAG",
                &format!("${{{{ needs.tag.outputs.{} }}}}", binary.package),
            )]),
            ..command(
                "Read the version of the tag".to_owned(),
                format!("echo \"version=${{TAG#{prefix}}}\" >> \"$GITHUB_OUTPUT\""),
            )
        }
    })
}

/// Hands the version to every step that names it, through that step's own `env`.
pub(crate) fn bind_version(steps: &mut [RenderedStep]) {
    for step in steps {
        let reads = step
            .run
            .as_deref()
            .is_some_and(|run| run.contains(VERSION_REFERENCE));
        if reads && step.env.iter().all(|(name, _)| name != "VERSION") {
            step.env.push((
                "VERSION".to_owned(),
                Value::Expr(format!("${{{{ steps.{VERSION_STEP}.outputs.version }}}}")),
            ));
        }
    }
}

pub(crate) fn pack(binary: &BinaryPlan, file: &str, release_dir: &str) -> String {
    let members = contents(binary);
    if versioned(binary) {
        format!("tar -czf \"{STAGE}/{file}\" -C {release_dir}{members}")
    } else {
        format!("tar -czf {} -C {release_dir}{members}", shell_word(file))
    }
}

pub(crate) fn uploaded(binary: &BinaryPlan, platform: Platform) -> String {
    if versioned(binary) && binary.format == BinaryFormat::Raw {
        format!("{STAGE}/*\n!{STAGE}/*.rustc")
    } else if versioned(binary) {
        format!("{STAGE}/*")
    } else {
        let mut names = published_files(binary, platform);
        if carries_files(binary, platform) {
            names.extend(file_names(binary));
        }
        names.join("\n")
    }
}

pub(crate) const COMMIT_FILE: &str = "COMMIT";

pub(crate) fn assets(binary: &BinaryPlan) -> Vec<String> {
    binary
        .platforms
        .iter()
        .flat_map(|platform| published_files(binary, *platform))
        .chain(file_names(binary))
        .chain(binary.commit_file.then(|| COMMIT_FILE.to_owned()))
        .collect()
}

pub(crate) fn quoted(binary: &BinaryPlan, names: &[String]) -> Vec<String> {
    names.iter().map(|name| word(binary, name)).collect()
}

pub(crate) fn selection(binary: &BinaryPlan) -> String {
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

pub(crate) fn contents(binary: &BinaryPlan) -> String {
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
