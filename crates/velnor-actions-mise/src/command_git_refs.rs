//! Native ref namespace capture and private native reconstruction.
use std::collections::BTreeMap;
use std::ffi::OsString;

use super::index::repository::RepositoryContext;
use super::owned_context::OwnedNativeContext;
use super::{Bounds, IsolatedCommand, ProcessOutput};
use crate::MiseError;

#[path = "command_git_refs_parse.rs"]
mod parse;
use self::parse::{admitted_name, parse, validate_chains};

const FORMAT: &str = "--format=%(refname)%00%(objectname)%00%(symref)%00";
const MAX_REFS: usize = 65_536;

struct NativeRef {
    name: OsString,
    oid: OsString,
    symbolic: Option<OsString>,
}

pub(super) struct NativeRefs {
    records: Vec<NativeRef>,
    namespace: Vec<u8>,
    oracle: Option<ProcessOutput>,
}

impl NativeRefs {
    pub(super) fn capture(
        owner: &IsolatedCommand,
        source: &RepositoryContext,
        width: usize,
        bounds: &Bounds<'_>,
    ) -> Result<Self, MiseError> {
        let first = query_source(owner, source, bounds)?;
        let mut records = parse(&first, width)?;
        capture_symbolic_targets(owner, source, &mut records, bounds)?;
        if query_source(owner, source, bounds)? != first {
            return Err(invalid("source_ref_namespace_changed"));
        }
        let oracle = if owner.args.first().is_some_and(|verb| verb == "rev-parse") {
            Some(source_oracle(owner, source, bounds)?)
        } else {
            None
        };
        Ok(Self {
            records,
            namespace: first,
            oracle,
        })
    }

    pub(super) fn install(
        &self,
        private: &OwnedNativeContext<'_>,
        bounds: &Bounds<'_>,
    ) -> super::super::process::Outcome {
        for symbolic in [false, true] {
            for record in &self.records {
                if record.symbolic.is_some() != symbolic {
                    continue;
                }
                let mut arguments = controls();
                match &record.symbolic {
                    Some(target) => arguments.extend([
                        "symbolic-ref".into(),
                        "--".into(),
                        record.name.clone(),
                        target.clone(),
                    ]),
                    None => arguments.extend([
                        "update-ref".into(),
                        "--no-deref".into(),
                        "--".into(),
                        record.name.clone(),
                        record.oid.clone(),
                    ]),
                }
                let outcome = run_private(private, arguments, bounds);
                if !outcome.result.as_ref().is_ok_and(|output| output.success) {
                    return redact(outcome, "private_ref_write_failed");
                }
            }
        }
        let outcome = run_private(private, namespace_arguments(), bounds);
        if outcome.result.as_ref().is_ok_and(|output| {
            output.success && output.stderr.is_empty() && output.stdout == self.namespace
        }) {
            outcome
        } else {
            redact(outcome, "private_ref_namespace_mismatch")
        }
    }

    pub(super) fn verify_source(
        &self,
        owner: &IsolatedCommand,
        source: &RepositoryContext,
        bounds: &Bounds<'_>,
    ) -> Result<(), MiseError> {
        if query_source(owner, source, bounds)? != self.namespace {
            return Err(invalid("source_ref_namespace_changed"));
        }
        for record in &self.records {
            if let Some(target) = &record.symbolic
                && symbolic_target(owner, source, &record.name, bounds)? != *target
            {
                return Err(invalid("source_symbolic_ref_changed"));
            }
        }
        if let Some(expected) = &self.oracle
            && source_oracle(owner, source, bounds)? != *expected
        {
            return Err(invalid("source_revision_oracle_changed"));
        }
        source
            .verify_binding()
            .map_err(|error| super::index_error(&error))
    }

    pub(super) fn has_head(&self) -> bool {
        self.records.iter().any(|record| record.name == "HEAD")
    }

    pub(super) fn verify_output(&self, outcome: &mut super::super::process::Outcome) {
        if let (Some(expected), Ok(output)) = (&self.oracle, &outcome.result)
            && output != expected
        {
            outcome.result = Err(invalid("private_revision_oracle_mismatch"));
        }
    }
}

fn capture_symbolic_targets(
    owner: &IsolatedCommand,
    source: &RepositoryContext,
    records: &mut [NativeRef],
    bounds: &Bounds<'_>,
) -> Result<(), MiseError> {
    let mut chains = BTreeMap::new();
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        for record in records {
            if record.symbolic.is_some() {
                record.symbolic = Some(symbolic_target(owner, source, &record.name, bounds)?);
            }
            chains.insert(
                record.name.as_bytes().to_vec(),
                record
                    .symbolic
                    .as_ref()
                    .map_or_else(Vec::new, |target| target.as_bytes().to_vec()),
            );
        }
    }
    validate_chains(&chains)
}

fn symbolic_target(
    owner: &IsolatedCommand,
    source: &RepositoryContext,
    name: &OsString,
    bounds: &Bounds<'_>,
) -> Result<OsString, MiseError> {
    let mut arguments = controls();
    arguments.extend([
        "symbolic-ref".into(),
        "--quiet".into(),
        "--no-recurse".into(),
        "--".into(),
        name.clone(),
    ]);
    let mut query = bounds.native.command(arguments);
    query.cwd.clone_from(&owner.cwd);
    let mut command = query.command()?;
    source
        .apply(&mut command)
        .map_err(|error| super::index_error(&error))?;
    let output = super::super::process::run(
        &query,
        command,
        bounds.cap,
        bounds.remaining()?,
        bounds.cancel,
    )
    .result?;
    let target = output
        .stdout
        .strip_suffix(b"\n")
        .ok_or_else(|| invalid("source_symbolic_ref_invalid"))?;
    if !output.success || !output.stderr.is_empty() || !admitted_name(target) {
        return Err(invalid("source_symbolic_ref_invalid"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        Ok(OsString::from_vec(target.to_vec()))
    }
    #[cfg(not(unix))]
    {
        Err(invalid("native_refs_platform_unsupported"))
    }
}

fn source_oracle(
    owner: &IsolatedCommand,
    source: &RepositoryContext,
    bounds: &Bounds<'_>,
) -> Result<ProcessOutput, MiseError> {
    let mut arguments = controls();
    arguments.extend(owner.args.iter().cloned());
    let mut query = bounds.native.command(arguments);
    query.cwd.clone_from(&owner.cwd);
    let mut command = query.command()?;
    source
        .apply(&mut command)
        .map_err(|error| super::index_error(&error))?;
    super::super::process::run(
        &query,
        command,
        bounds.cap,
        bounds.remaining()?,
        bounds.cancel,
    )
    .result
}

fn query_source(
    owner: &IsolatedCommand,
    source: &RepositoryContext,
    bounds: &Bounds<'_>,
) -> Result<Vec<u8>, MiseError> {
    source
        .verify_binding()
        .map_err(|error| super::index_error(&error))?;
    let mut query = bounds.native.command(namespace_arguments());
    query.cwd.clone_from(&owner.cwd);
    let mut command = query.command()?;
    source
        .apply(&mut command)
        .map_err(|error| super::index_error(&error))?;
    let output = super::super::process::run(
        &query,
        command,
        bounds.cap,
        bounds.remaining()?,
        bounds.cancel,
    )
    .result?;
    source
        .verify_binding()
        .map_err(|error| super::index_error(&error))?;
    if !output.success || !output.stderr.is_empty() {
        return Err(invalid("source_ref_capture_failed"));
    }
    Ok(output.stdout)
}

fn run_private(
    private: &OwnedNativeContext<'_>,
    arguments: Vec<OsString>,
    bounds: &Bounds<'_>,
) -> super::super::process::Outcome {
    let prepared = (|| {
        let query = bounds.native.command(arguments);
        let mut command = query.command()?;
        super::owned_config::apply_private_config_controls(&mut command);
        private
            .apply(&mut command)
            .map_err(|error| super::index_error(&error))?;
        Ok((query, command, bounds.remaining()?))
    })();
    match prepared {
        Ok((query, command, remaining)) => {
            super::super::process::run(&query, command, bounds.cap, remaining, bounds.cancel)
        }
        Err(error) => super::super::process::Outcome {
            result: Err(error),
            safe_to_cleanup: true,
        },
    }
}

fn namespace_arguments() -> Vec<OsString> {
    let mut arguments = controls();
    arguments.extend([
        "for-each-ref".into(),
        "--include-root-refs".into(),
        FORMAT.into(),
    ]);
    arguments
}

fn controls() -> Vec<OsString> {
    [
        "--no-pager",
        "-c",
        "core.hooksPath=/dev/null",
        "-c",
        "core.fsmonitor=false",
        "-c",
        "core.splitIndex=false",
        "-c",
        "core.logAllRefUpdates=false",
    ]
    .into_iter()
    .map(OsString::from)
    .collect()
}

fn redact(
    mut outcome: super::super::process::Outcome,
    code: &'static str,
) -> super::super::process::Outcome {
    if outcome.result.is_ok() {
        outcome.result = Err(invalid(code));
    }
    outcome
}

fn invalid(code: &'static str) -> MiseError {
    MiseError::InvalidStepInput {
        field: "git_refs".to_owned(),
        value: code.to_owned(),
    }
}
