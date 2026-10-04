//! Immutable generation descriptors for closed local database fixtures.

use serde::de::DeserializeOwned;
use velnor_actions_contract::{ProposedTask, Stack, canonical_json_bytes, config::PostgresFixture};

use crate::OrchestratorError;

pub(crate) const POSTGRES_FIXTURE_KEY: &str = "VELNOR_GRADLE_POSTGRES_FIXTURE";
const MAX_DESCRIPTOR_BYTES: usize = 4 * 1024 * 1024;

/// Read a closed local fixture bound into every grouped Gradle task.
pub(crate) fn postgres_fixture(
    tasks: &[&ProposedTask],
) -> Result<Option<PostgresFixture>, OrchestratorError> {
    let fixture = descriptor::<Option<PostgresFixture>>(
        tasks,
        POSTGRES_FIXTURE_KEY,
        &["gradle_check", "gradle_database_check"],
    )?
    .flatten();
    if fixture.is_none()
        && tasks
            .iter()
            .any(|task| task.configuration == "gradle_database_check")
    {
        return Err(invalid(POSTGRES_FIXTURE_KEY, "missing_required_fixture"));
    }
    if let Some(fixture) = &fixture {
        fixture.validate("task.identity.environment", POSTGRES_FIXTURE_KEY)?;
    }
    Ok(fixture)
}

/// Canonical validated descriptor bytes for the native toolchain preimage.
pub(crate) fn fixture_identity(task: &ProposedTask) -> Result<Option<String>, OrchestratorError> {
    postgres_fixture(&[task])?
        .map(|fixture| {
            String::from_utf8(canonical_json_bytes(&fixture)?)
                .map_err(|_| invalid(POSTGRES_FIXTURE_KEY, "non_utf8_descriptor"))
        })
        .transpose()
}

fn descriptor<T: DeserializeOwned + PartialEq>(
    tasks: &[&ProposedTask],
    key: &str,
    kinds: &[&str],
) -> Result<Option<T>, OrchestratorError> {
    let Some(first) = tasks.first() else {
        return Ok(None);
    };
    let mut values = tasks.iter().map(|task| {
        let Some(raw) = task.identity.environment.get(key) else {
            return Ok(None);
        };
        if task.stack_id != Stack::Workload.id() || !kinds.contains(&task.configuration.as_str()) {
            return Err(invalid(key, "wrong_workload_kind"));
        }
        if task.identity.unit_id != first.identity.unit_id
            || task.identity.unit_path != first.identity.unit_path
            || task.identity.project_root != first.identity.project_root
        {
            return Err(invalid(key, "mixed_group_identity"));
        }
        if raw.len() > MAX_DESCRIPTOR_BYTES {
            return Err(invalid(key, "descriptor_size"));
        }
        serde_json::from_str::<T>(raw)
            .map(Some)
            .map_err(|_| invalid(key, "malformed_descriptor"))
    });
    let Some(first) = values.next() else {
        return Ok(None);
    };
    let descriptor = first?;
    for value in values {
        if value? != descriptor {
            return Err(invalid(key, "contradictory_group_descriptors"));
        }
    }
    Ok(descriptor)
}

fn invalid(key: &str, problem: &str) -> OrchestratorError {
    OrchestratorError::Contract {
        problem: format!("workload_generation_metadata:{key}:{problem}"),
    }
}

#[cfg(test)]
#[path = "workloads_metadata_tests.rs"]
mod tests;
