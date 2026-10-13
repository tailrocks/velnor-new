//! Fixed Nextest archive/list/run vectors through pinned Mise tools.
//!
//! Only detected Nextest profiles use these shapes. The archive compiles one
//! exact package/configuration once; list and run consume the validated
//! archive and never compile again. Sorted features and the explicit target
//! attach at archive creation; the partition invocation carries neither.

use std::ffi::{OsStr, OsString};

use crate::catalog::{PinnedTool, ToolCatalog};
use crate::command::IsolatedCommand;
use crate::error::MiseError;
use crate::requests::PinnedToolExec;

/// Compile-driver prefix selecting the payload program and tool set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NextestDriver {
    /// Plain Cargo profile: `cargo nextest ...` under Rust plus Nextest.
    Cargo,
    /// MBX profile: `mbx nextest ...` under Rust plus MBX plus Nextest.
    Mbx,
}

impl NextestDriver {
    /// Payload program for this driver.
    #[must_use]
    pub const fn program(self) -> &'static str {
        match self {
            Self::Cargo => "cargo",
            Self::Mbx => "mbx",
        }
    }

    /// Pinned tools selecting this driver plus the Nextest runner.
    #[must_use]
    pub fn tools(self) -> Vec<PinnedTool> {
        match self {
            Self::Cargo => vec![PinnedTool::Rust, PinnedTool::Nextest],
            Self::Mbx => vec![
                PinnedTool::Rust,
                PinnedTool::MrBoxington,
                PinnedTool::Nextest,
            ],
        }
    }
}

/// Archive file every shape reads or writes.
pub const ARCHIVE_FILE: &str = "target/nextest/tests.tar.zst";

/// Runner-temp root for per-partition extraction directories.
pub const NEXTEST_EXTRACT_BASE: &str = "$RUNNER_TEMP/velnor/nextest";

/// Qualified Nextest profile shared by all three shapes.
const NEXTEST_PROFILE: &str = "ci";

/// Cargo profile selected at archive creation.
const CARGO_PROFILE: &str = "test";

/// One validated `hash:<index>/<count>` partition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NextestPartition {
    /// Partition index in Nextest `hash:M/N` numbering.
    index: u32,
    /// Total shard count.
    count: u32,
}

impl NextestPartition {
    /// Build a partition; a zero count or an index past the count fails.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::InvalidNextestInput`] when `count` is zero or
    /// `index` exceeds `count` (invalid under any numbering base).
    pub fn new(index: u32, count: u32) -> Result<Self, MiseError> {
        if count == 0 || index > count {
            return Err(MiseError::InvalidNextestInput {
                field: "partition".to_owned(),
                value: format!("{index}_of_{count}"),
            });
        }
        Ok(Self { index, count })
    }

    /// Partition index in Nextest `hash:M/N` numbering.
    #[must_use]
    pub const fn index(self) -> u32 {
        self.index
    }

    /// Total shard count.
    #[must_use]
    pub const fn count(self) -> u32 {
        self.count
    }

    /// `--partition` value: `hash:<index>/<count>`.
    #[must_use]
    pub fn partition_arg(self) -> String {
        format!("hash:{}/{}", self.index, self.count)
    }
}

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
        check_token("package", package)?;
        let mut sorted = features.to_vec();
        sorted.sort();
        for feature in &sorted {
            check_token("feature", feature)?;
        }
        let triple = target
            .map(|value| check_token("target", value).map(|()| value.to_owned()))
            .transpose()?;
        Ok(Self {
            driver,
            package: package.to_owned(),
            features: sorted,
            target: triple,
        })
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
                NEXTEST_PROFILE,
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NextestList {
    /// Compile-driver prefix.
    driver: NextestDriver,
    /// Partition inventoried.
    partition: NextestPartition,
}

impl NextestList {
    /// Build an inventory request for one partition.
    #[must_use]
    pub const fn new(driver: NextestDriver, partition: NextestPartition) -> Self {
        Self { driver, partition }
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
                NEXTEST_PROFILE,
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
        check_token("matrix_key", matrix_key)?;
        check_token("partition_id", partition_id)?;
        Ok(Self {
            driver,
            partition,
            matrix_key: matrix_key.to_owned(),
            partition_id: partition_id.to_owned(),
        })
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
                NEXTEST_PROFILE,
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

/// Reject empty values and names outside the token charset.
fn check_token(field: &str, value: &str) -> Result<(), MiseError> {
    let valid = !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'+'));
    if valid {
        Ok(())
    } else {
        Err(MiseError::InvalidNextestInput {
            field: field.to_owned(),
            value: value.to_owned(),
        })
    }
}

/// Program-first payload from the driver program plus fixed arguments.
fn fixed_payload(driver: NextestDriver, args: &[&str]) -> Vec<OsString> {
    let mut payload = Vec::with_capacity(args.len() + 1);
    payload.push(OsString::from(driver.program()));
    payload.extend(args.iter().map(OsString::from));
    payload
}

/// Pinned execution for one driver plus payload arguments.
fn nextest_exec(driver: NextestDriver, args: &[OsString]) -> Result<PinnedToolExec, MiseError> {
    PinnedToolExec::new(driver.tools(), OsStr::new(driver.program()), args.to_vec())
}

/// Full mise argv for one driver plus a program-first payload.
fn argv_of(catalog: &ToolCatalog, driver: NextestDriver, payload: &[OsString]) -> Vec<OsString> {
    let mut argv = vec![OsString::from("mise")];
    argv.extend(crate::command::mise_argv_tail(
        "exec",
        &catalog.tool_specs(&driver.tools()),
        payload,
    ));
    argv
}
