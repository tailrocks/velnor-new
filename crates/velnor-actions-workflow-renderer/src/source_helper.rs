//! Generic rendering of exact compiled-owner helper authority records.
pub use crate::source_helper_budget::{
    PreparedTransport, TransportAdmission, TransportBudgetExceeded,
};
use crate::{RenderError, marker, steps, yaml::Yaml};
use std::collections::BTreeMap;
use velnor_actions_contract::{CompiledSourceHelper, HelperInvocation, Step, StepKind};

#[path = "source_helper_transport.rs"]
mod transport;

#[path = "source_helper_credentials.rs"]
mod credentials;

#[path = "source_helper_input_expressions.rs"]
mod input_expressions;

#[cfg(test)]
#[path = "source_helper_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "source_helper_credential_tests.rs"]
mod credential_tests;

#[cfg(test)]
#[path = "source_helper_budget_tests.rs"]
mod budget_tests;

#[cfg(test)]
#[path = "source_helper_snapshot_tests.rs"]
mod snapshot_tests;

/// Startup controls blanked before the fixed isolated launcher starts.
pub const HELPER_STARTUP_KEYS: [&str; 13] = [
    "BASH_ENV",
    "ENV",
    "PYTHONPATH",
    "PYTHONHOME",
    "CDPATH",
    "LD_PRELOAD",
    "LD_LIBRARY_PATH",
    "LD_AUDIT",
    "DYLD_INSERT_LIBRARIES",
    "DYLD_LIBRARY_PATH",
    "DYLD_FRAMEWORK_PATH",
    "DYLD_FALLBACK_LIBRARY_PATH",
    "DYLD_FALLBACK_FRAMEWORK_PATH",
];

/// Construct an owner-qualified step; rendering still requires registry admission.
/// # Errors
/// Rejects unsafe names or environments.
pub fn source_helper_step(
    name: &str,
    record: &CompiledSourceHelper,
    env: BTreeMap<String, String>,
) -> Result<Step, RenderError> {
    validate_content(record)?;
    if name.trim().is_empty() {
        return Err(RenderError::BadCommand("empty_name".to_owned()));
    }
    crate::expressions::check_name_content(name)?;
    input_expressions::validate(record, &env)?;
    credentials::validate_step(record, &env)?;
    reject_startup(&env)?;
    if record.environment() != &env {
        return Err(RenderError::BadCommand(
            "source_helper_environment_mismatch".to_owned(),
        ));
    }
    steps::scan_for_private_subcommands(name)?;
    Ok(Step {
        id: None,
        name: name.to_owned(),
        condition: None,
        kind: StepKind::SourceBoundHelper {
            invocation: record.invocation().clone(),
            env,
        },
    })
}

/// Validate a token-bearing helper environment against its compiled owner.
pub(crate) fn validate_helper_credentials(
    record: &CompiledSourceHelper,
    environment: &BTreeMap<String, String>,
) -> Result<(), RenderError> {
    credentials::validate_step(record, environment)
}

fn reject_startup(env: &BTreeMap<String, String>) -> Result<(), RenderError> {
    if env
        .keys()
        .any(|key| HELPER_STARTUP_KEYS.contains(&key.as_str()))
    {
        return Err(RenderError::BadCommand(
            "source_helper_startup_env".to_owned(),
        ));
    }
    Ok(())
}

fn validate_content(record: &CompiledSourceHelper) -> Result<(), RenderError> {
    record.validate_binding().map_err(RenderError::Contract)?;
    input_expressions::validate(record, record.environment())?;
    credentials::validate_record(record)?;
    reject_startup(record.environment())?;
    if record.source().contains("${{") {
        return Err(RenderError::BadCommand(
            "source_helper_source_expression".to_owned(),
        ));
    }
    steps::scan_for_private_subcommands(record.source())?;
    transport::validate_raw_arguments(record)?;
    for argument in record.invocation().args() {
        crate::expressions::check_helper_argument(argument)?;
        steps::scan_for_private_subcommands(argument)?;
    }
    Ok(())
}

/// Measure a complete qualified helper before allocating an optional producer.
/// # Errors
/// Invalid source authority, environment, credentials or runner labels remain errors.
pub fn admit_source_helper_transport(
    record: &CompiledSourceHelper,
    runs_on: &str,
) -> Result<TransportAdmission, RenderError> {
    transport::validate_runner(runs_on)?;
    match validate_content(record) {
        Ok(()) => {}
        Err(RenderError::UnsupportedHelperTransport(budget)) => {
            return Ok(TransportAdmission::UnsupportedBudget(budget));
        }
        Err(error) => return Err(error),
    }
    let mut environment = credentials::scrubbed_environment(record, record.environment())?;
    environment.extend(
        HELPER_STARTUP_KEYS
            .iter()
            .map(|key| ((*key).to_owned(), String::new())),
    );
    match transport::encode(record, &mut environment, runs_on) {
        Ok(run) => Ok(TransportAdmission::Supported(PreparedTransport {
            run,
            environment,
        })),
        Err(RenderError::UnsupportedHelperTransport(budget)) => {
            Ok(TransportAdmission::UnsupportedBudget(budget))
        }
        Err(error) => Err(error),
    }
}

/// Validate the generic registry's paths, marker versions and conflicting records.
/// # Errors
/// Rejects malformed or contradictory owner records.
pub fn validate_registry(
    records: &[CompiledSourceHelper],
    version: &str,
) -> Result<(), RenderError> {
    for (index, record) in records.iter().enumerate() {
        validate_content(record)?;
        marker::check_first_line(record.source(), version)?;
        for previous in &records[..index] {
            if previous.invocation() == record.invocation()
                && previous.environment() == record.environment()
                && previous != record
            {
                return Err(RenderError::BadCommand(
                    "source_helper_recipe_conflict".to_owned(),
                ));
            }
            if previous.invocation().descriptor().path() == record.invocation().descriptor().path()
                && (previous.source() != record.source()
                    || previous.invocation().descriptor() != record.invocation().descriptor())
            {
                return Err(RenderError::BadCommand(
                    "source_helper_registry_conflict".to_owned(),
                ));
            }
        }
    }
    Ok(())
}

/// Render only an invocation exactly admitted by its compiled source owner.
///
/// # Errors
/// Rejects unqualified steps, invalid owner records and unsupported runner budgets.
pub fn source_helper_step_to_yaml(
    step: &Step,
    records: &[CompiledSourceHelper],
    version: &str,
    runs_on: &str,
) -> Result<Yaml, RenderError> {
    validate_registry(records, version)?;
    match &step.kind {
        StepKind::SourceBoundHelper { invocation, env } => {
            step_to_yaml(step, invocation, env, records, runs_on)
        }
        _ => Err(RenderError::InvalidWorkflow(
            "source_helper_step_required".to_owned(),
        )),
    }
}

/// Serialize one helper after its enclosing workflow registry was validated.
pub(crate) fn step_to_yaml(
    step: &Step,
    invocation: &HelperInvocation,
    env: &BTreeMap<String, String>,
    records: &[CompiledSourceHelper],
    runs_on: &str,
) -> Result<Yaml, RenderError> {
    crate::receipt_preparation_admission::validate_invocation(invocation)?;
    crate::expressions::check_name_content(&step.name)?;
    steps::scan_for_private_subcommands(&step.name)?;
    let record = records
        .iter()
        .find(|record| record.invocation() == invocation && record.environment() == env)
        .ok_or_else(|| RenderError::InvalidWorkflow("source_helper_unqualified".to_owned()))?;
    validate_content(record)?;
    invocation.validate().map_err(RenderError::Contract)?;
    if record.environment() != env {
        return Err(RenderError::BadCommand(
            "source_helper_environment_mismatch".to_owned(),
        ));
    }
    input_expressions::validate(record, env)?;
    credentials::validate_step(record, env)?;
    reject_startup(env)?;
    let mut entries = crate::steps_plain::step_header(step)?;
    if let Some(condition) = &step.condition {
        steps::scan_for_private_subcommands(condition)?;
        entries.push(("if".to_owned(), Yaml::str(condition.clone())));
    }
    let PreparedTransport { run, environment } =
        match admit_source_helper_transport(record, runs_on)? {
            TransportAdmission::Supported(prepared) => prepared,
            TransportAdmission::UnsupportedBudget(budget) => {
                return Err(RenderError::UnsupportedHelperTransport(budget));
            }
        };
    entries.push((
        "env".to_owned(),
        Yaml::Map(
            environment
                .into_iter()
                .map(|(key, value)| (key, Yaml::str(value)))
                .collect(),
        ),
    ));
    entries.push((
        "shell".to_owned(),
        Yaml::str("/bin/bash --noprofile --norc -p -e -o pipefail {0}"),
    ));
    entries.push(("run".to_owned(), Yaml::str(run)));
    Ok(Yaml::Map(entries))
}

/// Auxiliary review files corresponding to the executed compiled sources.
/// # Errors
/// Rejects conflicting records or invalid marker versions.
pub fn source_helper_files(
    records: &[CompiledSourceHelper],
    version: &str,
) -> Result<Vec<crate::RenderedFile>, RenderError> {
    validate_registry(records, version)?;
    let mut files = BTreeMap::new();
    for record in records {
        files.insert(
            record.invocation().descriptor().path().to_owned(),
            record.source().to_owned(),
        );
    }
    Ok(files
        .into_iter()
        .map(|(path, bytes)| crate::RenderedFile { path, bytes })
        .collect())
}
