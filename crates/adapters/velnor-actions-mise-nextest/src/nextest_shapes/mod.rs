//! Archive, inventory, and execution shapes for pinned Nextest runs.
//!
//! The three [`crate::nextest::NextestDriver`] shapes share one resolved
//! profile and the validated archive path; routing, partitions, and
//! payload helpers stay in [`crate::nextest`].

use std::ffi::OsString;

use velnor_actions_contract_config::is_valid_feature_name;

use crate::nextest::{
    ARCHIVE_FILE, CARGO_PROFILE, NEXTEST_EXTRACT_BASE, NEXTEST_PROFILE, NextestDriver,
    NextestPartition, argv_of, check_token, fixed_payload, nextest_exec,
};
use velnor_actions_mise_catalog::catalog::ToolCatalog;
use velnor_actions_mise_core::command::IsolatedCommand;
use velnor_actions_mise_core::error::MiseError;

/// Archive creation for one exact package/configuration.
///
/// Runs once per package/configuration; appends sorted `--features` and an
/// explicit `--target` (omitted for host builds).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NextestArchive {
    /// Compile-driver prefix.
    driver: NextestDriver,
    /// Archived package name.
    package: String,
    /// Sorted enabled features.
    features: Vec<String>,
    /// Explicit target triple; none omits `--target`.
    target: Option<String>,
    /// Resolved Nextest profile passed to `--profile`.
    profile: String,
}

impl NextestArchive {
    /// Build an archive request; features are sorted inside.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::InvalidNextestInput`] for an empty or malformed
    /// package, feature, or target value.
    pub fn new(
        driver: NextestDriver,
        package: &str,
        features: &[String],
        target: Option<&str>,
    ) -> Result<Self, MiseError> {
        Self::with_profile(driver, package, features, target, NEXTEST_PROFILE)
    }

    /// Build an archive request with a resolved Nextest profile.
    ///
    /// The profile is the detected selection (`ci` when the project
    /// declares `[profile.ci]`, else Nextest's documented `default`).
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::InvalidNextestInput`] for an empty or malformed
    /// package, feature, target, or profile value. Feature values use the
    /// contract feature-name charset, so config-accepted weak-dependency
    /// syntax (`dep?/feat`, `dep:feat`) survives archive construction.
    pub fn with_profile(
        driver: NextestDriver,
        package: &str,
        features: &[String],
        target: Option<&str>,
        profile: &str,
    ) -> Result<Self, MiseError> {
        check_token("package", package)?;
        let mut sorted = features.to_vec();
        sorted.sort();
        for feature in &sorted {
            if !is_valid_feature_name(feature) {
                return Err(MiseError::InvalidNextestInput {
                    field: "feature".to_owned(),
                    value: feature.to_owned(),
                });
            }
        }
        let triple = target
            .map(|value| check_token("target", value).map(|()| value.to_owned()))
            .transpose()?;
        check_token("profile", profile)?;
        Ok(Self {
            driver,
            package: package.to_owned(),
            features: sorted,
            target: triple,
            profile: profile.to_owned(),
        })
    }

    /// Resolved Nextest profile passed to `--profile`.
    #[must_use]
    pub fn profile(&self) -> &str {
        &self.profile
    }

    /// Configuration key proving archive-once: package, target, features.
    ///
    /// The driver is deliberately excluded: both drivers share one
    /// archive file, so either driver reserves the configuration.
    #[must_use]
    pub fn config_key(&self) -> (String, Option<String>, Vec<String>) {
        (
            self.package.clone(),
            self.target.clone(),
            self.features.clone(),
        )
    }

    /// Payload arguments: program first, then its fixed arguments.
    #[must_use]
    pub fn payload(&self) -> Vec<OsString> {
        let mut payload = fixed_payload(
            self.driver,
            &[
                "nextest",
                "archive",
                "--package",
                self.package.as_str(),
                "--profile",
                self.profile.as_str(),
                "--cargo-profile",
                CARGO_PROFILE,
                "--locked",
                "--archive-file",
                ARCHIVE_FILE,
            ],
        );
        if !self.features.is_empty() {
            payload.push(OsString::from("--features"));
            payload.push(OsString::from(self.features.join(",")));
        }
        if let Some(target) = &self.target {
            payload.push(OsString::from("--target"));
            payload.push(OsString::from(target));
        }
        payload
    }

    /// Full mise argument vector including the program.
    #[must_use]
    pub fn argv(&self, catalog: &ToolCatalog) -> Vec<OsString> {
        argv_of(catalog, self.driver, &self.payload())
    }

    /// Isolated command running this archive creation.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::EmptyCommand`] only if the fixed payload were
    /// empty, which construction rules out.
    pub fn command(&self, catalog: &ToolCatalog) -> Result<IsolatedCommand, MiseError> {
        nextest_exec(self.driver, &self.payload()[1..])?.command(catalog)
    }
}

/// Partition inventory over the validated archive.
///
/// Records sorted test identities for one partition; compiles nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NextestList {
    /// Compile-driver prefix.
    driver: NextestDriver,
    /// Partition inventoried.
    partition: NextestPartition,
    /// Resolved Nextest profile passed to `--profile`.
    profile: String,
}

impl NextestList {
    /// Build an inventory request for one partition.
    #[must_use]
    pub fn new(driver: NextestDriver, partition: NextestPartition) -> Self {
        Self {
            driver,
            partition,
            profile: NEXTEST_PROFILE.to_owned(),
        }
    }

    /// Build an inventory request with a resolved Nextest profile.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::InvalidNextestInput`] for an empty or malformed
    /// profile value.
    pub fn with_profile(
        driver: NextestDriver,
        partition: NextestPartition,
        profile: &str,
    ) -> Result<Self, MiseError> {
        check_token("profile", profile)?;
        Ok(Self {
            driver,
            partition,
            profile: profile.to_owned(),
        })
    }

    /// Resolved Nextest profile passed to `--profile`.
    #[must_use]
    pub fn profile(&self) -> &str {
        &self.profile
    }

    /// Payload arguments: program first, then its fixed arguments.
    #[must_use]
    pub fn payload(&self) -> Vec<OsString> {
        let partition = self.partition.partition_arg();
        fixed_payload(
            self.driver,
            &[
                "nextest",
                "list",
                "--profile",
                self.profile.as_str(),
                "--archive-file",
                ARCHIVE_FILE,
                "--locked",
                "--message-format",
                "json",
                "--partition",
                partition.as_str(),
            ],
        )
    }

    /// Full mise argument vector including the program.
    #[must_use]
    pub fn argv(&self, catalog: &ToolCatalog) -> Vec<OsString> {
        argv_of(catalog, self.driver, &self.payload())
    }

    /// Isolated command running this inventory.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::EmptyCommand`] only if the fixed payload were
    /// empty, which construction rules out.
    pub fn command(&self, catalog: &ToolCatalog) -> Result<IsolatedCommand, MiseError> {
        nextest_exec(self.driver, &self.payload()[1..])?.command(catalog)
    }
}

/// Partition execution from the validated archive.
///
/// Extracts to a unique directory and runs with `--no-tests fail`; an
/// empty full inventory fails unless metadata proves no tests exist.
/// Compiles nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NextestRun {
    /// Compile-driver prefix.
    driver: NextestDriver,
    /// Partition executed.
    partition: NextestPartition,
    /// Matrix key scoping the extraction directory.
    matrix_key: String,
    /// Partition id scoping the extraction directory.
    partition_id: String,
    /// Resolved Nextest profile passed to `--profile`.
    profile: String,
}

impl NextestRun {
    /// Build an execution request for one partition and extraction scope.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::InvalidNextestInput`] for an empty or malformed
    /// matrix key or partition id.
    pub fn new(
        driver: NextestDriver,
        partition: NextestPartition,
        matrix_key: &str,
        partition_id: &str,
    ) -> Result<Self, MiseError> {
        Self::with_profile(driver, partition, matrix_key, partition_id, NEXTEST_PROFILE)
    }

    /// Build an execution request with a resolved Nextest profile.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::InvalidNextestInput`] for an empty or malformed
    /// matrix key, partition id, or profile value.
    pub fn with_profile(
        driver: NextestDriver,
        partition: NextestPartition,
        matrix_key: &str,
        partition_id: &str,
        profile: &str,
    ) -> Result<Self, MiseError> {
        check_token("matrix_key", matrix_key)?;
        check_token("partition_id", partition_id)?;
        check_token("profile", profile)?;
        Ok(Self {
            driver,
            partition,
            matrix_key: matrix_key.to_owned(),
            partition_id: partition_id.to_owned(),
            profile: profile.to_owned(),
        })
    }

    /// Resolved Nextest profile passed to `--profile`.
    #[must_use]
    pub fn profile(&self) -> &str {
        &self.profile
    }

    /// Payload arguments: program first, then its fixed arguments.
    #[must_use]
    pub fn payload(&self) -> Vec<OsString> {
        let partition = self.partition.partition_arg();
        let extract_to = format!(
            "{NEXTEST_EXTRACT_BASE}/{}/{}",
            self.matrix_key, self.partition_id
        );
        fixed_payload(
            self.driver,
            &[
                "nextest",
                "run",
                "--profile",
                self.profile.as_str(),
                "--archive-file",
                ARCHIVE_FILE,
                "--extract-to",
                extract_to.as_str(),
                "--locked",
                "--no-tests",
                "fail",
                "--partition",
                partition.as_str(),
            ],
        )
    }

    /// Full mise argument vector including the program.
    #[must_use]
    pub fn argv(&self, catalog: &ToolCatalog) -> Vec<OsString> {
        argv_of(catalog, self.driver, &self.payload())
    }

    /// Isolated command running this partition.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::EmptyCommand`] only if the fixed payload were
    /// empty, which construction rules out.
    pub fn command(&self, catalog: &ToolCatalog) -> Result<IsolatedCommand, MiseError> {
        nextest_exec(self.driver, &self.payload()[1..])?.command(catalog)
    }
}
