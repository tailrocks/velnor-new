//! Native effective configuration admitted into an owned, flat Git config.
//! Values stay private; failures report fixed codes rather than config contents.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::process::Command;

use super::index::repository::RepositoryContext;
use super::owned_context::OwnedConfigTarget;
use super::{Bounds, IsolatedCommand};
use crate::MiseError;

#[path = "command_git_owned_config_protocol.rs"]
mod protocol;
use protocol::{
    name_arguments, named_values, native_names, native_records, ordered_tuples, single_pair_matches,
};
const MAX_CONFIG_BYTES: usize = 8 * 1024 * 1024;

pub(super) struct OwnedConfig {
    entries: Vec<(OsString, OsString)>,
    captured: Vec<u8>,
}

impl OwnedConfig {
    pub(super) fn capture(
        owner: &IsolatedCommand,
        cwd: &super::private_root::BoundCwd,
        context: Option<&RepositoryContext>,
        bounds: &Bounds<'_>,
    ) -> Result<Self, MiseError> {
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt;
            let names = source_query(owner, cwd, context, name_arguments(), bounds)?;
            let keys = native_names(&names)?;
            let mut values = BTreeMap::new();
            let mut captured_bytes = names.len();
            for key in &keys {
                if values.contains_key(*key) {
                    continue;
                }
                let bytes = source_query(
                    owner,
                    cwd,
                    context,
                    vec![
                        "--null".into(),
                        "--get-all".into(),
                        "--show-names".into(),
                        "--includes".into(),
                        "--".into(),
                        OsString::from_vec(key.to_vec()),
                    ],
                    bounds,
                )?;
                captured_bytes = captured_bytes
                    .checked_add(bytes.len())
                    .filter(|size| *size <= bounds.cap.min(MAX_CONFIG_BYTES))
                    .ok_or_else(|| invalid("native_config_capture_too_large"))?;
                values.insert(key.to_vec(), named_values(key, &bytes)?);
            }
            let tuples = ordered_tuples(&keys, values)?;
            if source_query(owner, cwd, context, name_arguments(), bounds)? != names
                || source_query(
                    owner,
                    cwd,
                    context,
                    vec!["--null".into(), "--list".into(), "--includes".into()],
                    bounds,
                )? != tuples
            {
                return Err(invalid("native_config_capture_drift"));
            }
            Self::from_native_output(&tuples)
        }
        #[cfg(not(unix))]
        {
            let _ = (owner, cwd, context, bounds);
            Err(invalid("native_config_unsupported_platform"))
        }
    }

    fn from_native_output(bytes: &[u8]) -> Result<Self, MiseError> {
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt;
            let records = native_records(bytes)?;
            let mut entries = Vec::with_capacity(records.len());
            for (key, value) in records {
                match admission(key)? {
                    Admission::Keep => entries.push((
                        OsString::from_vec(key.to_vec()),
                        OsString::from_vec(value.to_vec()),
                    )),
                    Admission::OwnedRouting => {}
                }
            }
            Ok(Self {
                entries,
                captured: bytes.to_vec(),
            })
        }
        #[cfg(not(unix))]
        {
            let _ = bytes;
            Err(invalid("native_config_unsupported_platform"))
        }
    }

    /// Native Git alone serializes values. Roundtrip rejects native reordering.
    pub(super) fn write_to(
        &self,
        target: &OwnedConfigTarget<'_>,
        bounds: &Bounds<'_>,
    ) -> super::super::process::Outcome {
        let mut flat = Vec::new();
        for (key, value) in &self.entries {
            if target.reset_fragment().is_err() {
                return before_spawn_error("native_config_fragment_reset");
            }
            let written = native_file_command(
                target,
                ConfigFile::Fragment,
                vec!["--add".into(), "--".into(), key.clone(), value.clone()],
                bounds,
            );
            if !written.result.as_ref().is_ok_and(|output| output.success) {
                return redact(written, "native_config_write_failed");
            }
            let checked = native_file_command(
                target,
                ConfigFile::Fragment,
                vec!["--null".into(), "--list".into(), "--no-includes".into()],
                bounds,
            );
            if !checked.result.as_ref().is_ok_and(|output| {
                output.success && single_pair_matches(&output.stdout, key, value)
            }) {
                return redact(checked, "native_config_fragment_mismatch");
            }
            let Ok(fragment) = target.fragment_bytes() else {
                return before_spawn_error("native_config_fragment_read");
            };
            if flat
                .len()
                .checked_add(fragment.len())
                .is_none_or(|size| size > bounds.cap.min(MAX_CONFIG_BYTES))
            {
                return before_spawn_error("native_config_serialized_too_large");
            }
            flat.extend(fragment);
        }
        if target.store_flat(&flat).is_err() {
            return before_spawn_error("native_config_flat_write");
        }
        let outcome = native_file_command(
            target,
            ConfigFile::Flat,
            vec!["--null".into(), "--list".into(), "--no-includes".into()],
            bounds,
        );
        match outcome.result.as_ref() {
            Ok(output) if output.success && self.matches_native_output(&output.stdout) => outcome,
            _ => redact(outcome, "native_config_roundtrip_mismatch"),
        }
    }

    fn matches_native_output(&self, bytes: &[u8]) -> bool {
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            let Ok(records) = native_records(bytes) else {
                return false;
            };
            records.len() == self.entries.len()
                && records
                    .iter()
                    .zip(&self.entries)
                    .all(|((key, value), entry)| {
                        *key == entry.0.as_bytes() && *value == entry.1.as_bytes()
                    })
        }
        #[cfg(not(unix))]
        {
            let _ = bytes;
            false
        }
    }
}

fn source_query(
    owner: &IsolatedCommand,
    cwd: &super::private_root::BoundCwd,
    context: Option<&RepositoryContext>,
    tail: Vec<OsString>,
    bounds: &Bounds<'_>,
) -> Result<Vec<u8>, MiseError> {
    if !super::is_discovery_git(owner) {
        return Err(invalid("native_config_owner_identity"));
    }
    let mut args = vec!["--no-pager".into(), "config".into()];
    args.extend(tail);
    let mut query = bounds.native.command(args);
    cwd.verify_binding()
        .map_err(|error| super::index_error(&error))?;
    query.cwd = Some(cwd.path().to_owned());
    let mut command = query.command()?;
    if let Some(context) = context {
        context
            .apply(&mut command)
            .map_err(|error| super::index_error(&error))?;
    }
    suppress_delegated_diagnostics(&mut command);
    let output = super::super::process::run(
        &query,
        command,
        bounds.cap.min(MAX_CONFIG_BYTES),
        bounds.remaining()?,
        bounds.cancel,
    )
    .result?;
    if !output.success {
        return Err(invalid("native_config_capture_failed"));
    }
    cwd.verify_binding()
        .map_err(|error| super::index_error(&error))?;
    Ok(output.stdout)
}

#[derive(Clone, Copy)]
enum ConfigFile {
    Fragment,
    Flat,
}

fn before_spawn_error(code: &'static str) -> super::super::process::Outcome {
    super::super::process::Outcome {
        result: Err(invalid(code)),
        safe_to_cleanup: true,
    }
}

fn native_file_command(
    target: &OwnedConfigTarget<'_>,
    file: ConfigFile,
    tail: Vec<OsString>,
    bounds: &Bounds<'_>,
) -> super::super::process::Outcome {
    let result = (|| {
        let path = match file {
            ConfigFile::Fragment => target.path(),
            ConfigFile::Flat => target.effective_path(),
        };
        let mut args = vec![
            "--no-pager".into(),
            "config".into(),
            "--file".into(),
            path.as_os_str().to_owned(),
        ];
        args.extend(tail);
        let mut query = bounds.native.command(args);
        query.cwd = path.parent().map(std::path::Path::to_path_buf);
        let mut command = query.command()?;
        apply_private_config_controls(&mut command);
        target
            .apply(&mut command)
            .map_err(|error| super::index_error(&error))?;
        Ok((query, command, bounds.remaining()?))
    })();
    match result {
        Ok((query, command, timeout)) => {
            let mut outcome =
                super::super::process::run(&query, command, bounds.cap, timeout, bounds.cancel);
            if outcome.safe_to_cleanup
                && outcome.result.is_ok()
                && let Err(error) = target.verify_effective()
            {
                outcome.result = Err(super::index_error(&error));
            }
            outcome
        }
        Err(error) => super::super::process::Outcome {
            result: Err(error),
            safe_to_cleanup: true,
        },
    }
}

pub(super) fn apply_private_config_controls(command: &mut Command) {
    suppress_delegated_diagnostics(command);
    command.env("GIT_CONFIG_SYSTEM", "/dev/null");
    command.env("GIT_CONFIG_GLOBAL", "/dev/null");
    command.env("GIT_CONFIG_NOSYSTEM", "1");
    command.env("GIT_NO_LAZY_FETCH", "1");
}

fn suppress_delegated_diagnostics(command: &mut Command) {
    for key in ["GIT_TRACE2", "GIT_TRACE2_EVENT", "GIT_TRACE2_PERF"] {
        command.env(key, "0");
    }
    command.env("GIT_PAGER", "cat");
}

enum Admission {
    Keep,
    OwnedRouting,
}

fn admission(key: &[u8]) -> Result<Admission, MiseError> {
    if key == b"include.path"
        || (key.starts_with(b"includeif.") && key.ends_with(b".path"))
        || key == b"extensions.worktreeconfig"
    {
        return Ok(Admission::OwnedRouting);
    }
    if key.starts_with(b"filter.") && (key.ends_with(b".clean") || key.ends_with(b".process")) {
        return Err(invalid("native_config_filter_unsupported"));
    }
    if key.starts_with(b"submodule.")
        || key == b"core.worktree"
        || key == b"diff.submodule"
        || key == b"extensions.partialclone"
        || (key.starts_with(b"remote.") && key.ends_with(b".promisor"))
        || (key.starts_with(b"extensions.") && key != b"extensions.objectformat")
    {
        return Err(invalid("native_config_context_unsupported"));
    }
    Ok(Admission::Keep)
}

fn redact(
    outcome: super::super::process::Outcome,
    code: &'static str,
) -> super::super::process::Outcome {
    super::super::process::Outcome {
        result: match outcome.result {
            Err(error) => Err(error),
            Ok(_) => Err(invalid(code)),
        },
        safe_to_cleanup: outcome.safe_to_cleanup,
    }
}

fn invalid(code: &'static str) -> MiseError {
    MiseError::InvalidStepInput {
        field: "git_owned_config".to_owned(),
        value: code.to_owned(),
    }
}

#[cfg(test)]
#[path = "command_git_owned_config_tests.rs"]
mod tests;
