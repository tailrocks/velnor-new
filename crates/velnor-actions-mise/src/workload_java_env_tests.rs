use super::*;
use std::collections::BTreeMap;
use std::process::Command;
use velnor_actions_contract::config::PostgresBinding;

#[test]
fn child_drops_java_options_credentials_and_unbound_database_values()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = PostgresFixture {
        user: "fixture".to_owned(),
        password: "fixture_password".to_owned(),
        databases: vec!["fixture".to_owned()],
        bindings: BTreeMap::from([
            ("TEST_DB_HOST".to_owned(), PostgresBinding::Host),
            ("TEST_DB_PORT".to_owned(), PostgresBinding::Port),
            (
                "TEST_DATASOURCE_URL".to_owned(),
                PostgresBinding::JdbcUrl {
                    database: "fixture".to_owned(),
                },
            ),
        ]),
    };
    let script = format!("{} /usr/bin/env", java_isolation_prefix(Some(&fixture))?);
    let mut command = Command::new("sh");
    command.args(["-c", &script]).env_clear();
    for (key, value) in [
        ("PATH", "/usr/bin:/bin"),
        ("HOME", "/tmp/fixture-home"),
        ("RUNNER_TEMP", "/tmp/fixture-runner"),
        ("MISE_DATA_DIR", "/tmp/fixture-mise"),
        ("TEST_DB_HOST", "127.0.0.1"),
        ("TEST_DB_PORT", "23456"),
        (
            "TEST_DATASOURCE_URL",
            "jdbc:postgresql://127.0.0.1:23456/fixture",
        ),
    ] {
        command.env(key, value);
    }
    for key in [
        "JAVA_HOME",
        "JAVA_OPTS",
        "JAVA_TOOL_OPTIONS",
        "JDK_JAVA_OPTIONS",
        "_JAVA_OPTIONS",
        "GRADLE_OPTS",
        "GRADLE_USER_HOME",
        "GITHUB_TOKEN",
        "NPM_TOKEN",
        "AWS_ACCESS_KEY_ID",
        "AWS_SECRET_ACCESS_KEY",
        "EXTERNAL_DATASOURCE_URL",
        "BASH_ENV",
        "ENV",
        "LD_PRELOAD",
        "HTTPS_PROXY",
    ] {
        command.env(key, "hostile-value");
    }
    let output = command.output()?;
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout)?;
    assert!(!text.contains("hostile-value"));
    let actual: BTreeMap<_, _> = text
        .lines()
        .map(|line| {
            let (key, value) = line.split_once('=').expect("environment entry");
            (key.to_owned(), value.to_owned())
        })
        .collect();
    assert_eq!(
        actual["GRADLE_USER_HOME"],
        "/tmp/fixture-runner/velnor/native/gradle"
    );
    assert_eq!(actual["TEST_DB_HOST"], "127.0.0.1");
    assert_eq!(actual["TEST_DB_PORT"], "23456");
    assert_eq!(
        actual["TEST_DATASOURCE_URL"],
        "jdbc:postgresql://127.0.0.1:23456/fixture"
    );
    assert!(!actual.contains_key("JAVA_HOME"));
    Ok(())
}

#[test]
fn bindings_are_validated_and_missing_runtime_value_fails() -> Result<(), Box<dyn std::error::Error>>
{
    let mut fixture = PostgresFixture {
        user: "fixture".to_owned(),
        password: "fixture_password".to_owned(),
        databases: vec!["fixture".to_owned()],
        bindings: BTreeMap::from([
            ("TEST_DB_HOST".to_owned(), PostgresBinding::Host),
            ("TEST_DB_PORT".to_owned(), PostgresBinding::Port),
        ]),
    };
    let output = Command::new("sh")
        .args([
            "-c",
            &format!("{} /usr/bin/env", java_isolation_prefix(Some(&fixture))?),
        ])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .output()?;
    assert!(!output.status.success());
    fixture.bindings = BTreeMap::from([("JAVA_HOME".to_owned(), PostgresBinding::Host)]);
    assert!(java_isolation_prefix(Some(&fixture)).is_err());
    Ok(())
}
