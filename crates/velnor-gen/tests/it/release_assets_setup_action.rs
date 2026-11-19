// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::error::Error;
use std::fs;
use std::path::Path;

use serde_json::Value;

use crate::yaml_tree::document;

type TestResult = Result<(), Box<dyn Error>>;

const WORKFLOW: &str = ".github/workflows/release.yml";
const ACTION: &str = "actions/setup-velnor/action.yml";
const SUMS_JOB: &str = "sums-family";
const PUBLISH_JOB: &str = "publish-family";
const SUMS: &str = "SHA256SUMS";
const BINARY_SUMS: &str = "BINARY-SHA256SUMS";
const COMMIT: &str = "COMMIT";
const WRITE_BINARY_SUMS: &str = "sha256sum velnor-ci-";

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

fn build_runs(workflow: &Value) -> Result<Vec<&str>, Box<dyn Error>> {
    let jobs = workflow["jobs"].as_object().ok_or("no jobs")?;
    let mut all = Vec::new();
    for id in jobs.keys().filter(|id| id.starts_with("build-")) {
        all.extend(runs(workflow, id)?);
    }
    Ok(all)
}

fn archives_produced(
    workflow: &Value,
    targets: &BTreeSet<String>,
    expected: &BTreeSet<String>,
) -> Result<BTreeSet<String>, Box<dyn Error>> {
    let built = build_runs(workflow)?;
    let added: BTreeSet<String> = built
        .iter()
        .filter_map(|run| run.strip_prefix("rustup target add "))
        .map(str::to_owned)
        .collect();
    assert_eq!(&added, targets);
    let produced: BTreeSet<String> = built
        .iter()
        .filter(|run| run.starts_with("tar -czf "))
        .filter_map(|run| run.split_whitespace().nth(2))
        .map(str::to_owned)
        .collect();
    assert!(produced.is_superset(expected), "{produced:?} {expected:?}");
    assert_eq!(produced.len(), 2 * targets.len(), "{produced:?}");
    Ok(produced)
}

fn words_before_redirect(run: &str, skip: usize) -> BTreeSet<String> {
    run.split_whitespace()
        .skip(skip)
        .take_while(|word| *word != ">")
        .map(str::to_owned)
        .collect()
}

fn checksums_cover(
    workflow: &Value,
    published: &BTreeSet<String>,
    produced: &BTreeSet<String>,
    expected: &BTreeSet<String>,
) -> TestResult {
    let listed = words_before_redirect(
        run_starting(workflow, SUMS_JOB, "sha256sum ", "> SHA256SUMS")?,
        1,
    );
    assert_eq!(&listed, published);

    let binary = words_before_redirect(
        run_starting(
            workflow,
            SUMS_JOB,
            WRITE_BINARY_SUMS,
            "> ../dist/BINARY-SHA256SUMS",
        )?,
        1,
    );
    for name in expected {
        assert!(binary.contains(&format!("{name}/velnor-ci")), "{binary:?}");
    }
    assert_eq!(binary.len(), produced.len(), "{binary:?}");

    let uploaded = workflow["jobs"][SUMS_JOB]["steps"]
        .as_array()
        .ok_or("the sums job has no steps")?
        .iter()
        .filter(|step| step["name"] == "Upload family-sums")
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
fn the_release_tag_prefix_the_setup_action_tries_first_starts_the_release_workflow() -> TestResult {
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
    let pushed: Vec<&str> = workflow["on"]["push"]["tags"]
        .as_array()
        .ok_or("the workflow does not start on tags")?
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert_eq!(pushed, [prefixes[0].as_str()]);
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

    let script_triples: BTreeSet<String> = between(script, "triple=", ';')
        .into_iter()
        .map(str::to_owned)
        .collect();

    let produced = archives_produced(&workflow, &script_triples, &expected)?;

    let published: BTreeSet<String> =
        run_starting(&workflow, PUBLISH_JOB, "velnor-ci release publish", "")?
            .split_whitespace()
            .skip(3)
            .filter(|word| !word.starts_with("--"))
            .map(str::to_owned)
            .collect();
    let wanted: BTreeSet<String> = produced
        .iter()
        .cloned()
        .chain([BINARY_SUMS, COMMIT].map(str::to_owned))
        .collect();
    assert_eq!(published, wanted);

    checksums_cover(&workflow, &published, &produced, &expected)?;

    Ok(())
}
