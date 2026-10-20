//! HD-4a: the JDK resolver function accepts exactly the adapter grammar.
//!
//! The generated resolver defines `velnor_exact_java`; this test extracts it
//! (with the helpers it uses) from the generated script and runs it with `sh`
//! over an accept corpus and an injection/looseness reject corpus.

use std::collections::BTreeSet;
use std::process::Command;

use velnor_actions_contract::config::project_tool_resolver::{
    JAVA_TOOL_VARIABLE, project_tool_resolver_script,
};

/// The shell definitions `velnor_exact_java` needs, cut from the real script.
fn harness() -> Result<String, String> {
    let script =
        project_tool_resolver_script(&BTreeSet::from([JAVA_TOOL_VARIABLE.to_owned()]), false);
    let between = |from: &str, to: &str| -> Result<String, String> {
        let start = script.find(from).ok_or_else(|| format!("missing {from}"))?;
        let end = start
            + script[start..]
                .find(to)
                .ok_or_else(|| format!("missing {to}"))?;
        Ok(script[start..end].to_owned())
    };
    let safe = between("velnor_safe()", "\n")? + "\n";
    let fail = between("velnor_fail()", "\n")? + "\n";
    let exact = between("velnor_exact() {", "velnor_tool() {")?;
    let java = between("velnor_exact_java() {", "velnor_tool_java() {")?;
    Ok(format!("{safe}{fail}{exact}{java}"))
}

fn accepts(harness: &str, value: &str) -> bool {
    Command::new("sh")
        .arg("-c")
        .arg(format!("{harness}\nvelnor_exact_java declared \"$1\""))
        .arg("sh")
        .arg(value)
        .output()
        .expect("sh")
        .status
        .success()
}

#[test]
fn accepts_the_documented_forms() {
    let harness = harness().expect("harness");
    for ok in [
        "oracle-graalvm-25.0.3",
        "temurin-21.0.8+9.0.LTS",
        "temurin-21.0.8+9",
        "corretto-17.0.12",
        "zulu-21.0.4",
        "graalvm-community-21.0.2+13.1",
    ] {
        assert!(accepts(&harness, ok), "{ok}");
    }
}

#[test]
fn rejects_loose_unknown_and_hostile_declarations() {
    let harness = harness().expect("harness");
    for bad in [
        "21",
        "temurin-21",
        "lts",
        "latest",
        "temurin-latest",
        "oracle-graalvm-25",
        "oracle-graalvm-25.0",
        "oracle-graalvm-25.0.03",
        "openjdk-21.0.1",
        "temurin-21.0.8+",
        "temurin-21.0.8+x",
        "temurin-21.0.8+9..1",
        "temurin-^21.0.8",
        "temurin-21.0.x",
        "temurin-21.0.8 ",
        "Temurin-21.0.8",
        "temurin-21.0.8;",
        "temurin-21.0.8$(id)",
        "temurin-21.0.8\nFOO=1",
        "",
    ] {
        assert!(!accepts(&harness, bad), "{bad:?}");
    }
}

#[test]
fn the_java_functions_exist_only_for_jobs_that_reference_the_jdk() {
    let without =
        project_tool_resolver_script(&BTreeSet::from(["VELNOR_TOOL_RUST".to_owned()]), false);
    assert!(!without.contains("velnor_exact_java"));
    let with =
        project_tool_resolver_script(&BTreeSet::from([JAVA_TOOL_VARIABLE.to_owned()]), false);
    assert!(with.contains("velnor_tool_java VELNOR_TOOL_JAVA 'java' 'core:java'"));
}
