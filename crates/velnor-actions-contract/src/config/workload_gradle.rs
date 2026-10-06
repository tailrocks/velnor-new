//! Closed Gradle check goals and local PostgreSQL fixtures.
use super::{WorkloadConfig, WorkloadKind};
use crate::errors::ContractError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Gradle project and optional isolated database prerequisite.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GradleWorkloadConfig {
    /// Canonical colon-prefixed project selector; absent means root project.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    /// Explicit local fixture for migration and code-generation checks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub postgres: Option<PostgresFixture>,
}

/// Isolated local database fixture, never an external connection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PostgresFixture {
    /// Fixture user required by the checked source.
    pub user: String,
    /// Local fixture password, never a secret expression or external credential.
    pub password: String,
    /// Sorted unique databases to create.
    pub databases: Vec<String>,
    /// Explicit environment names receiving closed fixture values.
    pub bindings: BTreeMap<String, PostgresBinding>,
}

/// Closed values that may be bound to the Gradle process environment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PostgresBinding {
    /// Local loopback host.
    Host,
    /// Dynamically assigned local port.
    Port,
    /// Configured fixture user.
    User,
    /// Configured local fixture password.
    Password,
    /// Local JDBC URL for a declared database.
    JdbcUrl {
        /// Declared fixture database.
        database: String,
    },
    /// Schema identifier consumed by the source migration.
    Schema {
        /// Safe schema identifier.
        schema: String,
    },
}

fn identifier(value: &str) -> bool {
    let mut bytes = value.bytes();
    matches!(bytes.next(), Some(b'a'..=b'z' | b'_'))
        && bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

fn project_selector(project: &str) -> bool {
    project.strip_prefix(':').is_some_and(|rest| {
        rest.split(':').all(|part| {
            !part.is_empty()
                && part != "."
                && part != ".."
                && !part.starts_with('-')
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        })
    })
}

fn binding_name(name: &str, binding: &PostgresBinding) -> bool {
    let reserved = [
        "GITHUB_", "RUNNER_", "VELNOR_", "MISE_", "CARGO_", "RUSTUP_", "JAVA_", "GRADLE_", "LD_",
        "DYLD_",
    ];
    if reserved.iter().any(|prefix| name.starts_with(prefix)) {
        return false;
    }
    let suffixes: &[&str] = match binding {
        PostgresBinding::Host => &["_DB_HOST"],
        PostgresBinding::Port => &["_DB_PORT"],
        PostgresBinding::User => &["_DATASOURCE_USERNAME", "_DB_USERNAME", "_DB_USER"],
        PostgresBinding::Password => &["_DATASOURCE_PASSWORD", "_DB_PASSWORD"],
        PostgresBinding::JdbcUrl { .. } => &["_DATASOURCE_URL", "_DB_URL"],
        PostgresBinding::Schema { .. } => &["_DATASOURCE_SCHEMA", "_DB_SCHEMA"],
    };
    suffixes.iter().any(|suffix| {
        name.strip_suffix(suffix)
            .is_some_and(|prefix| !prefix.is_empty())
    })
}

impl PostgresFixture {
    /// Validate local fixture literals and environment bindings.
    /// # Errors
    pub fn validate(&self, file: &str, key: &str) -> Result<(), ContractError> {
        let bad = || ContractError::config(file, key, "invalid_postgres_fixture");
        if !identifier(&self.user)
            || self.password.is_empty()
            || !self
                .password
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
            || self.databases.is_empty()
            || self.databases.iter().any(|database| !identifier(database))
            || self.databases.windows(2).any(|pair| pair[0] >= pair[1])
        {
            return Err(bad());
        }
        for (name, binding) in &self.bindings {
            let mut bytes = name.bytes();
            if !matches!(bytes.next(), Some(b'A'..=b'Z' | b'_'))
                || !bytes
                    .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
                || !binding_name(name, binding)
            {
                return Err(bad());
            }
            match binding {
                PostgresBinding::JdbcUrl { database } if !self.databases.contains(database) => {
                    return Err(bad());
                }
                PostgresBinding::Schema { schema } if !identifier(schema) => return Err(bad()),
                _ => {}
            }
        }
        let jdbc = self
            .bindings
            .values()
            .any(|binding| matches!(binding, PostgresBinding::JdbcUrl { .. }));
        let host = self
            .bindings
            .values()
            .any(|binding| matches!(binding, PostgresBinding::Host));
        let port = self
            .bindings
            .values()
            .any(|binding| matches!(binding, PostgresBinding::Port));
        if !(jdbc || (host && port)) {
            return Err(bad());
        }
        Ok(())
    }
}

impl WorkloadConfig {
    /// Require closed descriptors only for their owning Gradle operations.
    /// # Errors
    pub(super) fn validate_gradle(&self, file: &str) -> Result<(), ContractError> {
        let key = format!("stacks.workloads.{}.gradle", self.name);
        let owns = matches!(
            self.kind,
            WorkloadKind::GradleCheck | WorkloadKind::GradleDatabaseCheck
        );
        let Some(gradle) = &self.gradle else {
            return if owns {
                Err(ContractError::config(
                    file,
                    key,
                    "missing_gradle_descriptor",
                ))
            } else {
                Ok(())
            };
        };
        if !owns
            || gradle
                .project
                .as_deref()
                .is_some_and(|value| !project_selector(value))
        {
            return Err(ContractError::config(
                file,
                key,
                "invalid_gradle_descriptor",
            ));
        }
        if let Some(postgres) = &gradle.postgres {
            postgres.validate(file, &key)?;
        } else if self.kind == WorkloadKind::GradleDatabaseCheck {
            return Err(ContractError::config(file, key, "missing_postgres_fixture"));
        }
        Ok(())
    }
}
