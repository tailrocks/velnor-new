//! Closed initialization and process bindings for a local PostgreSQL fixture.
use std::collections::BTreeMap;
use velnor_actions_contract::{
    ContractError,
    config::{PostgresBinding, PostgresFixture},
};

/// Local fixture semantics; container launch and lifecycle belong to orchestration.
#[derive(Debug, PartialEq, Eq)]
pub struct DatabaseInitialization {
    /// Bootstrap database created by the official image.
    pub initial_database: String,
    /// Only local fixture values enter the container environment.
    pub environment: BTreeMap<&'static str, String>,
    /// Fixed PostgreSQL client vectors for the remaining declared databases.
    pub create_database_argv: Vec<Vec<String>>,
    /// Explicit process bindings, derived only from the local fixture.
    pub bindings: Vec<FixtureBinding>,
}

/// One validated process environment name and its closed local value.
#[derive(Debug, PartialEq, Eq)]
pub struct FixtureBinding {
    /// Validated database environment binding name.
    pub name: String,
    /// Local source of its runtime value.
    pub value: FixtureValue,
}

/// Runtime values never select remote endpoints or external credentials.
#[derive(Debug, PartialEq, Eq)]
pub enum FixtureValue {
    /// Fixed local fixture literal.
    Literal(String),
    /// Dynamic loopback port assigned by the container lifecycle.
    Port,
    /// JDBC URL using that local port and the declared database.
    JdbcUrl(String),
}

/// Compile typed fixture configuration into fixed database initialization semantics.
/// # Errors
/// Rejects invalid local fixture identities, values, or environment bindings.
pub fn postgres_initialization(
    fixture: &PostgresFixture,
) -> Result<DatabaseInitialization, ContractError> {
    fixture.validate("native_gradle", "postgres")?;
    let first = fixture
        .databases
        .first()
        .ok_or_else(|| super::invalid("gradle_postgres_database_inventory_empty"))?;
    let create_database_argv = fixture
        .databases
        .iter()
        .skip(1)
        .map(|database| {
            vec![
                "psql".to_owned(),
                "--username".to_owned(),
                fixture.user.clone(),
                "--dbname".to_owned(),
                first.clone(),
                "--set".to_owned(),
                "ON_ERROR_STOP=1".to_owned(),
                "--command".to_owned(),
                format!("CREATE DATABASE \"{database}\";"),
            ]
        })
        .collect();
    let bindings = fixture
        .bindings
        .iter()
        .map(|(name, binding)| FixtureBinding {
            name: name.clone(),
            value: match binding {
                PostgresBinding::Host => FixtureValue::Literal("127.0.0.1".to_owned()),
                PostgresBinding::Port => FixtureValue::Port,
                PostgresBinding::User => FixtureValue::Literal(fixture.user.clone()),
                PostgresBinding::Password => FixtureValue::Literal(fixture.password.clone()),
                PostgresBinding::JdbcUrl { database } => FixtureValue::JdbcUrl(database.clone()),
                PostgresBinding::Schema { schema } => FixtureValue::Literal(schema.clone()),
            },
        })
        .collect();
    Ok(DatabaseInitialization {
        initial_database: first.clone(),
        environment: BTreeMap::from([
            ("POSTGRES_USER", fixture.user.clone()),
            ("POSTGRES_PASSWORD", fixture.password.clone()),
            ("POSTGRES_DB", first.clone()),
        ]),
        create_database_argv,
        bindings,
    })
}

#[cfg(test)]
#[path = "postgres_tests.rs"]
mod tests;
