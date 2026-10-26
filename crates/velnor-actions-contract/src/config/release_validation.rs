// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::errors::ContractError;
use std::collections::{BTreeMap, BTreeSet};

use super::release::BootstrapRelease;

pub(super) fn validate_manifest_path(file: &str, path: &str) -> Result<(), ContractError> {
    let key = "stacks.rust.release.manifest_path";
    if path.is_empty() {
        return Err(ContractError::config(file, key, "empty_manifest_path"));
    }
    if !path.bytes().all(is_manifest_byte) {
        return Err(ContractError::config(
            file,
            key,
            format!("bad_charset:{path}"),
        ));
    }
    if path.starts_with('/') || path.split('/').any(|seg| seg.is_empty() || seg == "..") {
        return Err(ContractError::config(file, key, "non_relative_manifest"));
    }
    if path != "Cargo.toml" && !path.ends_with("/Cargo.toml") {
        return Err(ContractError::config(file, key, "missing_cargo_toml"));
    }
    Ok(())
}

pub(super) fn validate_packages(
    file: &str,
    packages: &[String],
    enabled: bool,
    publishable: bool,
) -> Result<(), ContractError> {
    let key = "stacks.rust.release.packages";
    if publishable && !packages.is_empty() {
        return Err(ContractError::config(
            file,
            "stacks.rust.release.publishable_workspace",
            "packages_with_publishable",
        ));
    }
    if enabled && packages.is_empty() && !publishable {
        return Err(ContractError::config(file, key, "empty_packages"));
    }
    let mut sorted = packages.to_vec();
    sorted.sort();
    if sorted.as_slice() != packages {
        return Err(ContractError::config(file, key, "must_be_sorted"));
    }
    let unique: BTreeSet<&str> = packages.iter().map(String::as_str).collect();
    if unique.len() != packages.len() {
        return Err(ContractError::config(file, key, "duplicate_package"));
    }
    for name in packages {
        if !is_package_name(name) {
            return Err(ContractError::config(
                file,
                key,
                format!("unsafe_package:{name}"),
            ));
        }
    }
    Ok(())
}

pub(super) fn validate_environment(file: &str, name: &str) -> Result<(), ContractError> {
    let key = "stacks.rust.release.environment";
    if name.is_empty() {
        return Err(ContractError::config(file, key, "empty_environment"));
    }
    if name != name.trim() {
        return Err(ContractError::config(file, key, "padded_environment"));
    }
    if !name.bytes().all(is_environment_byte)
        || name.split('/').any(|seg| seg.is_empty() || seg == "..")
    {
        return Err(ContractError::config(
            file,
            key,
            format!("bad_environment:{name}"),
        ));
    }
    Ok(())
}

pub(super) fn validate_tag_name(file: &str, tag: &str) -> Result<(), ContractError> {
    let key = "stacks.rust.release.tag_name";
    if tag.is_empty() || !tag.bytes().all(is_tag_byte) {
        return Err(ContractError::config(
            file,
            key,
            format!("bad_charset:{tag}"),
        ));
    }
    if is_stable_tag_name(tag) {
        return Ok(());
    }
    if tag.matches("{{").count() != tag.matches("}}").count() {
        return Err(ContractError::config(file, key, "unbalanced_braces"));
    }
    let compact: String = tag.chars().filter(|c| !c.is_whitespace()).collect();
    if !compact.contains("{{package}}") {
        return Err(ContractError::config(file, key, "missing_package_var"));
    }
    if !compact.contains("{{version}}") {
        return Err(ContractError::config(file, key, "missing_version_var"));
    }
    Ok(())
}

pub(super) fn is_stable_tag_name(tag: &str) -> bool {
    tag.strip_prefix("v{{")
        .and_then(|body| body.strip_suffix("}}"))
        .is_some_and(|body| body.trim_matches(' ') == "version")
}

pub(super) fn validate_bootstrap(
    file: &str,
    bootstrap: &BootstrapRelease,
) -> Result<(), ContractError> {
    if !is_package_name(&bootstrap.package) {
        let package = bootstrap.package.as_str();
        return Err(ContractError::config(
            file,
            "stacks.rust.release.bootstrap.package",
            format!("unsafe_package:{package}"),
        ));
    }
    if !is_bootstrap_version(&bootstrap.version) {
        let version = bootstrap.version.as_str();
        return Err(ContractError::config(
            file,
            "stacks.rust.release.bootstrap.version",
            format!("bad_version:{version}"),
        ));
    }
    if !is_full_sha(&bootstrap.source_sha) {
        return Err(ContractError::config(
            file,
            "stacks.rust.release.bootstrap.source_sha",
            "bad_source_sha",
        ));
    }
    Ok(())
}

pub(super) fn validate_version_groups(
    file: &str,
    groups: &BTreeMap<String, Vec<String>>,
    packages: &[String],
    publishable: bool,
) -> Result<(), ContractError> {
    let allowed: BTreeSet<&str> = packages.iter().map(String::as_str).collect();
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    for (group, members) in groups {
        let key = format!("stacks.rust.release.version_groups.{group}");
        if !is_package_name(group) {
            return Err(ContractError::config(
                file,
                "stacks.rust.release.version_groups",
                format!("unsafe_group:{group}"),
            ));
        }
        if members.is_empty() {
            return Err(ContractError::config(file, key, "empty_group"));
        }
        let mut sorted = members.clone();
        sorted.sort();
        if sorted != *members {
            return Err(ContractError::config(file, key, "must_be_sorted"));
        }
        for member in members {
            if !is_package_name(member) {
                return Err(ContractError::config(
                    file,
                    key,
                    format!("unsafe_package:{member}"),
                ));
            }
            if !publishable && !allowed.contains(member.as_str()) {
                return Err(ContractError::config(
                    file,
                    key,
                    format!("unknown_package:{member}"),
                ));
            }
            if !seen.insert(member.as_str()) {
                return Err(ContractError::config(
                    file,
                    key,
                    format!("member_in_two_groups:{member}"),
                ));
            }
        }
        let unique: BTreeSet<&str> = members.iter().map(String::as_str).collect();
        if unique.len() != members.len() {
            return Err(ContractError::config(file, key, "duplicate_member"));
        }
    }
    Ok(())
}

pub(super) fn is_package_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == b'_')
        && bytes.all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
}

pub(super) fn is_bootstrap_version(version: &str) -> bool {
    let parts: Vec<&str> = version.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
}

pub(super) fn is_full_sha(sha: &str) -> bool {
    sha.len() == 40
        && sha
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

pub(super) fn is_manifest_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'.' | b'-' | b'_')
}

pub(super) fn is_environment_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'/' | b'.')
}

pub(super) fn is_tag_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b' ' | b'-' | b'_' | b'.' | b'/' | b'{' | b'}')
}
