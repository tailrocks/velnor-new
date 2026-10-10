//! Driver routing, partitions, and payload helpers for Nextest vectors.
//!
//! The archive/list/run shapes live in [`crate::nextest_shapes`]; this
//! module owns the compile-driver prefix, partition numbering, shared
//! profile constants, and the pinned-execution constructors.

use std::ffi::OsString;

use crate::catalog::{PinnedTool, ToolCatalog};
use crate::error::MiseError;
use crate::preflight::RouteDriver;
use crate::requests::PinnedToolExec;

/// Compile-driver prefix selecting the payload program and tool set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NextestDriver {
    /// Plain Cargo profile: `cargo nextest ...` under Rust plus Nextest.
    Cargo,
    /// MBX profile: `mbx nextest ...` under action-owned MBX plus Rust and Nextest.
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
            Self::Cargo | Self::Mbx => vec![PinnedTool::Rust, PinnedTool::Nextest],
        }
    }

    /// Construct an execution using this driver's pinned authority.
    ///
    /// MBX is supplied by the separately pinned workflow action, while Mise
    /// selects Rust and Nextest. It must not be impersonated as a catalog
    /// tool; Cargo remains a normal Mise-selected payload.
    pub(crate) fn exec(self, args: Vec<OsString>) -> Result<PinnedToolExec, MiseError> {
        let route = match self {
            Self::Cargo => RouteDriver::Cargo,
            Self::Mbx => RouteDriver::Mbx,
        };
        route.task_exec(args, true)
    }
}

/// Archive file every shape reads or writes.
pub const ARCHIVE_FILE: &str = "target/nextest/tests.tar.zst";

/// Runner-temp root for per-partition extraction directories.
pub const NEXTEST_EXTRACT_BASE: &str = "$RUNNER_TEMP/velnor/nextest";

/// Qualified Nextest profile shared by all three shapes.
///
/// Default constructors select `ci`; `with_profile` constructors (see
/// [`crate::nextest_shapes`]) select another resolved profile such as
/// Nextest's documented `default` when the project has no `[profile.ci]`.
pub(crate) const NEXTEST_PROFILE: &str = "ci";

/// Cargo profile selected at archive creation.
pub(crate) const CARGO_PROFILE: &str = "test";

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

/// Reject empty values and names outside the token charset.
pub(crate) fn check_token(field: &str, value: &str) -> Result<(), MiseError> {
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
pub(crate) fn fixed_payload(driver: NextestDriver, args: &[&str]) -> Vec<OsString> {
    let mut payload = Vec::with_capacity(args.len() + 1);
    payload.push(OsString::from(driver.program()));
    payload.extend(args.iter().map(OsString::from));
    payload
}

/// Pinned execution for one driver plus payload arguments.
pub(crate) fn nextest_exec(
    driver: NextestDriver,
    args: &[OsString],
) -> Result<PinnedToolExec, MiseError> {
    driver.exec(args.to_vec())
}

/// Full mise argv for one driver plus a program-first payload.
pub(crate) fn argv_of(
    catalog: &ToolCatalog,
    driver: NextestDriver,
    payload: &[OsString],
) -> Vec<OsString> {
    let mut argv = vec![OsString::from("mise")];
    argv.extend(crate::command::mise_argv_tail(
        "exec",
        &catalog.tool_specs(&driver.tools()),
        payload,
    ));
    argv
}
