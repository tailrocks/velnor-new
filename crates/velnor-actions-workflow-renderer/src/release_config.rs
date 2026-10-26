// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{
    RenderError, marker,
    release_spec::{is_clean_text, validate_package_name},
    steps::scan_for_private_subcommands,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleasePlzPackage {
    pub name: String,
    pub publish_features: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleasePlzConfig {
    pub tag_pattern: String,
    pub semver_check: bool,
    pub packages: Vec<ReleasePlzPackage>,
}

pub fn stable_release_pr_config(version_package: &str) -> Result<String, RenderError> {
    validate_package_name(version_package)?;
    Ok(format!(
        "[workspace]\nrelease = false\ngit_only = true\npublish = false\ngit_tag_enable = false\ngit_release_enable = false\nsemver_check = false\n\n[[package]]\nname = \"{version_package}\"\nrelease = true\n"
    ))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootstrapReleasePlzConfig {
    inner: ReleasePlzConfig,
}

impl BootstrapReleasePlzConfig {
    pub fn new(config: ReleasePlzConfig) -> Result<Self, RenderError> {
        config.validate()?;
        Ok(Self { inner: config })
    }

    #[must_use]
    pub fn inner(&self) -> &ReleasePlzConfig {
        &self.inner
    }
}

pub fn validate_feature_name(feature: &str) -> Result<(), RenderError> {
    let charset =
        |b: u8| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'+' | b'.' | b'/' | b'-');
    if is_clean_text(feature, 128) && feature.bytes().all(charset) {
        Ok(())
    } else {
        Err(RenderError::InvalidWorkflow(format!(
            "bad_feature:{feature}"
        )))
    }
}

pub fn validate_tag_pattern(pattern: &str) -> Result<(), RenderError> {
    let invalid = RenderError::InvalidWorkflow(format!("bad_tag_pattern:{pattern}"));
    if !is_clean_text(pattern, 256) || pattern.contains(['"', '\\']) {
        return Err(invalid);
    }
    let mut seen_package = false;
    let mut seen_version = false;
    let mut rest = pattern;
    while let Some(start) = rest.find("{{") {
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else {
            return Err(invalid);
        };
        match after[..end].trim() {
            "package" => seen_package = true,
            "version" => seen_version = true,
            _ => return Err(invalid),
        }
        rest = &after[end + 2..];
    }
    if rest.contains("}}") || !(seen_package && seen_version) {
        return Err(invalid);
    }
    Ok(())
}

impl ReleasePlzPackage {
    pub fn validate(&self) -> Result<(), RenderError> {
        validate_package_name(&self.name)?;
        if self.publish_features.len() > 64 {
            return Err(RenderError::InvalidWorkflow(format!(
                "bad_feature:{}:too_many",
                self.name
            )));
        }
        for feature in &self.publish_features {
            validate_feature_name(feature)?;
        }
        Ok(())
    }
}

impl ReleasePlzConfig {
    pub fn validate(&self) -> Result<(), RenderError> {
        validate_tag_pattern(&self.tag_pattern)?;
        scan_for_private_subcommands(&self.tag_pattern)?;
        if self.packages.is_empty() || self.packages.len() > 64 {
            return Err(RenderError::InvalidWorkflow(
                "no_release_packages".to_owned(),
            ));
        }
        let mut names = std::collections::BTreeSet::new();
        for package in &self.packages {
            package.validate()?;
            if !names.insert(package.name.clone()) {
                return Err(RenderError::InvalidWorkflow(format!(
                    "duplicate_package:{}",
                    package.name
                )));
            }
        }
        Ok(())
    }
}

fn toml_escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn toml_line(out: &mut String, args: std::fmt::Arguments<'_>) -> Result<(), RenderError> {
    use std::fmt::Write as _;
    out.write_fmt(args)
        .map_err(|_| RenderError::InvalidWorkflow("toml_write".to_owned()))
}

fn toml_body(config: &ReleasePlzConfig, release_always: bool) -> Result<String, RenderError> {
    let mut out = String::from("[workspace]\nrelease = false\n");
    toml_line(
        &mut out,
        format_args!("release_always = {release_always}\n"),
    )?;
    toml_line(
        &mut out,
        format_args!("semver_check = {}\n", config.semver_check),
    )?;
    out.push_str("publish_no_verify = false\npublish_allow_dirty = false\n");
    toml_line(
        &mut out,
        format_args!("git_tag_name = \"{}\"\n", toml_escape(&config.tag_pattern)),
    )?;
    for package in &config.packages {
        out.push_str("\n[[package]]\n");
        toml_line(
            &mut out,
            format_args!("name = \"{}\"\n", toml_escape(&package.name)),
        )?;
        out.push_str("release = true\npublish = true\ngit_only = false\n");
        if !package.publish_features.is_empty() {
            let features: Vec<String> = package
                .publish_features
                .iter()
                .map(|feature| format!("\"{}\"", toml_escape(feature)))
                .collect();
            toml_line(
                &mut out,
                format_args!("publish_features = [{}]\n", features.join(", ")),
            )?;
        }
    }
    Ok(out)
}

pub fn render_release_plz_config(
    config: &ReleasePlzConfig,
    version: &str,
) -> Result<String, RenderError> {
    config.validate()?;
    let text = marker::with_marker(version, &toml_body(config, false)?)?;
    scan_for_private_subcommands(&text)?;
    Ok(text)
}

pub fn render_bootstrap_release_plz_config(
    config: &BootstrapReleasePlzConfig,
    version: &str,
) -> Result<String, RenderError> {
    let text = marker::with_marker(version, &toml_body(config.inner(), true)?)?;
    scan_for_private_subcommands(&text)?;
    Ok(text)
}

fn git_tag_toml_body(config: &ReleasePlzConfig) -> Result<String, RenderError> {
    let mut out = String::from("[workspace]\nrelease = false\nrelease_always = true\n");
    toml_line(
        &mut out,
        format_args!("semver_check = {}\n", config.semver_check),
    )?;
    out.push_str(
        "publish = false\ngit_only = true\ngit_tag_enable = true\ngit_release_enable = true\n",
    );
    toml_line(
        &mut out,
        format_args!("git_tag_name = \"{}\"\n", toml_escape(&config.tag_pattern)),
    )?;
    for package in &config.packages {
        out.push_str("\n[[package]]\n");
        toml_line(
            &mut out,
            format_args!("name = \"{}\"\n", toml_escape(&package.name)),
        )?;
        out.push_str("release = true\n");
    }
    Ok(out)
}

pub fn render_git_tag_release_plz_config(
    config: &ReleasePlzConfig,
    version: &str,
) -> Result<String, RenderError> {
    config.validate()?;
    if let Some(package) = config
        .packages
        .iter()
        .find(|package| !package.publish_features.is_empty())
    {
        return Err(RenderError::InvalidWorkflow(format!(
            "git_tag_publish_features:{}",
            package.name
        )));
    }
    let text = marker::with_marker(version, &git_tag_toml_body(config)?)?;
    scan_for_private_subcommands(&text)?;
    Ok(text)
}
