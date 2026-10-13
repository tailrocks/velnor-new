//! Identity constructors and validators.
//!
//! Derivation formulas are normative; example strings are illustrative.
//! `artifact_id` values are derived GitHub artifact names. Numeric run IDs
//! appear only inside `run_key`/baseline proofs, never as identity inputs.
//! Task, input, and compatibility identities never include cwd, absolute
//! paths, or `run_key`.

pub mod artifact;
pub mod job_ids;
pub mod shard;
mod task_ids;

pub use artifact::{
    artifact_id_for_baseline, artifact_id_for_crate_job, artifact_id_for_final,
    artifact_id_for_matrix, artifact_id_for_plan, target_key, validate_artifact_id,
};
pub use shard::split_shard_suffix;

use crate::canonical::validate_digest;
use crate::errors::ContractError;
use task_ids::{validate_shard, validate_stack_task_id};

/// Define a validated identifier newtype with a private constructor.
///
/// Parsing is the only construction path; serde deserialization routes
/// through the same `TryFrom<String>` validation.
macro_rules! id_newtype {
    ($name:ident, $validate:expr) => {
        #[doc = concat!("Validated `", stringify!($name), "` identifier.")]
        #[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
        #[serde(try_from = "String")]
        pub struct $name(String);
        impl $name {
            /// Parse and validate.
            /// # Errors
            pub fn parse(value: &str) -> Result<Self, ContractError> {
                $validate(value)?;
                Ok(Self(value.to_owned()))
            }
            /// Borrow the inner string.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
            /// Unwrap the validated string.
            #[must_use]
            pub fn into_inner(self) -> String {
                self.0
            }
        }
        impl TryFrom<String> for $name {
            type Error = ContractError;
            fn try_from(value: String) -> Result<Self, Self::Error> {
                Self::parse(&value)
            }
        }
    };
}

id_newtype!(RunKey, validate_run_key);
id_newtype!(ManifestKey, validate_manifest_key);
id_newtype!(TaskId, validate_task_id);
id_newtype!(MatrixId, validate_id);
id_newtype!(MatrixKey, validate_matrix_key);
id_newtype!(PlanId, validate_plan_id);
id_newtype!(ReportId, validate_report_id);
id_newtype!(TaskReportId, validate_task_report_id);
id_newtype!(ArtifactId, artifact::validate_artifact_id);
id_newtype!(TargetKey, artifact::validate_target_key);

/// Build a CI run key `r<run-id>-a<attempt>` from numeric GitHub IDs.
#[must_use]
pub fn run_key_for_ci(run_id: u64, run_attempt: u64) -> String {
    let key = format!("r{run_id}-a{run_attempt}");
    debug_assert!(RunKey::parse(&key).is_ok());
    key
}

/// Validate a run key (`local` or `r<digits>-a<digits>`).
/// # Errors
pub fn validate_run_key(value: &str) -> Result<(), ContractError> {
    if value == "local" || is_ci_run_key(value) {
        Ok(())
    } else {
        Err(ContractError::identity("run_key", "malformed_run_key"))
    }
}

/// Derive a manifest key from a repo-relative Cargo manifest path.
///
/// Strips the trailing `/Cargo.toml`; the root manifest maps to `root`.
/// # Errors
pub fn manifest_key_for_cargo_manifest(manifest: &str) -> Result<String, ContractError> {
    if manifest.starts_with('-') {
        return Err(ContractError::identity(
            "manifest_key",
            "leading_dash_manifest",
        ));
    }
    if manifest.is_empty()
        || manifest.starts_with('/')
        || manifest.contains('\\')
        || manifest.split('/').any(|seg| seg.is_empty() || seg == "..")
    {
        return Err(ContractError::identity(
            "manifest_key",
            "non_relative_manifest",
        ));
    }
    if manifest == "Cargo.toml" {
        return Ok(ManifestKey::parse("root")?.into_inner());
    }
    let Some(dir) = manifest.strip_suffix("/Cargo.toml") else {
        return Err(ContractError::identity(
            "manifest_key",
            "missing_cargo_toml",
        ));
    };
    if dir == "root" {
        return Err(ContractError::identity("manifest_key", "reserved_root_key"));
    }
    validate_path_segments(dir, "manifest_key")?;
    Ok(ManifestKey::parse(dir)?.into_inner())
}

/// Validate a manifest key (`root` or `/`-separated segments).
/// # Errors
pub fn validate_manifest_key(value: &str) -> Result<(), ContractError> {
    if value == "root" {
        return Ok(());
    }
    validate_path_segments(value, "manifest_key")
}

/// Build `stack/<stack>/<manifest-key>/<kind>/<config>[/shard-i-of-n]`.
/// # Errors
pub fn task_id_for_stack(
    stack: &str,
    manifest_key: &str,
    kind: &str,
    config: &str,
    shard: Option<(u32, u32)>,
) -> Result<String, ContractError> {
    validate_component(stack, "stack_id")?;
    validate_manifest_key(manifest_key)?;
    validate_component(kind, "task_kind")?;
    validate_component(config, "configuration")?;
    let mut id = format!("stack/{stack}/{manifest_key}/{kind}/{config}");
    if let Some((index, count)) = shard {
        validate_shard(index, count)?;
        id = format!("{id}/shard-{index}-of-{count}");
    }
    Ok(TaskId::parse(&id)?.into_inner())
}

/// Build `internal/<kind>/<config>` for orchestration obligations.
/// # Errors
pub fn task_id_for_internal(kind: &str, config: &str) -> Result<String, ContractError> {
    validate_component(kind, "task_kind")?;
    validate_component(config, "configuration")?;
    Ok(TaskId::parse(&format!("internal/{kind}/{config}"))?.into_inner())
}

/// Validate a stack or internal task ID.
/// # Errors
pub fn validate_task_id(value: &str) -> Result<(), ContractError> {
    if let Some(rest) = value.strip_prefix("stack/") {
        return validate_stack_task_id(rest);
    }
    if let Some(rest) = value.strip_prefix("internal/") {
        let parts: Vec<&str> = rest.split('/').collect();
        if parts.len() == 2 {
            validate_component(parts[0], "task_kind")?;
            validate_component(parts[1], "configuration")?;
            return Ok(());
        }
    }
    Err(ContractError::identity("task_id", "malformed_task_id"))
}

/// Build a matrix `id` as `stack:<sid>|task:<task-group-id>`.
/// # Errors
pub fn matrix_id_for_task_group(stack: &str, task_group: &str) -> Result<String, ContractError> {
    validate_component(stack, "stack_id")?;
    validate_task_id(task_group)?;
    Ok(MatrixId::parse(&format!("stack:{stack}|task:{task_group}"))?.into_inner())
}

/// Validate a matrix `id` (charset plus `stack:`/`task:` shape).
/// # Errors
pub fn validate_id(value: &str) -> Result<(), ContractError> {
    if !value.bytes().all(is_id_byte) {
        return Err(ContractError::identity("id", "bad_charset"));
    }
    let Some(rest) = value.strip_prefix("stack:") else {
        return Err(ContractError::identity("id", "missing_stack_prefix"));
    };
    let Some((stack, group)) = rest.split_once("|task:") else {
        return Err(ContractError::identity("id", "missing_task_separator"));
    };
    validate_component(stack, "stack_id")?;
    validate_task_id(group)?;
    Ok(())
}

/// Derive `matrix_key` as `m-` + first 16 hex of BLAKE3 over `id` bytes.
/// # Errors
pub fn matrix_key_for_id(id: &str) -> Result<String, ContractError> {
    validate_id(id)?;
    let hex = blake3::hash(id.as_bytes()).to_hex();
    Ok(MatrixKey::parse(&format!("m-{}", &hex.to_string()[..16]))?.into_inner())
}

/// Validate a `matrix_key` (`m-` + 16 lowercase hex).
/// # Errors
pub fn validate_matrix_key(value: &str) -> Result<(), ContractError> {
    let Some(hex) = value.strip_prefix("m-") else {
        return Err(ContractError::identity(
            "matrix_key",
            "malformed_matrix_key",
        ));
    };
    if is_lower_hex_len(hex, 16) {
        Ok(())
    } else {
        Err(ContractError::identity(
            "matrix_key",
            "malformed_matrix_key",
        ))
    }
}

/// Derive `plan_id` as `plan-<run-key>`.
/// # Errors
pub fn plan_id_for_run(run_key: &str) -> Result<String, ContractError> {
    validate_run_key(run_key)?;
    Ok(PlanId::parse(&format!("plan-{run_key}"))?.into_inner())
}

/// Validate a `plan_id`.
/// # Errors
pub fn validate_plan_id(value: &str) -> Result<(), ContractError> {
    let Some(run_key) = value.strip_prefix("plan-") else {
        return Err(ContractError::identity("plan_id", "malformed_plan_id"));
    };
    validate_run_key(run_key).map_err(|_| ContractError::identity("plan_id", "malformed_plan_id"))
}

/// Derive `report_id` as `report-<run-key>-<matrix-key>`.
/// # Errors
pub fn report_id_for_matrix(run_key: &str, matrix_key: &str) -> Result<String, ContractError> {
    validate_run_key(run_key)?;
    validate_matrix_key(matrix_key)?;
    Ok(ReportId::parse(&format!("report-{run_key}-{matrix_key}"))?.into_inner())
}

/// Validate a `report_id`.
/// # Errors
pub fn validate_report_id(value: &str) -> Result<(), ContractError> {
    let bad = || ContractError::identity("report_id", "malformed_report_id");
    let Some(rest) = value.strip_prefix("report-") else {
        return Err(bad());
    };
    let Some((run_key, hex)) = rest.rsplit_once("-m-") else {
        return Err(bad());
    };
    validate_run_key(run_key).map_err(|_| ContractError::identity("report_id", "bad_run_key"))?;
    validate_matrix_key(&format!("m-{hex}"))
        .map_err(|_| ContractError::identity("report_id", "bad_matrix_key"))?;
    Ok(())
}

/// Derive `task_report_id` as `task-<run-key>-<matrix-key>-<digest[0:16]>`.
/// # Errors
pub fn task_report_id_for_task(
    run_key: &str,
    matrix_key: &str,
    task_digest: &str,
) -> Result<String, ContractError> {
    validate_run_key(run_key)?;
    validate_matrix_key(matrix_key)?;
    validate_digest(task_digest)
        .map_err(|_| ContractError::identity("task_digest", "bad_digest"))?;
    let id = format!("task-{run_key}-{matrix_key}-{}", &task_digest[3..19]);
    Ok(TaskReportId::parse(&id)?.into_inner())
}

/// Validate a `task_report_id`.
/// # Errors
pub fn validate_task_report_id(value: &str) -> Result<(), ContractError> {
    let bad = || ContractError::identity("task_report_id", "malformed_task_report_id");
    let Some(rest) = value.strip_prefix("task-") else {
        return Err(bad());
    };
    let Some((head, prefix)) = rest.rsplit_once('-') else {
        return Err(bad());
    };
    if !is_lower_hex_len(prefix, 16) {
        return Err(ContractError::identity(
            "task_report_id",
            "bad_digest_prefix",
        ));
    }
    let Some((run_key, hex)) = head.rsplit_once("-m-") else {
        return Err(bad());
    };
    validate_run_key(run_key)
        .map_err(|_| ContractError::identity("task_report_id", "bad_run_key"))?;
    validate_matrix_key(&format!("m-{hex}"))
        .map_err(|_| ContractError::identity("task_report_id", "bad_matrix_key"))?;
    Ok(())
}

/// Check whether a byte is allowed in a matrix `id`.
fn is_id_byte(byte: u8) -> bool {
    byte.is_ascii_lowercase()
        || byte.is_ascii_digit()
        || matches!(byte, b':' | b'|' | b'/' | b'.' | b'-' | b'_')
}

/// Check whether a byte is allowed in a single path/name component.
#[must_use]
pub fn is_component_byte(byte: u8) -> bool {
    byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'-' | b'_')
}

/// Canonical lowercase-hex charset check (empty passes vacuously;
/// fixed-width callers must use [`is_lower_hex_len`] instead).
#[must_use]
pub fn is_lower_hex(text: &str) -> bool {
    text.bytes()
        .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

/// Check exactly `len` lowercase-hex chars (empty always fails).
#[must_use]
pub fn is_lower_hex_len(text: &str, len: usize) -> bool {
    text.len() == len && is_lower_hex(text)
}

/// Check the `r<digits>-a<digits>` CI run-key shape.
fn is_ci_run_key(value: &str) -> bool {
    let Some(rest) = value.strip_prefix('r') else {
        return false;
    };
    let Some((run, attempt)) = rest.split_once("-a") else {
        return false;
    };
    !run.is_empty()
        && !attempt.is_empty()
        && run.bytes().all(|b| b.is_ascii_digit())
        && attempt.bytes().all(|b| b.is_ascii_digit())
}

/// Validate one lowercase component name (`.`/`..` are never components).
fn validate_component(value: &str, field: &'static str) -> Result<(), ContractError> {
    if !value.is_empty() && value != "." && value != ".." && value.bytes().all(is_component_byte) {
        Ok(())
    } else {
        Err(ContractError::identity(field, "bad_component"))
    }
}

/// Validate `/`-separated manifest-key segments.
fn validate_path_segments(path: &str, field: &'static str) -> Result<(), ContractError> {
    if path.is_empty() {
        return Err(ContractError::identity(field, "empty_path"));
    }
    for segment in path.split('/') {
        validate_component(segment, field)?;
    }
    Ok(())
}

/// Validate a fetch workspace root for shell interpolation.
///
/// Single source shared by the fetch-step generator (orchestrator) and
/// the render-time ambient-auth exemption (renderer): both agree on
/// which roots are safe by construction. Empty is the root workspace.
/// Rejects parent traversal, quoting and expansion characters,
/// backslashes, newlines, and absolute paths.
/// # Errors
pub fn validate_fetch_root(root: &str) -> Result<(), ContractError> {
    let bad = root.contains("..")
        || root.contains('\'')
        || root.contains('"')
        || root.contains('$')
        || root.contains('`')
        || root.contains('\\')
        || root.contains('\n')
        || root.starts_with('/');
    if bad {
        return Err(ContractError::identity(
            "fetch.root",
            format!("unsafe_fetch_root:{root}"),
        ));
    }
    Ok(())
}
