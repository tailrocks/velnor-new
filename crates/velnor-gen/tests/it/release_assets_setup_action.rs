// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::error::Error;
use std::fs;
use std::path::Path;

use serde_json::Value;

use crate::yaml_tree::document;

type TestResult = Result<(), Box<dyn Error>>;

const WORKFLOW: &str = ".github/workflows/release-rust-ci-tools.yml";
const ACTION: &str = "actions/setup-velnor/action.yml";
const SUMS: &str = "SHA256SUMS";
const BINARY_SUMS: &str = "BINARY-SHA256SUMS";
const COMMIT: &str = "COMMIT";

fn parse(path: &str) -> Result<Value, Box<dyn Error>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    Ok(document(&fs::read_to_string(root.join(path))?)?)
}

fn runs<'a>(workflow: &'a Value, job: &str) -> Result<Vec<&'a str>, Box<dyn Error>> {
    let steps = workflow["jobs"][job]["steps"]
        .as_array()
        .ok_or_else(|| format!("no steps in job {job}"))?;
    Ok(steps
        .iter()
        .filter_map(|step| step["run"].as_str())
        .collect())
}

fn run_starting<'a>(
    workflow: &'a Value,
    job: &str,
    prefix: &str,
    suffix: &str,
) -> Result<&'a str, Box<dyn Error>> {
    runs(workflow, job)?
        .into_iter()
        .find(|run| run.starts_with(prefix) && run.ends_with(suffix))
        .ok_or_else(|| format!("job {job} has no run of {prefix} ... {suffix}").into())
}

fn names(run: &str, strip: &str) -> BTreeSet<String> {
    run.split_whitespace()
        .filter_map(|word| word.trim_matches('"').strip_prefix(strip))
        .map(str::to_owned)
        .collect()
}

fn action_script(action: &Value) -> Result<&str, Box<dyn Error>> {
    action["runs"]["steps"]
        .as_array()
        .and_then(|steps| {
            steps
                .iter()
                .find(|step| step["name"] == "Install velnor-ci")
        })
        .and_then(|step| step["run"].as_str())
        .ok_or_else(|| "the action has no Install velnor-ci step".into())
}

fn between<'a>(text: &'a str, open: &str, close: char) -> Vec<&'a str> {
    text.split(open)
        .skip(1)
        .filter_map(|rest| rest.split(close).next())
        .collect()
}

fn setup_archives(script: &str) -> Result<BTreeSet<String>, Box<dyn Error>> {
    let template = between(script, "archive=\"", '"')
        .first()
        .copied()
        .ok_or("the action does not name its archive")?;
    let triples: BTreeSet<&str> = between(script, "triple=", ';').into_iter().collect();
    assert_eq!(triples.len(), 2, "{triples:?}");
    Ok(triples
        .into_iter()
        .map(|triple| template.replace("$triple", triple))
        .collect())
}

fn archives_produced(
    workflow: &Value,
    targets: &BTreeSet<String>,
    expected: &BTreeSet<String>,
) -> Result<BTreeSet<String>, Box<dyn Error>> {
    let archiving: Vec<&Value> = workflow["jobs"]["build"]["steps"]
        .as_array()
        .ok_or("the build job has no steps")?
        .iter()
        .filter(|step| {
            step["run"]
                .as_str()
                .is_some_and(|run| run.starts_with("tar "))
        })
        .collect();
    assert_eq!(archiving.len(), 2);
    assert!(
        archiving
            .iter()
            .all(|step| step["env"]["TARGET"] == "${{ matrix.target }}")
    );
    let produced: BTreeSet<String> = archiving
        .iter()
        .filter_map(|step| step["run"].as_str())
        .flat_map(|run| names(run, "dist/"))
        .flat_map(|name| {
            targets
                .iter()
                .map(move |target| name.replace("$TARGET", target))
        })
        .collect();
    assert!(produced.is_superset(expected), "{produced:?} {expected:?}");
    assert_eq!(produced.len(), 2 * targets.len(), "{produced:?}");

    Ok(produced)
}

fn checksums_cover(
    workflow: &Value,
    published: &BTreeSet<String>,
    produced: &BTreeSet<String>,
    expected: &BTreeSet<String>,
) -> TestResult {
    let listed: BTreeSet<String> = runs(workflow, "checksums")?
        .into_iter()
        .find(|run| run.starts_with("sha256sum ") && run.ends_with("> SHA256SUMS"))
        .map(|run| {
            run.split_whitespace()
                .skip(1)
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .ok_or("the checksums job never writes SHA256SUMS")?
        .into_iter()
        .take_while(|word| word != ">")
        .collect();
    let mut without_sums = published.clone();
    without_sums.remove(SUMS);
    assert_eq!(listed, without_sums);

    let binary: BTreeSet<String> = run_starting(
        workflow,
        "checksums",
        "sha256sum ",
        "> ../dist/BINARY-SHA256SUMS",
    )?
    .split_whitespace()
    .skip(1)
    .take_while(|word| *word != ">")
    .map(str::to_owned)
    .collect();
    for name in expected {
        assert!(binary.contains(&format!("{name}/velnor-ci")), "{binary:?}");
    }
    assert_eq!(binary.len(), produced.len(), "{binary:?}");

    let uploaded = workflow["jobs"]["checksums"]["steps"]
        .as_array()
        .ok_or("the checksums job has no steps")?
        .iter()
        .filter(|step| step["name"] == "Upload SHA256SUMS")
        .filter_map(|step| step["with"]["path"].as_str())
        .flat_map(|path| names(&path.replace('\n', " "), "dist/"))
        .collect::<BTreeSet<_>>();
    assert_eq!(
        uploaded,
        [SUMS, BINARY_SUMS, COMMIT].map(str::to_owned).into()
    );
    Ok(())
}

#[test]
fn the_release_tag_prefixes_the_setup_action_tries_start_the_release_workflow() -> TestResult {
    let action = parse(ACTION)?;
    let workflow = parse(WORKFLOW)?;
    let script = action_script(&action)?;
    let line = script
        .lines()
        .find(|line| line.contains("for tag in"))
        .ok_or("the action tries no release tags")?;
    let prefixes: Vec<String> = line
        .split('"')
        .filter_map(|piece| piece.strip_suffix("$VERSION"))
        .map(|prefix| format!("{prefix}*"))
        .collect();
    assert_eq!(prefixes, ["tools-v*", "rust-ci-v*"]);
    let pushed: BTreeSet<&str> = workflow["on"]["push"]["tags"]
        .as_array()
        .ok_or("the workflow does not start on tags")?
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert!(
        prefixes
            .iter()
            .all(|prefix| pushed.contains(prefix.as_str()))
    );
    Ok(())
}

#[test]
fn the_release_publishes_every_archive_the_setup_action_downloads_with_its_checksums() -> TestResult
{
    let action = parse(ACTION)?;
    let workflow = parse(WORKFLOW)?;
    let script = action_script(&action)?;
    let expected = setup_archives(script)?;
    assert!(
        expected
            .iter()
            .all(|name| name.starts_with("velnor-ci-") && name.ends_with(".tar.gz")),
        "{expected:?}"
    );

    let targets: BTreeSet<String> = workflow["jobs"]["build"]["strategy"]["matrix"]["include"]
        .as_array()
        .ok_or("the build job has no matrix")?
        .iter()
        .filter_map(|entry| entry["target"].as_str().map(str::to_owned))
        .collect();
    let script_triples: BTreeSet<String> = between(script, "triple=", ';')
        .into_iter()
        .map(str::to_owned)
        .collect();
    assert_eq!(targets, script_triples);

    let produced = archives_produced(&workflow, &targets, &expected)?;

    let published = names(
        run_starting(&workflow, "release", "gh release create", "")?,
        "dist/",
    );
    let wanted: BTreeSet<String> = produced
        .iter()
        .cloned()
        .chain([SUMS, BINARY_SUMS, COMMIT].map(str::to_owned))
        .collect();
    assert_eq!(published, wanted);

    checksums_cover(&workflow, &published, &produced, &expected)?;

    Ok(())
}
