//! Validation for closed, source-qualified distribution records.

use super::{
    DistributionAssetFormat, DistributionTool, QualifiedDistribution, QualifiedInstallBackend,
};
use crate::MiseError;

impl QualifiedDistribution {
    pub(super) fn validate(self) -> Result<Self, MiseError> {
        if let Some(path) = self.installed_binary_relative_path {
            validate_install_path(path)?;
        }
        validate_distribution_version(self.owner, self.version)?;
        validate_distribution_version(self.owner, self.selection_version)?;
        let container_valid = match self.asset_format {
            DistributionAssetFormat::Binary => {
                self.binary_member.is_empty() && self.archive_sha256 == self.binary_sha256
            }
            DistributionAssetFormat::TarGzip
            | DistributionAssetFormat::TarXz
            | DistributionAssetFormat::Zip => {
                validate_install_path(self.binary_member)?;
                match self.tool {
                    DistributionTool::Mise => self.binary_member == "mise/bin/mise",
                    DistributionTool::Mbx => self.binary_member == "mbx",
                    _ => true,
                }
            }
        };
        if !container_valid
            || !valid_hash(self.archive_sha256, 64)
            || !valid_hash(self.binary_sha256, 64)
            || !valid_hash(self.source_commit, 40)
            || !valid_hash(self.source_tree, 40)
            || self.abi.is_empty()
            || !self.valid_asset_origin()
            || !self.source_repository.starts_with("https://github.com/")
            || self.owner.is_empty()
            || self.selector.is_empty()
        {
            return Err(invalid("invalid distribution qualification"));
        }
        self.validate_launch_closure()?;
        self.validate_source_lineage()?;
        self.validate_install_plan()?;
        Ok(self)
    }

    fn valid_asset_origin(&self) -> bool {
        let prefixes: &[&str] = match self.tool {
            DistributionTool::ReleasePlz => {
                &["https://github.com/release-plz/release-plz/releases/download/"]
            }
            DistributionTool::CargoSemverChecks => {
                &["https://github.com/obi1kenobi/cargo-semver-checks/releases/download/"]
            }
            DistributionTool::Gh => &["https://github.com/cli/cli/releases/download/"],
            DistributionTool::Mise => &["https://github.com/jdx/mise/releases/download/"],
            DistributionTool::Mbx => &["https://github.com/jdx/mr-boxington/releases/download/"],
            DistributionTool::Bun => &["https://github.com/oven-sh/bun/releases/download/"],
            DistributionTool::Node => &["https://nodejs.org/dist/"],
            DistributionTool::OpenTofu => {
                &["https://github.com/opentofu/opentofu/releases/download/"]
            }
            DistributionTool::Python => {
                &["https://github.com/astral-sh/python-build-standalone/releases/download/"]
            }
            DistributionTool::Uv => &["https://github.com/astral-sh/uv/releases/download/"],
            DistributionTool::Java => {
                &["https://github.com/graalvm/graalvm-ce-builds/releases/download/"]
            }
            DistributionTool::Gradle => &[
                "https://services.gradle.org/distributions/",
                "https://downloads.gradle.org/distributions/",
                "https://github.com/gradle/gradle-distributions/releases/download/",
            ],
        };
        if prefixes
            .iter()
            .any(|prefix| self.asset_url.starts_with(prefix))
        {
            return true;
        }
        // Test fixtures never become a production accepted origin.
        #[cfg(test)]
        if self
            .asset_url
            .starts_with("https://github.com/velnor-test-fixtures/")
        {
            return true;
        }
        false
    }

    fn validate_launch_closure(&self) -> Result<(), MiseError> {
        let mut members = std::collections::BTreeSet::new();
        let mut installed = std::collections::BTreeSet::new();
        for entry in self.launch_entries {
            validate_install_path(entry.archive_member)?;
            if !valid_hash(entry.sha256, 64) || !members.insert(entry.archive_member) {
                return Err(invalid("invalid qualified launch closure"));
            }
            if let Some(path) = entry.installed_relative_path {
                validate_install_path(path)?;
                if !installed.insert(path) {
                    return Err(invalid("duplicate qualified launch path"));
                }
            }
        }
        if !matches!(self.tool, DistributionTool::Mise | DistributionTool::Mbx)
            && !self.launch_entries.iter().any(|entry| {
                entry.archive_member == self.binary_member && entry.sha256 == self.binary_sha256
            })
        {
            return Err(invalid(
                "qualified native archive is missing its primary launch bytes",
            ));
        }
        Ok(())
    }

    fn validate_source_lineage(&self) -> Result<(), MiseError> {
        let mut sources = std::collections::BTreeSet::new();
        for source in self.source_lineage {
            if source.name.is_empty()
                || !sources.insert(source.name)
                || !source.repository.starts_with("https://github.com/")
                || !valid_hash(source.commit, 40)
                || !valid_hash(source.tree, 40)
                || !exact_literal(source.version)
            {
                return Err(invalid("invalid qualified source lineage"));
            }
        }
        Ok(())
    }

    fn validate_install_plan(&self) -> Result<(), MiseError> {
        let Some(plan) = self.install_plan else {
            return Ok(());
        };
        validate_install_path(plan.root_relative_path)?;
        if !plan.bin_path.is_empty() {
            validate_install_path(plan.bin_path)?;
        }
        if plan.transform_abi.is_empty()
            || self.installed_binary_relative_path.is_none()
            || self.launch_entries.is_empty()
            || self
                .launch_entries
                .iter()
                .any(|entry| entry.installed_relative_path.is_none())
        {
            return Err(invalid("incomplete qualified native installation"));
        }
        let prefix = format!("{}/", plan.root_relative_path);
        if self.launch_entries.iter().any(|entry| {
            !entry
                .installed_relative_path
                .is_some_and(|path| path.starts_with(&prefix))
        }) || !self.launch_entries.iter().any(|entry| {
            entry.archive_member == self.binary_member
                && entry.sha256 == self.binary_sha256
                && entry.installed_relative_path == self.installed_binary_relative_path
        }) {
            return Err(invalid(
                "qualified native launch closure does not match installation",
            ));
        }
        if plan.backend == QualifiedInstallBackend::MiseHttp {
            self.validate_http_selector(plan)?;
        }
        let mut environment = std::collections::BTreeSet::new();
        for entry in plan.environment {
            if !matches!(entry.name, "JAVA_HOME" | "PYTHONHOME" | "PATH")
                || !environment.insert(entry.name)
                || entry.name == "JAVA_HOME" && self.tool != DistributionTool::Java
                || entry.name == "PYTHONHOME" && self.tool != DistributionTool::Python
            {
                return Err(invalid("unsupported qualified installer environment"));
            }
            if !entry.relative_path.is_empty() {
                validate_install_path(entry.relative_path)?;
            }
        }
        Ok(())
    }
    fn validate_http_selector(&self, plan: super::QualifiedInstallPlan) -> Result<(), MiseError> {
        let slot = match self.tool {
            DistributionTool::Gh => "gh",
            DistributionTool::ReleasePlz => "release-plz",
            DistributionTool::CargoSemverChecks => "cargo-semver-checks",
            DistributionTool::Node => "node",
            DistributionTool::Bun => "bun",
            DistributionTool::OpenTofu => "opentofu",
            DistributionTool::Python => "python",
            DistributionTool::Uv => "uv",
            DistributionTool::Java => "graalvm-community-jdk",
            DistributionTool::Gradle => "gradle",
            _ => return Err(invalid("unsupported native HTTP slot")),
        };
        if self.asset_url.bytes().any(|byte| {
            byte.is_ascii_whitespace() || byte.is_ascii_control() || matches!(byte, b'"' | b'\\')
        }) {
            return Err(invalid("invalid qualified HTTP asset literal"));
        }
        let bin = if plan.bin_path.is_empty() {
            String::new()
        } else {
            format!(",bin_path=\"{}\"", plan.bin_path)
        };
        let selector = format!(
            "http:{slot}[url=\"{}\",checksum=\"sha256:{}\",strip_components={}{bin}]@{}",
            self.asset_url, self.archive_sha256, plan.strip_components, self.selection_version
        );
        let root = format!("installs/http-{slot}/{}", self.selection_version);
        if self.selector != selector || plan.root_relative_path != root {
            return Err(invalid(
                "qualified HTTP selector does not match source artifact and layout",
            ));
        }
        Ok(())
    }
}

pub(super) fn validate_install_path(path: &str) -> Result<(), MiseError> {
    if path
        .bytes()
        .any(|byte| byte.is_ascii_control() || byte == b'\\')
        || path
            .split('/')
            .any(|part| part.is_empty() || matches!(part, "." | ".."))
    {
        return Err(invalid("invalid_qualified_installed_binary_path"));
    }
    Ok(())
}

pub(super) fn validate_distribution_version(owner: &str, version: &str) -> Result<(), MiseError> {
    let (release, build) = version
        .split_once('+')
        .map_or((version, None), |(release, build)| (release, Some(build)));
    let (core, prerelease) = release
        .split_once('-')
        .map_or((release, None), |(core, suffix)| (core, Some(suffix)));
    let parts: Vec<_> = core.split('.').collect();
    if parts.len() < 3
        || parts.iter().any(|part| {
            part.is_empty()
                || !part.bytes().all(|byte| byte.is_ascii_digit())
                || part.len() > 1 && part.starts_with('0')
        })
        || prerelease.is_some_and(|suffix| !valid_identifiers(suffix, true))
        || build.is_some_and(|suffix| !valid_identifiers(suffix, false))
    {
        return Err(super::super::versions::invalid_version(owner, version));
    }
    Ok(())
}

fn valid_identifiers(value: &str, reject_numeric_zero: bool) -> bool {
    value.split('.').all(|part| {
        !part.is_empty()
            && part
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            && !(reject_numeric_zero
                && part.bytes().all(|byte| byte.is_ascii_digit())
                && part.len() > 1
                && part.starts_with('0'))
    })
}

fn valid_hash(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        && !value.bytes().all(|byte| byte == b'0')
}

fn exact_literal(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().any(|byte| byte.is_ascii_digit())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'+'))
}

fn invalid(problem: &str) -> MiseError {
    MiseError::Contract {
        problem: problem.to_owned(),
    }
}
