// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use serde_json::Value;

use crate::facts::TestResult;
use crate::pipeline::workflow;
use crate::single_fetch::repo;
use crate::yaml_tree::document;

const HELPER: &str = "0.1.110";
const LITERAL_HELPER: &str = "0.1.112";

const PLAIN_ROOTS: [&str; 6] = [
    "svc/a-plus+sign-service",
    "svc/b-at@sign-service",
    "svc/c-comma,name-service",
    "svc/d-colon:name-service",
    "svc/e-equals=name-service",
    "svc/f-percent%name-service",
];

const AWKWARD_ROOTS: [&str; 7] = [
    "svc/a-space name-service",
    "svc/b-it's-service",
    "svc/c-glob*star-service",
    "svc/d-dollar$HOME-service",
    "svc/e-brace{a,b}-service",
    "svc/f-bang!hash#-service",
    "svc/g-tilde~question?-service",
];

struct Rendered {
    env: BTreeMap<String, String>,
    runs: Vec<String>,
}

fn rendered(directories: &[&str]) -> Result<Rendered, Box<dyn std::error::Error>> {
    rendered_with(directories, HELPER)
}

fn rendered_with(
    directories: &[&str],
    helper: &str,
) -> Result<Rendered, Box<dyn std::error::Error>> {
    let manifests: Vec<String> = std::iter::once("Cargo.toml".to_owned())
        .chain(
            directories
                .iter()
                .map(|directory| format!("{directory}/Cargo.toml")),
        )
        .collect();
    let names: Vec<&str> = manifests.iter().map(String::as_str).collect();
    let tree = document(&workflow(&repo(&names, helper)?)?)?;
    let env = tree
        .get("env")
        .and_then(Value::as_object)
        .ok_or("no workflow env")?
        .iter()
        .filter(|(name, _)| name.starts_with("VELNOR_SOURCE_ROOTS_"))
        .filter_map(|(name, value)| Some((name.clone(), value.as_str()?.to_owned())))
        .collect();
    let mut shapes = Vec::new();
    for (id, job) in tree
        .get("jobs")
        .and_then(Value::as_object)
        .ok_or("no jobs")?
    {
        let runs: Vec<String> = job
            .get("steps")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|step| {
                step.get("name")
                    .and_then(Value::as_str)
                    .is_some_and(|name| name.starts_with("Fetch the sources of every root"))
            })
            .filter_map(|step| Some(step.get("run")?.as_str()?.to_owned()))
            .collect();
        if !runs.is_empty() {
            shapes.push((id.clone(), runs));
        }
    }
    let (_, runs) = shapes.first().ok_or("no fetch step")?.clone();
    for (id, other) in &shapes {
        assert_eq!(other, &runs, "{id}");
    }
    Ok(Rendered { env, runs })
}

#[test]
fn a_roots_env_list_with_shell_special_but_plain_characters_is_hoisted_into_one_entry() -> TestResult
{
    let workflow = rendered(&PLAIN_ROOTS)?;
    let [(name, value)] = workflow.env.iter().collect::<Vec<_>>()[..] else {
        return Err(format!("one roots entry: {:?}", workflow.env).into());
    };
    assert_eq!(
        value,
        ". ./svc/a-plus+sign-service ./svc/b-at@sign-service ./svc/c-comma,name-service ./svc/d-colon:name-service ./svc/e-equals=name-service ./svc/f-percent%name-service"
    );
    let [run] = &workflow.runs[..] else {
        return Err(format!("one fetch step: {:?}", workflow.runs).into());
    };
    assert!(run.ends_with(&format!("--roots ${name}")), "{run}");
    Ok(())
}

#[test]
fn roots_with_spaces_quotes_globs_or_shell_syntax_are_quoted_word_by_word_in_the_step() -> TestResult
{
    let workflow = rendered(&AWKWARD_ROOTS)?;
    assert!(workflow.env.is_empty(), "{:?}", workflow.env);
    let [run] = &workflow.runs[..] else {
        return Err(format!("one fetch step: {:?}", workflow.runs).into());
    };
    assert!(
        run.ends_with(
            "--roots . './svc/a-space name-service' './svc/b-it'\\''s-service' './svc/c-glob*star-service' './svc/d-dollar$HOME-service' './svc/e-brace{a,b}-service' './svc/f-bang!hash#-service' './svc/g-tilde~question?-service'"
        ),
        "{run}"
    );
    Ok(())
}

#[test]
fn roots_with_command_separators_are_refused_instead_of_expanded() -> TestResult {
    for root in [
        "svc/semi;colon-service",
        "svc/amp&pipe-service",
        "svc/bar|x-service",
    ] {
        let manifests = ["Cargo.toml".to_owned(), format!("{root}/Cargo.toml")];
        let names: Vec<&str> = manifests.iter().map(String::as_str).collect();
        let result = workflow(&repo(&names, HELPER)?);
        assert!(
            result.is_err_and(|error| error.to_string().contains("a run value contains")),
            "{root}"
        );
    }
    Ok(())
}

#[test]
fn a_helper_with_the_literal_roots_argument_never_reads_the_roots_from_the_environment()
-> TestResult {
    let workflow = rendered_with(&PLAIN_ROOTS, LITERAL_HELPER)?;
    assert!(workflow.env.is_empty(), "{:?}", workflow.env);
    let [run] = &workflow.runs[..] else {
        return Err(format!("one fetch step: {:?}", workflow.runs).into());
    };
    assert!(
        run.ends_with(
            "--roots . './svc/a-plus+sign-service' './svc/b-at@sign-service' './svc/c-comma,name-service' './svc/d-colon:name-service' './svc/e-equals=name-service' './svc/f-percent%name-service'"
        ),
        "{run}"
    );
    assert!(!run.contains("VELNOR_SOURCE_ROOTS"), "{run}");
    Ok(())
}

#[test]
fn the_helper_just_below_the_literal_roots_gate_still_hoists_the_roots() -> TestResult {
    let workflow = rendered_with(&PLAIN_ROOTS, "0.1.111")?;
    assert_eq!(workflow.env.len(), 1, "{:?}", workflow.env);
    let [run] = &workflow.runs[..] else {
        return Err(format!("one fetch step: {:?}", workflow.runs).into());
    };
    assert!(run.contains("--roots $VELNOR_SOURCE_ROOTS_"), "{run}");
    Ok(())
}
