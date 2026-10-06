//! Strict upstream URL admission for the supported backend families.
use super::{safe_name, sha256};
use crate::config::{
    QualifiedCargoInstallation, QualifiedTool, QualifiedToolArtifact, QualifiedToolBackend,
    QualifiedToolOptions, QualifiedToolPlatform,
};
use crate::errors::ContractError;
use std::collections::BTreeSet;

pub(super) fn safe_package(value: &str, optional_tool: bool) -> bool {
    let parts: Vec<_> = value.split('/').collect();
    (parts.len() == 2 || (optional_tool && parts.len() == 3))
        && parts
            .iter()
            .all(|part| safe_name(part) && *part != "." && *part != "..")
}

fn url_parts(url: &str) -> Option<(&str, &str)> {
    let rest = url.strip_prefix("https://")?;
    let (host, path) = rest.split_once('/')?;
    if !host
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'))
        || !path.split('/').all(|segment| {
            !segment.is_empty()
                && segment != "."
                && segment != ".."
                && segment.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_' | b'+')
                })
        })
    {
        return None;
    }
    Some((host, path))
}

fn github_release(path: &str, repository: &str, version: &str) -> bool {
    let Some(rest) = path.strip_prefix(&format!("{repository}/releases/download/")) else {
        return false;
    };
    let Some((tag, filename)) = rest.split_once('/') else {
        return false;
    };
    !filename.contains('/')
        && !tag.contains("latest")
        && (tag == version
            || tag == format!("v{version}")
            || tag.ends_with(&format!("-{version}"))
            || tag.ends_with(&format!("-v{version}")))
}

fn exact_crate_version(value: &str) -> bool {
    let core = value.split(['-', '+']).next().unwrap_or_default();
    let components: Vec<_> = core.split('.').collect();
    components.len() == 3
        && components
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'+'))
        && value
            .bytes()
            .last()
            .is_some_and(|byte| byte.is_ascii_alphanumeric())
}

fn crate_archive(host: &str, path: &str) -> Option<(String, String)> {
    let parts: Vec<_> = path.split('/').collect();
    match (host, parts.as_slice()) {
        ("crates.io", ["api", "v1", "crates", name, version, "download"])
            if safe_name(name) && exact_crate_version(version) =>
        {
            Some(((*name).to_owned(), (*version).to_owned()))
        }
        ("static.crates.io", ["crates", name, filename]) if safe_name(name) => {
            let version = filename
                .strip_prefix(&format!("{name}-"))?
                .strip_suffix(".crate")?;
            exact_crate_version(version).then(|| ((*name).to_owned(), version.to_owned()))
        }
        _ => None,
    }
}

fn source_allowed(
    tool: &QualifiedTool,
    artifact: &QualifiedToolArtifact,
    dependency: bool,
) -> bool {
    let Some((host, path)) = url_parts(&artifact.url) else {
        return false;
    };
    if dependency {
        return matches!(
            tool.options,
            QualifiedToolOptions::Cargo {
                installation: QualifiedCargoInstallation::Source { .. },
                ..
            }
        ) && crate_archive(host, path).is_some();
    }
    match &tool.backend {
        QualifiedToolBackend::Core { tool: name } if name == "rust" => {
            host == "static.rust-lang.org"
                && path.starts_with("dist/")
                && path.contains(&format!("-{}-", tool.version))
                && (path.strip_suffix(".tar.xz").is_some()
                    || path.strip_suffix(".tar.gz").is_some())
        }
        QualifiedToolBackend::Core { tool: name } if name == "node" => {
            host == "nodejs.org"
                && path.starts_with(&format!("dist/v{}/node-v{}-", tool.version, tool.version))
                && (path.strip_suffix(".tar.xz").is_some()
                    || path.strip_suffix(".tar.gz").is_some())
        }
        QualifiedToolBackend::Core { tool: name } if name == "bun" => {
            host == "github.com" && github_release(path, "oven-sh/bun", &tool.version)
        }
        QualifiedToolBackend::Aqua { package } => {
            let repository = package.split('/').take(2).collect::<Vec<_>>().join("/");
            host == "github.com" && github_release(path, &repository, &tool.version)
        }
        QualifiedToolBackend::Cargo { crate_name } => match &tool.options {
            QualifiedToolOptions::Cargo {
                installation: QualifiedCargoInstallation::Source { .. },
                ..
            } => crate_archive(host, path)
                .is_some_and(|(name, version)| name == *crate_name && version == tool.version),
            QualifiedToolOptions::Cargo {
                installation: QualifiedCargoInstallation::Prebuilt { repository },
                ..
            } => host == "github.com" && github_release(path, repository, &tool.version),
            _ => false,
        },
        QualifiedToolBackend::Core { .. } => false,
    }
}

pub(super) fn validate_platform_sources(
    tool: &QualifiedTool,
    platform: &QualifiedToolPlatform,
    file: &str,
    key: &str,
) -> Result<(), ContractError> {
    let bad = |problem| ContractError::config(file, key, problem);
    if platform.artifacts.is_empty() {
        return Err(bad("missing_qualified_tool_artifacts"));
    }
    let rust_components =
        matches!(&tool.backend, QualifiedToolBackend::Core { tool } if tool == "rust");
    if !rust_components && platform.artifacts.len() != 1 {
        return Err(bad("qualified_tool_requires_single_root_artifact"));
    }

    let mut urls = BTreeSet::new();
    for (artifacts, dependency) in [
        (&platform.artifacts, false),
        (&platform.dependency_artifacts, true),
    ] {
        if !artifacts.windows(2).all(|pair| pair[0].url < pair[1].url) {
            return Err(bad("qualified_artifacts_must_be_sorted_unique"));
        }
        for artifact in artifacts {
            if !sha256(&artifact.sha256)
                || !urls.insert(&artifact.url)
                || !source_allowed(tool, artifact, dependency)
            {
                return Err(bad("invalid_qualified_artifact_source"));
            }
        }
    }
    Ok(())
}
