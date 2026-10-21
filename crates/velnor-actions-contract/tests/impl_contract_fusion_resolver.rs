//! HD-4a: the dbt-fusion resolver function accepts exactly `X.Y.Z-preview.N` or stable `X.Y.Z`.
//!
//! The generated resolver defines `velnor_exact_fusion`; this test extracts it
//! (with the helpers it uses) from the generated script and runs it with `sh`
//! over an accept corpus and an injection/looseness reject corpus.

use std::collections::BTreeSet;
use std::process::Command;

use velnor_actions_contract::config::project_tool_resolver::{
    DBT_FUSION_TOOL_VARIABLE, project_tool_resolver_script,
};

/// The shell definitions `velnor_exact_fusion` needs, cut from the real script.
fn harness() -> Result<String, String> {
    let script = project_tool_resolver_script(
        &BTreeSet::from([DBT_FUSION_TOOL_VARIABLE.to_owned()]),
        false,
    );
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
    let fusion = between("velnor_exact_fusion() {", "velnor_tool_fusion() {")?;
    Ok(format!("{safe}{fail}{exact}{fusion}"))
}

fn accepts(harness: &str, value: &str) -> Result<bool, std::io::Error> {
    Ok(Command::new("sh")
        .arg("-c")
        .arg(format!("{harness}\nvelnor_exact_fusion declared \"$1\""))
        .arg("sh")
        .arg(value)
        .output()?
        .status
        .success())
}

#[test]
fn accepts_the_documented_forms() {
    let harness = harness().expect("harness");
    for ok in [
        "2.0.0-preview.219",
        "2.10.3-preview.0",
        "0.1.0-preview.7",
        "10.20.30-preview.1000",
        "2.0.9",
        "10.20.30",
    ] {
        assert!(accepts(&harness, ok).expect("sh"), "{ok}");
    }
}

#[test]
fn rejects_loose_unknown_and_hostile_declarations() {
    let harness = harness().expect("harness");
    for bad in [
        "latest",
        "2.0.9-",
        "2.0.9-rc.1",
        "2.0.09",
        "2.0.9.1",
        "2.0.*",
        "2.0.9;",
        "2.0.9$(id)",
        "2.0.9\nFOO=1",
        "~2.0.9",
        "2",
        "2.0",
        "2.0.0-preview",
        "2.0.0-preview.",
        "2.0.0-preview.01",
        "2.0.0-preview.x",
        "2.0.0-beta.1",
        "02.0.0-preview.1",
        "2.0.0-preview.1.2",
        "^2.0.0-preview.1",
        "v2.0.0-preview.1",
        "2.0.0-preview.1 ",
        "2.0.0-preview.1;",
        "2.0.0-preview.1$(id)",
        "2.0.0-preview.1\nFOO=1",
        "{{version}}",
        "",
    ] {
        assert!(!accepts(&harness, bad).expect("sh"), "{bad:?}");
    }
}

#[test]
fn the_fusion_functions_exist_only_for_jobs_that_reference_it() {
    let without =
        project_tool_resolver_script(&BTreeSet::from(["VELNOR_TOOL_RUST".to_owned()]), false);
    assert!(!without.contains("velnor_exact_fusion"));
    let java =
        project_tool_resolver_script(&BTreeSet::from(["VELNOR_TOOL_JAVA".to_owned()]), false);
    assert!(!java.contains("velnor_exact_fusion") && java.contains("velnor_exact_java"));
    let with = project_tool_resolver_script(
        &BTreeSet::from([DBT_FUSION_TOOL_VARIABLE.to_owned()]),
        false,
    );
    assert!(with.contains(
        "velnor_tool_fusion VELNOR_TOOL_DBT_FUSION 'dbt-fusion' 'aqua:getdbt.com/dbt-fusion'"
    ));
}
