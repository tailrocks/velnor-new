//! Effective release-plz configuration rendering (deterministic TOML).
//!
//! The normal policy (`release_always = false`, verification on, registry
//! publication) is emitted as literals: [`ReleasePlzConfig`] cannot express
//! the bootstrap exception. The bootstrap-only file needs the distinct
//! [`BootstrapReleasePlzConfig`] type, so the normal path can never emit it.

use crate::{
    RenderError, marker,
    release_spec::{is_clean_text, validate_package_name},
    steps::scan_for_private_subcommands,
};

/// One selected package in the effective config.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleasePlzPackage {
    /// Package name (validated Cargo charset).
    pub name: String,
    /// Features enabled for verification builds.
    pub publish_features: Vec<String>,
}

/// Effective normal-policy release-plz configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleasePlzConfig {
    /// Explicit package-qualified tag pattern.
    pub tag_pattern: String,
    /// Semver comparison gate (typed input, passed through).
    pub semver_check: bool,
    /// Selected packages in caller order.
    pub packages: Vec<ReleasePlzPackage>,
}

/// Bootstrap-only effective config (the single `release_always` file).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootstrapReleasePlzConfig {
    inner: ReleasePlzConfig,
}

impl BootstrapReleasePlzConfig {
    /// Wrap a validated normal config as the bootstrap-only variant.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError`] when the inner config is invalid.
    pub fn new(config: ReleasePlzConfig) -> Result<Self, RenderError> {
        config.validate()?;
        Ok(Self { inner: config })
    }

    /// Borrow the inner normal config.
    #[must_use]
    pub fn inner(&self) -> &ReleasePlzConfig {
        &self.inner
    }
}

/// Validate one feature name (`[A-Za-z0-9_+./-]`, 1..=128).
///
/// # Errors
///
/// Returns [`RenderError::InvalidWorkflow`] for malformed features.
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

/// Validate the tag pattern: both placeholders, no other template spans.
///
/// # Errors
///
/// Returns [`RenderError::InvalidWorkflow`] for malformed patterns.
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
    /// Validate the package name plus every feature name.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError::InvalidWorkflow`] for malformed packages.
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
    /// Validate the tag pattern plus the selected package set.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError`] for malformed configs.
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

/// TOML-escape one string scalar (quotes and backslashes).
fn toml_escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Render one TOML line; `String` writes are infallible in practice.
fn toml_line(out: &mut String, args: std::fmt::Arguments<'_>) -> Result<(), RenderError> {
    use std::fmt::Write as _;
    out.write_fmt(args)
        .map_err(|_| RenderError::InvalidWorkflow("toml_write".to_owned()))
}

/// Deterministic TOML body; only the bootstrap call passes `true`.
///
/// # Errors
///
/// Returns [`RenderError`] for malformed configs.
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

/// Render the effective normal-policy config with the marker line.
///
/// # Errors
///
/// Returns [`RenderError`] for invalid configs or versions.
pub fn render_release_plz_config(
    config: &ReleasePlzConfig,
    version: &str,
) -> Result<String, RenderError> {
    config.validate()?;
    let text = marker::with_marker(version, &toml_body(config, false)?)?;
    scan_for_private_subcommands(&text)?;
    Ok(text)
}

/// Render the bootstrap-only config with the marker line.
///
/// # Errors
///
/// Returns [`RenderError`] for invalid configs or versions.
pub fn render_bootstrap_release_plz_config(
    config: &BootstrapReleasePlzConfig,
    version: &str,
) -> Result<String, RenderError> {
    let text = marker::with_marker(version, &toml_body(config.inner(), true)?)?;
    scan_for_private_subcommands(&text)?;
    Ok(text)
}
