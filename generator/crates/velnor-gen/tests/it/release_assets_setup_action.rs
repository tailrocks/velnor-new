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
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
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
    assert_eq!(triples.len(), 3, "{triples:?}");
    Ok(triples
        .into_iter()
        .map(|triple| template.replace("$triple", triple))
        .collect())
}

struct Release {
    expected: BTreeSet<String>,
    produced: BTreeSet<String>,
    published: BTreeSet<String>,
    workflow: Value,
}

fn release() -> Result<Release, Box<dyn Error>> {
    let action = parse(ACTION)?;
    let workflow = parse(WORKFLOW)?;
    let script = action_script(&action)?;
    let expected = setup_archives(script)?;
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
    let published = names(
        run_starting(&workflow, "release", "gh release create", "")?,
        "dist/",
    );
    Ok(Release {
        expected,
        produced,
        published,
        workflow,
    })
}

fn listed(run: &str) -> BTreeSet<String> {
    run.split_whitespace()
        .skip(1)
        .take_while(|word| *word != ">")
        .map(str::to_owned)
        .collect()
}

#[test]
fn the_release_publishes_every_archive_the_setup_action_downloads() -> TestResult {
    let release = release()?;
    assert!(
        release
            .expected
            .iter()
            .all(|name| name.starts_with("velnor-ci-") && name.ends_with(".tar.gz")),
        "{:?}",
        release.expected
    );
    assert!(
        release.produced.is_superset(&release.expected),
        "{:?} {:?}",
        release.produced,
        release.expected
    );
    let wanted: BTreeSet<String> = release
        .produced
        .iter()
        .cloned()
        .chain([SUMS, BINARY_SUMS, COMMIT].map(str::to_owned))
        .collect();
    assert_eq!(release.published, wanted);
    Ok(())
}

#[test]
fn sha256sums_lists_every_published_file_and_the_binary_sums_list_every_member() -> TestResult {
    let release = release()?;
    let sums = listed(run_starting(
        &release.workflow,
        "checksums",
        "sha256sum ",
        "> SHA256SUMS",
    )?);
    let mut without_sums = release.published.clone();
    without_sums.remove(SUMS);
    assert_eq!(sums, without_sums);

    let binary = listed(run_starting(
        &release.workflow,
        "checksums",
        "sha256sum ",
        "> ../dist/BINARY-SHA256SUMS",
    )?);
    for name in &release.expected {
        assert!(binary.contains(&format!("{name}/velnor-ci")), "{binary:?}");
    }
    assert_eq!(binary.len(), release.produced.len(), "{binary:?}");

    let uploaded = release.workflow["jobs"]["checksums"]["steps"]
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
