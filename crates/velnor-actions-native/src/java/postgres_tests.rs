use super::*;

#[test]
fn initialization_owns_client_vectors_and_local_binding_values() -> Result<(), ContractError> {
    let fixture = PostgresFixture {
        user: "fixture".to_owned(),
        password: "fixture_password".to_owned(),
        databases: vec!["primary".to_owned(), "secondary".to_owned()],
        bindings: BTreeMap::from([
            ("APP_DB_HOST".to_owned(), PostgresBinding::Host),
            ("APP_DB_PORT".to_owned(), PostgresBinding::Port),
            ("APP_DB_USERNAME".to_owned(), PostgresBinding::User),
            ("APP_DB_PASSWORD".to_owned(), PostgresBinding::Password),
            (
                "APP_DATASOURCE_URL".to_owned(),
                PostgresBinding::JdbcUrl {
                    database: "secondary".to_owned(),
                },
            ),
        ]),
    };
    let initialization = postgres_initialization(&fixture)?;
    assert_eq!(initialization.initial_database, "primary");
    assert_eq!(initialization.environment.len(), 3);
    assert_eq!(initialization.environment["POSTGRES_USER"], "fixture");
    assert_eq!(
        initialization.environment["POSTGRES_PASSWORD"],
        "fixture_password"
    );
    assert_eq!(
        initialization.create_database_argv,
        [vec![
            "psql",
            "--username",
            "fixture",
            "--dbname",
            "primary",
            "--set",
            "ON_ERROR_STOP=1",
            "--command",
            "CREATE DATABASE \"secondary\";",
        ]]
    );
    assert!(
        initialization
            .bindings
            .iter()
            .any(|binding| binding.name == "APP_DB_HOST"
                && binding.value == FixtureValue::Literal("127.0.0.1".to_owned()))
    );
    assert!(
        initialization
            .bindings
            .iter()
            .any(|binding| binding.name == "APP_DB_PORT" && binding.value == FixtureValue::Port)
    );
    assert!(
        initialization
            .bindings
            .iter()
            .any(|binding| binding.name == "APP_DATASOURCE_URL"
                && binding.value == FixtureValue::JdbcUrl("secondary".to_owned()))
    );
    Ok(())
}

#[test]
fn raw_sql_identifiers_and_reserved_process_controls_are_rejected() {
    let mut fixture = PostgresFixture {
        user: "fixture".to_owned(),
        password: "fixture_password".to_owned(),
        databases: vec!["primary;DROP_DATABASE".to_owned()],
        bindings: BTreeMap::from([(
            "APP_DATASOURCE_URL".to_owned(),
            PostgresBinding::JdbcUrl {
                database: "primary;DROP_DATABASE".to_owned(),
            },
        )]),
    };
    assert!(postgres_initialization(&fixture).is_err());
    fixture.databases = vec!["primary".to_owned()];
    fixture.bindings = BTreeMap::from([("JAVA_HOME".to_owned(), PostgresBinding::Host)]);
    assert!(postgres_initialization(&fixture).is_err());
}
