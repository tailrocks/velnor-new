//! Positive environment policy before Mise selects the qualified Java installation.

use crate::MiseError;
use velnor_actions_contract::config::PostgresFixture;

/// Retain only generator-owned runtime locations and closed local fixture values.
/// Java and Gradle option variables never enter from the parent environment.
/// # Errors
/// Rejects invalid local database fixture values or environment binding names.
pub fn java_isolation_prefix(fixture: Option<&PostgresFixture>) -> Result<String, MiseError> {
    let mut prefix = String::from("/usr/bin/env -i PATH=\"$PATH\"");
    for key in ["HOME", "RUNNER_TEMP", "MISE_DATA_DIR"] {
        prefix.push(' ');
        prefix.push_str(key);
        prefix.push_str("=\"$");
        prefix.push_str(key);
        prefix.push('"');
    }
    prefix.push_str(" GRADLE_USER_HOME=\"$RUNNER_TEMP/velnor/native/gradle\" CI=true");
    for (key, value) in crate::ISOLATION_ENV
        .iter()
        .chain(crate::NO_AUTO_INSTALL_ENV.iter())
    {
        prefix.push(' ');
        prefix.push_str(key);
        prefix.push_str("='");
        prefix.push_str(value);
        prefix.push('\'');
    }
    if let Some(fixture) = fixture {
        fixture
            .validate("gradle_environment", "postgres")
            .map_err(|error| MiseError::InvalidStepInput {
                field: "java_fixture".to_owned(),
                value: error.to_string(),
            })?;
        for key in fixture.bindings.keys() {
            prefix.push(' ');
            prefix.push_str(key);
            prefix.push_str("=\"${");
            prefix.push_str(key);
            prefix.push_str(":?}\"");
        }
    }
    prefix.push(' ');
    Ok(prefix)
}

#[cfg(test)]
#[path = "workload_java_env_tests.rs"]
mod tests;
