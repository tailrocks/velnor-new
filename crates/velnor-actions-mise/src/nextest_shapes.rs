// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsString;

use velnor_actions_contract::is_valid_feature_name;

use crate::catalog::ToolCatalog;
use crate::command::IsolatedCommand;
use crate::error::MiseError;
use crate::nextest::{
    ARCHIVE_FILE, CARGO_PROFILE, NEXTEST_EXTRACT_BASE, NEXTEST_PROFILE, NextestDriver,
    NextestPartition, argv_of, check_token, fixed_payload, nextest_exec,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NextestArchive {
    driver: NextestDriver,
    package: String,
    features: Vec<String>,
    target: Option<String>,
    profile: String,
}

impl NextestArchive {
    pub fn new(
        driver: NextestDriver,
        package: &str,
        features: &[String],
        target: Option<&str>,
    ) -> Result<Self, MiseError> {
        Self::with_profile(driver, package, features, target, NEXTEST_PROFILE)
    }

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

    #[must_use]
    pub fn profile(&self) -> &str {
        &self.profile
    }

    #[must_use]
    pub fn config_key(&self) -> (String, Option<String>, Vec<String>) {
        (
            self.package.clone(),
            self.target.clone(),
            self.features.clone(),
        )
    }

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

    #[must_use]
    pub fn argv(&self, catalog: &ToolCatalog) -> Vec<OsString> {
        argv_of(catalog, self.driver, &self.payload())
    }

    pub fn command(&self, catalog: &ToolCatalog) -> Result<IsolatedCommand, MiseError> {
        nextest_exec(self.driver, &self.payload()[1..])?.command(catalog)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NextestList {
    driver: NextestDriver,
    partition: NextestPartition,
    profile: String,
}

impl NextestList {
    #[must_use]
    pub fn new(driver: NextestDriver, partition: NextestPartition) -> Self {
        Self {
            driver,
            partition,
            profile: NEXTEST_PROFILE.to_owned(),
        }
    }

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

    #[must_use]
    pub fn profile(&self) -> &str {
        &self.profile
    }

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

    #[must_use]
    pub fn argv(&self, catalog: &ToolCatalog) -> Vec<OsString> {
        argv_of(catalog, self.driver, &self.payload())
    }

    pub fn command(&self, catalog: &ToolCatalog) -> Result<IsolatedCommand, MiseError> {
        nextest_exec(self.driver, &self.payload()[1..])?.command(catalog)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NextestRun {
    driver: NextestDriver,
    partition: NextestPartition,
    matrix_key: String,
    partition_id: String,
    profile: String,
}

impl NextestRun {
    pub fn new(
        driver: NextestDriver,
        partition: NextestPartition,
        matrix_key: &str,
        partition_id: &str,
    ) -> Result<Self, MiseError> {
        Self::with_profile(driver, partition, matrix_key, partition_id, NEXTEST_PROFILE)
    }

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

    #[must_use]
    pub fn profile(&self) -> &str {
        &self.profile
    }

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

    #[must_use]
    pub fn argv(&self, catalog: &ToolCatalog) -> Vec<OsString> {
        argv_of(catalog, self.driver, &self.payload())
    }

    pub fn command(&self, catalog: &ToolCatalog) -> Result<IsolatedCommand, MiseError> {
        nextest_exec(self.driver, &self.payload()[1..])?.command(catalog)
    }
}
