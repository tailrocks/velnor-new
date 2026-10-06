//! Fixed Git controls belong to the discovery constructor.
//! Optional locks prevent status refresh writes; Git diff needs separate protection.

use std::ffi::OsString;
use std::time::{Duration, Instant};

use super::{CancelHandle, EnvPolicy, IsolatedCommand, ProcessOutput};
use crate::MiseError;

#[path = "command_git_diff.rs"]
mod diff;
#[path = "command_git_index.rs"]
mod index;
#[path = "command_git_native.rs"]
pub(super) mod native;
#[path = "command_git_objects.rs"]
mod objects;
#[path = "command_git_owned_config.rs"]
mod owned_config;
#[path = "command_git_owned_context.rs"]
mod owned_context;
#[path = "command_git_private_root.rs"]
mod private_root;
#[path = "command_git_read.rs"]
mod read;
#[path = "command_git_refs.rs"]
mod refs;
#[path = "command_git_source.rs"]
mod source;

#[cfg(test)]
#[path = "command_git_owned_tests.rs"]
mod owned_tests;

pub(super) const DISCOVERY_DIAGNOSTIC_ENV: [(&str, &str); 3] = [
    ("GIT_TRACE2", "0"),
    ("GIT_TRACE2_EVENT", "0"),
    ("GIT_TRACE2_PERF", "0"),
];

pub(super) fn is_discovery_git(owner: &IsolatedCommand) -> bool {
    owner.program == "git" && owner.policy == EnvPolicy::Discovery
}

pub(super) fn run(
    owner: &IsolatedCommand,
    cap: usize,
    timeout: Duration,
    cancel: &CancelHandle,
) -> Result<ProcessOutput, MiseError> {
    if !is_discovery_git(owner) {
        return super::process::run(owner, owner.command()?, cap, timeout, cancel).result;
    }
    if cancel.is_cancelled() {
        return Err(failed(super::SPAWN_CANCELLED_MESSAGE));
    }
    let start = Instant::now();
    let invocation = read::prepare(&owner.args)?;
    let native = native::NativeGitBinding::discover(owner, cap, timeout, cancel)?;
    let source =
        source::PreparedSource::prepare(owner, &invocation).map_err(|error| index_error(&error))?;
    let bounds = Bounds {
        native: &native,
        cap,
        start,
        timeout,
        cancel,
    };
    let outcome = run_owned(owner, invocation, &source, &bounds);
    let safe = outcome
        .as_ref()
        .map_or(true, |outcome| outcome.safe_to_cleanup);
    let result = outcome.and_then(|outcome| outcome.result);
    if !safe {
        source.retain_unreaped();
    } else if let Err(cleanup) = source.finish() {
        if result.is_ok() {
            return Err(index_error(&cleanup));
        }
        eprintln!("private_git_context:cleanup_after_error:{cleanup}");
    }
    result
}

struct PreparedOwned<'a> {
    context: owned_context::OwnedNativeContext<'a>,
    config: owned_config::OwnedConfig,
    namespace: Option<refs::NativeRefs>,
    format: Option<index::repository_format::NativeFormatResult<'a>>,
}

enum Preparation<'a> {
    Ready(Box<PreparedOwned<'a>>),
    Failed(super::process::Outcome),
}

fn run_owned(
    owner: &IsolatedCommand,
    invocation: read::ReadInvocation,
    source: &source::PreparedSource,
    bounds: &Bounds<'_>,
) -> Result<super::process::Outcome, MiseError> {
    let ready = match prepare_owned(owner, &invocation, source, bounds)? {
        Preparation::Ready(ready) => *ready,
        Preparation::Failed(outcome) => return Ok(outcome),
    };
    if let Some(refused) = objects::admit(
        owner,
        &invocation,
        &ready.context,
        ready
            .namespace
            .as_ref()
            .is_some_and(refs::NativeRefs::has_head),
        bounds,
    )? {
        return Ok(refused);
    }
    execute_owned(owner, invocation, source, ready, bounds)
}

fn prepare_owned<'a>(
    owner: &IsolatedCommand,
    invocation: &read::ReadInvocation,
    source: &'a source::PreparedSource,
    bounds: &Bounds<'_>,
) -> Result<Preparation<'a>, MiseError> {
    source
        .verify_binding()
        .map_err(|error| index_error(&error))?;
    if source.repository().is_none() && invocation.verb != read::ReadVerb::Diff {
        return Err(invalid_read("non_repository_topology_unsupported"));
    }
    let format = source_format(source, bounds)?;
    let config =
        owned_config::OwnedConfig::capture(owner, source.bound_cwd(), source.repository(), bounds)?;
    let expected = format
        .as_ref()
        .map(index::repository_format::NativeFormatResult::format);
    validate_oid_width(invocation, expected)?;
    let namespace = source
        .repository()
        .map(|repository| {
            refs::NativeRefs::capture(
                owner,
                repository,
                if expected == Some(index::repository_format::IndexObjectFormat::Sha256) {
                    64
                } else {
                    40
                },
                bounds,
            )
        })
        .transpose()?;
    let mut context = owned_context::OwnedNativeContext::prepare(
        source.root(),
        source.cwd(),
        source.repository(),
        source.common(),
        source.index().is_some(),
        expected,
        bounds,
    )?;
    let written = config.write_to(context.config_target(), bounds);
    if !written.result.as_ref().is_ok_and(|output| output.success) {
        return Ok(Preparation::Failed(written));
    }
    context
        .install_config()
        .map_err(|error| index_error(&error))?;
    let verified = verify_private_format(&context, expected, bounds)?;
    if !verified.result.as_ref().is_ok_and(|output| output.success) {
        return Ok(Preparation::Failed(verified));
    }
    if let Some(namespace) = &namespace {
        let installed = namespace.install(&context, bounds);
        if !installed.result.as_ref().is_ok_and(|output| output.success) {
            return Ok(Preparation::Failed(installed));
        }
        if let Some(repository) = source.repository() {
            namespace.verify_source(owner, repository, bounds)?;
        }
    }
    Ok(Preparation::Ready(Box::new(PreparedOwned {
        context,
        config,
        namespace,
        format,
    })))
}

fn source_format<'a>(
    source: &'a source::PreparedSource,
    bounds: &Bounds<'_>,
) -> Result<Option<index::repository_format::NativeFormatResult<'a>>, MiseError> {
    Ok(match (source.repository(), source.common_repository()) {
        (Some(repository), Some(common)) => Some(index::repository_format::probe(
            bounds.native,
            repository,
            common,
            source.expected_format(),
            bounds.cap,
            bounds.remaining()?,
            bounds.cancel,
        )?),
        (None, None) => None,
        _ => return Err(invalid_read("source_repository_binding_mismatch")),
    })
}

fn execute_owned(
    owner: &IsolatedCommand,
    invocation: read::ReadInvocation,
    source: &source::PreparedSource,
    ready: PreparedOwned<'_>,
    bounds: &Bounds<'_>,
) -> Result<super::process::Outcome, MiseError> {
    if let Some(format) = ready.format {
        format
            .verify_unchanged()
            .map_err(|error| index_error(&error))?;
    }
    source
        .verify_binding()
        .map_err(|error| index_error(&error))?;
    ready
        .config
        .verify_source(owner, source.bound_cwd(), source.repository(), bounds)?;
    let mut prepared = bounds.native.bind(owner.clone())?;
    prepared.args = invocation.arguments;
    let mut command = prepared.command()?;
    owned_config::apply_private_config_controls(&mut command);
    ready
        .context
        .apply(&mut command)
        .map_err(|error| index_error(&error))?;
    let mut outcome = super::process::run(
        &prepared,
        command,
        bounds.cap,
        bounds.remaining()?,
        bounds.cancel,
    );
    if outcome.safe_to_cleanup && outcome.result.is_ok() {
        if let Err(error) = source.verify_binding() {
            outcome.result = Err(index_error(&error));
        }
        if let Err(error) = ready.context.verify_binding() {
            outcome.result = Err(index_error(&error));
        }
    }
    if let Some(namespace) = ready.namespace.as_ref() {
        namespace.verify_output(&mut outcome);
        if outcome.safe_to_cleanup
            && outcome.result.is_ok()
            && let Some(repository) = source.repository()
            && let Err(error) = namespace.verify_source(owner, repository, bounds)
        {
            outcome.result = Err(error);
        }
    }
    if outcome.safe_to_cleanup
        && outcome.result.is_ok()
        && let Err(error) =
            ready
                .config
                .verify_source(owner, source.bound_cwd(), source.repository(), bounds)
    {
        outcome.result = Err(error);
    }
    Ok(outcome)
}

fn verify_private_format(
    context: &owned_context::OwnedNativeContext<'_>,
    expected: Option<index::repository_format::IndexObjectFormat>,
    bounds: &Bounds<'_>,
) -> Result<super::process::Outcome, MiseError> {
    let query = bounds
        .native
        .command(vec!["rev-parse".into(), "--show-object-format".into()]);
    let mut command = query.command()?;
    owned_config::apply_private_config_controls(&mut command);
    context
        .apply(&mut command)
        .map_err(|error| index_error(&error))?;
    let mut outcome = super::process::run(
        &query,
        command,
        bounds.cap,
        bounds.remaining()?,
        bounds.cancel,
    );
    if let Ok(output) = &outcome.result {
        let required: &[u8] = match expected {
            Some(index::repository_format::IndexObjectFormat::Sha256) => b"sha256\n",
            _ => b"sha1\n",
        };
        if !output.success || output.stdout != required || !output.stderr.is_empty() {
            outcome.result = Err(invalid_read("private_object_format_mismatch"));
        }
    }
    Ok(outcome)
}

fn validate_oid_width(
    invocation: &read::ReadInvocation,
    expected: Option<index::repository_format::IndexObjectFormat>,
) -> Result<(), MiseError> {
    let width = match expected {
        Some(index::repository_format::IndexObjectFormat::Sha256) => 64,
        _ => 40,
    };
    if invocation.full_oids.iter().any(|oid| oid.len() != width) {
        return Err(invalid_read("native_object_id_width_mismatch"));
    }
    Ok(())
}

fn invalid_read(code: &'static str) -> MiseError {
    MiseError::InvalidStepInput {
        field: "git_read".to_owned(),
        value: code.to_owned(),
    }
}

struct Bounds<'a> {
    native: &'a native::NativeGitBinding,
    cap: usize,
    start: Instant,
    timeout: Duration,
    cancel: &'a CancelHandle,
}

impl Bounds<'_> {
    fn remaining(&self) -> Result<Duration, MiseError> {
        self.timeout
            .checked_sub(self.start.elapsed())
            .ok_or_else(|| {
                failed(&format!(
                    "{}{}",
                    super::SPAWN_TIMEOUT_MESSAGE_PREFIX,
                    self.timeout.as_secs()
                ))
            })
    }
}

fn failed(message: &str) -> MiseError {
    MiseError::SpawnFailed {
        program: "git".to_owned(),
        message: message.to_owned(),
    }
}

fn index_error(error: &std::io::Error) -> MiseError {
    failed(&error.to_string())
}

impl IsolatedCommand {
    pub(crate) fn direct(program: &str, args: Vec<OsString>) -> Self {
        let extra_env = if program == "git" {
            vec![(OsString::from("GIT_OPTIONAL_LOCKS"), OsString::from("0"))]
        } else {
            Vec::new()
        };
        Self {
            program: OsString::from(program),
            args,
            cwd: None,
            extra_env,
            policy: EnvPolicy::Discovery,
            runtime_paths: None,
            resolved_git: None,
        }
    }
}
