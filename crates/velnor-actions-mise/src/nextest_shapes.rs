//! Archive, inventory, and execution shapes for pinned Nextest runs.
//! Routing and partition helpers stay in [`crate::nextest`].

use std::ffi::OsString;

use velnor_actions_contract::{is_valid_feature_name, normalize_posix_path};

use crate::catalog::ToolCatalog;
use crate::command::IsolatedCommand;
use crate::error::MiseError;
use crate::nextest::{
    ARCHIVE_FILE, CARGO_PROFILE, NEXTEST_EXTRACT_BASE, NEXTEST_PROFILE, NextestDriver,
    NextestPartition, argv_of, check_token, fixed_payload, nextest_exec,
};

/// Append typed filter and ignored-mode arguments after Mise's `--`.
/// # Errors
pub fn append_selection_arguments(
    argv: &mut Vec<OsString>,
    run_ignored: Option<&str>,
    filterset: Option<&str>,
) -> Result<(), MiseError> {
    let Some(separator) = argv.iter().position(|arg| arg == "--") else {
        return Err(MiseError::InvalidNextestInput {
            field: "argv".to_owned(),
            value: "missing_payload_separator".to_owned(),
        });
    };
    let payload = &argv[separator + 1..];
    for flag in ["--run-ignored", "--filterset"] {
        if payload.iter().any(|arg| arg == flag) {
            return Err(MiseError::InvalidNextestInput {
                field: "selection".to_owned(),
                value: format!("duplicate:{flag}"),
            });
        }
    }
    if let Some(mode) = run_ignored {
        if !matches!(mode, "all" | "only" | "ignored-only") {
            return Err(MiseError::InvalidNextestInput {
                field: "run_ignored".to_owned(),
                value: mode.to_owned(),
            });
        }
        argv.extend([OsString::from("--run-ignored"), OsString::from(mode)]);
    }
    if let Some(filter) = filterset {
        if filter.is_empty() || filter.bytes().any(|byte| byte.is_ascii_control()) {
            return Err(MiseError::InvalidNextestInput {
                field: "filterset".to_owned(),
                value: filter.to_owned(),
            });
        }
        argv.extend([OsString::from("--filterset"), OsString::from(filter)]);
    }
    Ok(())
}

/// Archive creation for one exact package/configuration.
/// Features are sorted; host builds omit `--target`.
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
    /// Workspace manifest for a nested Cargo workspace, when supplied.
    manifest_path: Option<String>,
    /// Resolved Nextest profile passed to `--profile`.
    profile: String,
}

impl NextestArchive {
    /// Build an archive request; features are sorted.
    /// # Errors: rejects malformed package, feature, or target values.
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
    /// Build an archive request with a resolved profile.
    /// Features use the contract charset, including weak-dependency syntax.
    /// # Errors: rejects malformed package, feature, target, or profile values.
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
            manifest_path: None,
            profile: profile.to_owned(),
        })
    }

    /// Bind archive creation to its workspace manifest.
    /// # Errors: rejects absolute or traversing paths.
    pub fn with_manifest_path(mut self, manifest_path: &str) -> Result<Self, MiseError> {
        self.manifest_path = Some(normalize_posix_path(manifest_path).map_err(|_| {
            MiseError::InvalidNextestInput {
                field: "manifest_path".to_owned(),
                value: manifest_path.to_owned(),
            }
        })?);
        Ok(self)
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
        if let Some(manifest) = &self.manifest_path {
            payload.splice(
                5..5,
                [OsString::from("--manifest-path"), OsString::from(manifest)],
            );
        }
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
    /// # Errors: returned only if the fixed payload is empty.
    pub fn command(&self, catalog: &ToolCatalog) -> Result<IsolatedCommand, MiseError> {
        nextest_exec(self.driver, &self.payload()[1..])?.command(catalog)
    }
}

/// Inventory sorted test identities in one archive partition.
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

    /// Build an inventory request with a resolved profile.
    /// # Errors: rejects malformed profiles.
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
    /// # Errors: returned only if the fixed payload is empty.
    pub fn command(&self, catalog: &ToolCatalog) -> Result<IsolatedCommand, MiseError> {
        nextest_exec(self.driver, &self.payload()[1..])?.command(catalog)
    }
}

/// Extract and execute one archive partition without compiling.
/// Empty partitions fail through `--no-tests fail`.
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
    /// # Errors: rejects malformed matrix keys or partition IDs.
    pub fn new(
        driver: NextestDriver,
        partition: NextestPartition,
        matrix_key: &str,
        partition_id: &str,
    ) -> Result<Self, MiseError> {
        Self::with_profile(driver, partition, matrix_key, partition_id, NEXTEST_PROFILE)
    }

    /// Build an execution request with a resolved profile.
    /// # Errors: rejects malformed matrix keys, partition IDs, or profiles.
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
    /// # Errors: returned only if the fixed payload is empty.
    pub fn command(&self, catalog: &ToolCatalog) -> Result<IsolatedCommand, MiseError> {
        nextest_exec(self.driver, &self.payload()[1..])?.command(catalog)
    }
}
