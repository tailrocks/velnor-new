// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use serde_json::Value;
use velnor_gen::expected_files;
use velnor_model::{RELEASE_WORKFLOW_PATH, RepoView, WORKFLOWS_CONFIG_PATH};
use velnor_render::DELTA_CLEANUP_PATH;
use velnor_rust::LOCK_PATH;

use crate::base_save::pinned;
use crate::facts::TestResult;
use crate::pipeline::{pin_helper, view, workflow};
use crate::release_fixture::{APP, LOCK, release_view};
use crate::yaml_tree::document;

const HELPERS: &[&str] = &["0.1.109", "0.1.110", "0.1.111"];
const REUSABLE: &str = r#"
[[workflow]]
file = "build-assets"
name = "Build assets"

[[workflow.job]]
id = "build"
permissions = { contents = "read" }

[[workflow.job.step]]
uses = "checkout"

[[workflow.job.step]]
id = "build"
name = "Build"
run = "cargo build --release --locked"

[[workflow.job.step]]
id = "upload"
uses = "upload-artifact"
with = { name = "assets", path = "target/release/app" }
"#;

const CONFIGS: &[Option<&str>] = &[
    None,
    Some("mbx_shared_store = true\n"),
    Some("mbx_shared_store = false\n"),
];

fn strings<'a>(value: &'a Value, out: &mut Vec<&'a str>) {
    match value {
        Value::String(text) => out.push(text),
        Value::Array(items) => items.iter().for_each(|item| strings(item, out)),
        Value::Object(fields) => fields.values().for_each(|field| strings(field, out)),
        _ => {}
    }
}

fn expressions(text: &str) -> Vec<&str> {
    text.split("${{")
        .skip(1)
        .filter_map(|rest| rest.split_once("}}").map(|(expr, _)| expr))
        .collect()
}

fn step_refs(expr: &str) -> Vec<&str> {
    expr.match_indices("steps.")
        .filter(|(at, _)| {
            expr[..*at]
                .chars()
                .last()
                .is_none_or(|before| !(before.is_alphanumeric() || before == '_' || before == '.'))
        })
        .filter_map(|(at, _)| {
            let rest = &expr[at + "steps.".len()..];
            let id_len = rest
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
                .unwrap_or(rest.len());
            (id_len > 0 && rest[id_len..].starts_with('.')).then(|| &rest[..id_len])
        })
        .collect()
}

fn dangling_refs(workflow: &Value) -> Vec<String> {
    let mut dangling = Vec::new();
    let Some(jobs) = workflow["jobs"].as_object() else {
        return dangling;
    };
    for (job_id, job) in jobs {
        let ids: BTreeSet<&str> = job["steps"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|step| step["id"].as_str())
            .collect();
        let mut texts = Vec::new();
        strings(job, &mut texts);
        for text in texts {
            for expr in expressions(text) {
                for id in step_refs(expr) {
                    if !ids.contains(id) {
                        dangling.push(format!("{job_id}: steps.{id} in `{expr}`"));
                    }
                }
            }
        }
    }
    dangling.sort();
    dangling.dedup();
    dangling
}

#[test]
fn every_steps_reference_names_a_step_of_its_own_job() -> TestResult {
    let mut refs = 0;
    for helper in HELPERS {
        for config in CONFIGS.iter().copied() {
            let mut repo = view(
                &["Cargo.toml", "tools/Cargo.toml"],
                config.map(str::as_bytes),
            )?;
            pin_helper(&mut repo, helper)?;
            let yaml = document(&workflow(&repo)?)?;
            let mut texts = Vec::new();
            strings(&yaml, &mut texts);
            refs += texts
                .iter()
                .flat_map(|text| expressions(text))
                .map(|expr| step_refs(expr).len())
                .sum::<usize>();
            let dangling = dangling_refs(&yaml);
            assert!(
                dangling.is_empty(),
                "helper {helper}, config {config:?}: {dangling:#?}"
            );
        }
    }
    assert_ne!(refs, 0, "no steps reference was checked");
    Ok(())
}

#[test]
fn every_file_of_the_cleanup_release_and_reusable_families_names_its_steps() -> TestResult {
    let mut reusable = release_view(&[], None)?;
    reusable
        .files
        .insert(WORKFLOWS_CONFIG_PATH, REUSABLE.as_bytes().to_vec());
    let mut release = release_view(&[APP], Some("roots = [\".\"]\n"))?;
    release.files.insert(LOCK_PATH, LOCK.as_bytes().to_vec());
    let families: [(&str, RepoView); 4] = [
        ("cleanup", pinned("0.1.102", "")?),
        (
            "cleanup-runner",
            pinned("0.1.102", "runner = \"ubuntu-26.04-scale-set\"\n")?,
        ),
        ("release", release),
        ("reusable", reusable),
    ];
    let mut checked = Vec::new();
    let mut refs = 0;
    for (family, repo) in &families {
        for (path, text) in expected_files(repo)? {
            let yaml = document(&text)?;
            let mut texts = Vec::new();
            strings(&yaml, &mut texts);
            refs += texts
                .iter()
                .flat_map(|text| expressions(text))
                .map(|expr| step_refs(expr).len())
                .sum::<usize>();
            let dangling = dangling_refs(&yaml);
            assert!(dangling.is_empty(), "{family} {path}: {dangling:#?}");
            checked.push(path);
        }
    }
    assert!(checked.contains(&DELTA_CLEANUP_PATH), "{checked:?}");
    assert!(checked.contains(&RELEASE_WORKFLOW_PATH), "{checked:?}");
    assert!(
        checked
            .iter()
            .any(|path| path.to_string().ends_with("build-assets.yml")),
        "{checked:?}"
    );
    assert_ne!(refs, 0, "no steps reference was checked");
    Ok(())
}

#[test]
fn the_reference_check_reads_only_expressions_and_only_step_ids() {
    assert_eq!(
        step_refs("steps.toolsanchor.outputs.cache-hit == 'true' && x.steps.nope.y"),
        vec!["toolsanchor"]
    );
    assert_eq!(step_refs("steps.a-b_1.outputs.x"), vec!["a-b_1"]);
    assert_eq!(step_refs("steps.toolsanchor"), Vec::<&str>::new());
    assert_eq!(
        expressions("run $x steps.a.b ${{ steps.c.outputs.d }} tail"),
        vec![" steps.c.outputs.d "]
    );
}
