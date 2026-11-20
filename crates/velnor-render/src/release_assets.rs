// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::Platform;
use velnor_release::{
    BinaryFormat, BinaryPlan, Checksum, asset_name_uses_version, expand_asset_name,
};

use crate::release_binaries::{command, target};
use crate::release_binary_sums::BINARY_SUMS_FILE;
use crate::release_raw::bin_names;
use crate::release_steps::{shell_word, variables};
use crate::rules::{RenderedStep, Value};

const VERSION_REFERENCE: &str = "${VERSION}";
const VERSION_STEP: &str = "version";
pub(crate) const STAGE: &str = "${{ runner.temp }}/stage";
pub(crate) const STAGE_SHELL: &str = "$RUNNER_TEMP/stage";

pub(crate) fn versioned(binary: &BinaryPlan) -> bool {
    binary
        .asset_name
        .as_deref()
        .is_some_and(asset_name_uses_version)
}

pub(crate) fn stem(binary: &BinaryPlan, bin: &str, platform: Platform) -> String {
    match &binary.asset_name {
        None => match binary.format {
            BinaryFormat::Archive => format!("{bin}-{}", target(platform, binary).1),
            BinaryFormat::Raw => format!("{bin}-{}", platform.file_suffix()),
        },
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
        BinaryFormat::Archive => {
            let archive = asset(binary, platform);
            let sidecar = format!("{archive}.sha256");
            if binary.checksum_files.is_some() {
                vec![archive, sidecar]
            } else {
                vec![archive]
            }
        }
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

pub(crate) fn word(binary: &BinaryPlan, name: &str) -> String {
    if versioned(binary) {
        format!("\"{name}\"")
    } else {
        shell_word(name)
    }
}

pub(crate) fn read_version(binary: &BinaryPlan, tag: &str) -> Option<RenderedStep> {
    versioned(binary).then(|| {
        let pattern = binary
            .tag_pattern
            .as_str()
            .replace("{{ package }}", &binary.package);
        let prefix = pattern.split("{{ version }}").next().unwrap_or_default();
        RenderedStep {
            id: Some(VERSION_STEP.to_owned()),
            env: variables(&[("TAG", tag)]),
            ..command(
                "Read the version of the tag".to_owned(),
                format!("echo \"version=${{TAG#{prefix}}}\" >> \"$GITHUB_OUTPUT\""),
            )
        }
    })
}

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

const REPRODUCIBLE: &str = "--sort=name --mtime=@0 --owner=0 --group=0 --numeric-owner ";
const MACOS_PACK_DIR: &str = "$RUNNER_TEMP/pack";
const MACOS_MTIME: &str = "197001010000";

fn linux_pack(binary: &BinaryPlan, file: &str, release_dir: &str) -> String {
    let members = contents(binary);
    if versioned(binary) {
        format!("tar {REPRODUCIBLE}-czf \"{STAGE_SHELL}/{file}\" -C {release_dir}{members}")
    } else {
        format!(
            "tar {REPRODUCIBLE}-czf {} -C {release_dir}{members}",
            shell_word(file)
        )
    }
}

fn macos_pack(binary: &BinaryPlan, file: &str, release_dir: &str) -> Vec<RenderedStep> {
    let bins = std::iter::once(binary.bin.as_str())
        .chain(binary.extra_bins.iter().map(|extra| extra.bin.as_str()));
    let mut sources: Vec<String> = bins
        .clone()
        .map(|bin| format!("{release_dir}/{bin}"))
        .collect();
    sources.extend(
        binary
            .include
            .iter()
            .map(|path| format!("\"$GITHUB_WORKSPACE/{path}\"")),
    );
    let mut names: Vec<String> = bins.map(str::to_owned).collect();
    names.extend(binary.include.iter().map(|path| {
        path.rsplit_once('/')
            .map_or(path.as_str(), |(_, name)| name)
            .to_owned()
    }));
    names.sort();
    let staged = names
        .iter()
        .map(|name| format!("\"{MACOS_PACK_DIR}/{name}\""))
        .collect::<Vec<_>>()
        .join(" ");
    let out = if versioned(binary) {
        format!("\"{STAGE_SHELL}/{file}\"")
    } else {
        shell_word(file)
    };
    vec![
        command(
            format!("Stage {file}"),
            format!("mkdir -p \"{MACOS_PACK_DIR}\""),
        ),
        command(
            format!("Copy {file} members"),
            format!("cp -p {} \"{MACOS_PACK_DIR}/\"", sources.join(" ")),
        ),
        command(
            format!("Normalise {file} mtimes"),
            format!("TZ=UTC touch -h -t {MACOS_MTIME} {staged}"),
        ),
        command(
            format!("Pack {file}"),
            format!(
                "COPYFILE_DISABLE=1 tar -cf \"{MACOS_PACK_DIR}.tar\" -n --uid 0 --gid 0 --uname '' --gname '' --no-xattrs --no-mac-metadata -C \"{MACOS_PACK_DIR}\" {}",
                names.join(" ")
            ),
        ),
        command(
            format!("Compress {file}"),
            format!("gzip -n -c \"{MACOS_PACK_DIR}.tar\" > {out}"),
        ),
    ]
}

pub(crate) fn archive_packaging(
    binary: &BinaryPlan,
    platform: Platform,
    file: &str,
    release_dir: &str,
) -> Vec<RenderedStep> {
    let mut steps = match platform {
        Platform::MacosArm64 => macos_pack(binary, file, release_dir),
        Platform::LinuxX64 | Platform::LinuxArm64 => vec![command(
            format!("Pack {file}"),
            linux_pack(binary, file, release_dir),
        )],
    };
    steps.extend(binary.checksum_files.map(|Checksum::Sha256| {
        let checksum = if platform == Platform::MacosArm64 {
            "shasum -a 256"
        } else {
            "sha256sum"
        };
        let name = word(binary, file);
        let sidecar = word(binary, &format!("{file}.sha256"));
        RenderedStep {
            working_directory: versioned(binary).then(|| STAGE.to_owned()),
            ..command(
                format!("Checksum {file}"),
                format!("{checksum} {name} > {sidecar}"),
            )
        }
    }));
    steps
}

pub(crate) fn uploaded(binary: &BinaryPlan, platform: Platform) -> String {
    if versioned(binary) && binary.format == BinaryFormat::Raw {
        format!("{STAGE}/*\n!{STAGE}/*.rustc")
    } else if versioned(binary) {
        format!("{STAGE}/*")
    } else {
        published_files(binary, platform).join("\n")
    }
}

pub(crate) const COMMIT_FILE: &str = "COMMIT";

pub(crate) fn assets(binary: &BinaryPlan) -> Vec<String> {
    binary
        .platforms
        .iter()
        .flat_map(|platform| published_files(binary, *platform))
        .chain(binary.commit_file.then(|| COMMIT_FILE.to_owned()))
        .chain(binary.member_digests.map(|_| BINARY_SUMS_FILE.to_owned()))
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
