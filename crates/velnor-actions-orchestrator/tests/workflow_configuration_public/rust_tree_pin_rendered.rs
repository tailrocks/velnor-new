// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::error::Error;

use sha2::{Digest, Sha256};
use velnor_actions_contract::WorkflowPolicy;
use velnor_actions_mise::{RUST_TARGET_TRIPLE, RUST_VERSION};
use velnor_actions_orchestrator::{prepare, render_staged_tree};
use velnor_actions_workflow_renderer::verifier_builders::RUST_TREE_DIGESTS;

use super::maintenance_features::owner_feature_repo;
use super::support::rendered_job_block;
use super::tools_producers::POLICIES;
use super::tools_producers_donors::SCHEDULED_SUITES_CONFIG;
use super::waste_lint::CI_PATH;
use super::waste_oracle::{Files, files_of, job_ids};
use super::waste_verifier_oracle::{StepText, steps_of};
use super::workflow_configuration_fixture::make_repo;
use velnor_actions_contract::workflow::state_seal::STATE_SEAL_WORKFLOW_PATH;

#[path = "rust_tree_pin_rendered_shell.rs"]
mod shell;
use shell::{Command, commands_of};

const FRESHNESS_PATH: &str = ".github/workflows/freshness.yml";
const SUITES_PATH: &str = ".github/workflows/suites.yml";
const INSTALLED: [&str; 2] = [
    "Verify Rust channel manifest",
    "velnor-verify-rust-manifest",
];
const RESTORED: [&str; 2] = [
    "Verify restored Rust toolchain",
    "velnor-verify-restored-rust",
];
const LOCAL_ACTIONS: &str = "./.github/actions/";
const MAX_COMPOSITE_DEPTH: usize = 3;
const MINIMAL: [&str; 3] = ["cargo", "rustc", "rust-std"];
const DEFAULT: [&str; 6] = [
    "cargo",
    "rustc",
    "rust-std",
    "rust-docs",
    "clippy-preview",
    "rustfmt-preview",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Installed,
    Restored,
}

#[derive(Debug)]
struct Event {
    labels: Vec<String>,
    run: String,
}

#[derive(Debug, Default)]
struct Toolchain {
    base: Option<&'static [&'static str]>,
    added: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Finding {
    file: String,
    job: String,
    detail: String,
}

#[derive(Debug, Default)]
struct Report {
    observed: BTreeSet<String>,
    checked: BTreeSet<(String, String)>,
    with_adds: BTreeSet<(String, String)>,
    restored: usize,
    findings: Vec<Finding>,
}

fn pin_markers() -> Vec<String> {
    RUST_TREE_DIGESTS
        .iter()
        .map(|pin| format!("pin={}", pin.tree_sha256()))
        .collect()
}

fn pinned_components() -> BTreeSet<String> {
    RUST_TREE_DIGESTS
        .iter()
        .filter(|pin| pin.version() == RUST_VERSION && pin.target() == RUST_TARGET_TRIPLE)
        .map(|pin| pin.components_sha256().to_owned())
        .collect()
}

fn flatten(
    files: &Files,
    steps: Vec<StepText>,
    labels: &[String],
    depth: usize,
) -> Result<Vec<Event>, Box<dyn Error>> {
    let mut events = Vec::new();
    for step in steps {
        let mut here = labels.to_vec();
        here.push(step.name.clone());
        let Some(action) = step
            .uses
            .as_deref()
            .and_then(|uses| uses.strip_prefix(LOCAL_ACTIONS))
        else {
            events.push(Event {
                labels: here,
                run: step.run.unwrap_or_default(),
            });
            continue;
        };
        if depth >= MAX_COMPOSITE_DEPTH {
            return Err(format!("local action {action} nests deeper than {depth}").into());
        }
        let path = format!(".github/actions/{action}/action.yml");
        let text = files
            .get(&path)
            .ok_or_else(|| format!("local action {path} is missing"))?;
        here.push(action.to_owned());
        events.extend(flatten(files, steps_of(text, &path)?, &here, depth + 1)?);
    }
    Ok(events)
}

fn phase_of(labels: &[String]) -> Option<Phase> {
    let has = |names: &[&str; 2]| labels.iter().any(|label| names.contains(&label.as_str()));
    match (has(&INSTALLED), has(&RESTORED)) {
        (true, false) => Some(Phase::Installed),
        (false, true) => Some(Phase::Restored),
        _ => None,
    }
}

fn is_pinned_verifier(run: &str) -> bool {
    pin_markers().iter().all(|marker| run.contains(marker))
}

fn component_name(word: &str) -> Result<String, Box<dyn Error>> {
    match word {
        "clippy" | "rustfmt" => Ok(format!("{word}-preview")),
        other => Err(format!("unknown rustup component {other:?}").into()),
    }
}

fn base_of(word: &str) -> Result<Option<&'static [&'static str]>, Box<dyn Error>> {
    if word.starts_with("rust@") {
        return Ok(Some(&DEFAULT));
    }
    let Some(rest) = word.strip_prefix("rust[") else {
        return Ok(None);
    };
    match rest.split_once("]@") {
        Some(("profile=minimal", _)) => Ok(Some(&MINIMAL)),
        _ => Err(format!("unknown rust install option in {word:?}").into()),
    }
}

fn apply(toolchain: &mut Toolchain, command: &Command) -> Result<(), Box<dyn Error>> {
    let words = command.words();
    if let Some(mise) = words.iter().position(|word| word == "mise")
        && let Some(install) = words[mise..].iter().position(|word| word == "install")
    {
        for word in &words[mise + install + 1..] {
            if let Some(base) = base_of(word)? {
                toolchain.base = Some(base);
            }
        }
    }
    if let Some(add) = words
        .windows(3)
        .position(|window| window == ["rustup", "component", "add"])
    {
        let mut rest = words[add + 3..].iter();
        while let Some(word) = rest.next() {
            if word == "--toolchain" {
                rest.next();
            } else {
                toolchain.added.insert(component_name(word)?);
            }
        }
    }
    Ok(())
}

fn digest_of(base: &[&str], added: &BTreeSet<String>) -> (Vec<String>, String) {
    let mut names = base
        .iter()
        .map(|name| (*name).to_owned())
        .chain(added.iter().cloned())
        .map(|name| format!("{name}-{RUST_TARGET_TRIPLE}"))
        .collect::<Vec<_>>();
    names.sort();
    let joined = names
        .iter()
        .map(|name| format!("{name}\n"))
        .collect::<Vec<_>>()
        .concat();
    let sha = Sha256::digest(joined.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .concat();
    (names, sha)
}

fn check_job(
    files: &Files,
    file: &str,
    workflow: &str,
    id: &str,
    report: &mut Report,
) -> Result<(), Box<dyn Error>> {
    let label = format!("{file} {id}");
    let steps = steps_of(rendered_job_block(workflow, id)?, &label)?;
    let pinned = pinned_components();
    let mut toolchain = Toolchain::default();
    let mut finding = |detail: String| {
        report.findings.push(Finding {
            file: file.to_owned(),
            job: id.to_owned(),
            detail,
        });
    };
    let mut observed = Vec::new();
    let mut restored = 0;
    for event in flatten(files, steps, &[], 0)? {
        if !is_pinned_verifier(&event.run) {
            for command in commands_of(&event.run) {
                apply(&mut toolchain, &command)?;
            }
            continue;
        }
        let at = event.labels.join(" > ");
        match phase_of(&event.labels) {
            None => finding(format!(
                "{at}: pinned verifier is neither installed nor restored"
            )),
            Some(Phase::Restored) => restored += 1,
            Some(Phase::Installed) => match toolchain.base {
                None => finding(format!("{at}: no Rust install precedes the verifier")),
                Some(base) => {
                    let (names, sha) = digest_of(base, &toolchain.added);
                    if !pinned.contains(&sha) {
                        finding(format!("{at} sees {names:?} sha={sha}"));
                    }
                    observed.push((sha, !toolchain.added.is_empty()));
                }
            },
        }
    }
    report.restored += restored;
    for (sha, adds) in observed {
        report.observed.insert(sha);
        report.checked.insert((file.to_owned(), id.to_owned()));
        if adds {
            report.with_adds.insert((file.to_owned(), id.to_owned()));
        }
    }
    Ok(())
}

fn scan(trees: &[Files]) -> Result<Report, Box<dyn Error>> {
    let mut report = Report::default();
    for files in trees {
        for (file, workflow) in files {
            if !file.starts_with(".github/workflows/") {
                continue;
            }
            for id in job_ids(workflow) {
                check_job(files, file, workflow, id, &mut report)?;
            }
        }
    }
    Ok(report)
}

fn tree_of(policy: WorkflowPolicy) -> Result<Files, Box<dyn Error>> {
    let repo = owner_feature_repo(policy, false, "default", "default")?;
    files_of(&render_staged_tree(&prepare(repo.path())?)?)
}

fn suites_tree() -> Result<Files, Box<dyn Error>> {
    let repo = make_repo(SCHEDULED_SUITES_CONFIG)?;
    files_of(&render_staged_tree(&prepare(repo.path())?)?)
}

fn all_trees() -> Result<Vec<Files>, Box<dyn Error>> {
    let mut trees = POLICIES
        .into_iter()
        .map(tree_of)
        .collect::<Result<Vec<_>, _>>()?;
    trees.push(suites_tree()?);
    Ok(trees)
}

fn has(report: &Report, file: &str, job: &str) -> bool {
    report.checked.contains(&(file.to_owned(), job.to_owned()))
}

#[test]
fn every_pinned_installed_verifier_sees_a_pinned_component_set() -> Result<(), Box<dyn Error>> {
    let report = scan(&all_trees()?)?;
    assert_eq!(report.findings, Vec::<Finding>::new());
    assert_eq!(report.observed, pinned_components(), "{report:#?}");
    assert!(report.restored > 0, "{report:#?}");
    assert!(has(&report, FRESHNESS_PATH, "freshness"), "{report:#?}");
    assert!(has(&report, SUITES_PATH, "suite-nightly"), "{report:#?}");
    assert!(
        has(&report, CI_PATH, velnor_actions_contract::PLAN_JOB_ID),
        "{report:#?}"
    );
    assert!(!report.with_adds.is_empty(), "{report:#?}");
    Ok(())
}

fn mutated(files: &Files, path: &str, from: &str, to: &str) -> Result<Files, Box<dyn Error>> {
    let text = files
        .get(path)
        .ok_or_else(|| format!("{path} is missing"))?;
    assert!(text.contains(from), "{path} has no {from:?}");
    let mut copy = files.clone();
    copy.insert(path.to_owned(), text.replace(from, to));
    Ok(copy)
}

#[test]
fn a_bare_rust_install_in_the_freshness_job_is_detected() -> Result<(), Box<dyn Error>> {
    let tree = tree_of(WorkflowPolicy::VelnorRepositoryV1)?;
    let clean = scan(std::slice::from_ref(&tree))?;
    assert_eq!(clean.findings, Vec::<Finding>::new());
    assert!(has(&clean, FRESHNESS_PATH, "freshness"));
    let broken = mutated(&tree, FRESHNESS_PATH, "rust[profile=minimal]@", "rust@")?;
    let report = scan(&[broken])?;
    assert_eq!(report.findings.len(), 1, "{:#?}", report.findings);
    let found = &report.findings[0];
    assert_eq!(
        (found.file.as_str(), found.job.as_str()),
        (FRESHNESS_PATH, "freshness")
    );
    for expected in [
        "rust-docs-",
        "clippy-preview-",
        "rustfmt-preview-",
        "sha=0961e13a",
    ] {
        assert!(found.detail.contains(expected), "{}", found.detail);
    }
    Ok(())
}

#[test]
fn a_partial_component_add_in_ci_is_detected_in_every_job_that_adds() -> Result<(), Box<dyn Error>>
{
    let tree = tree_of(WorkflowPolicy::VelnorRepositoryV1)?;
    let clean = scan(std::slice::from_ref(&tree))?;
    assert_eq!(clean.findings, Vec::<Finding>::new());
    assert!(!clean.with_adds.is_empty());
    let broken = mutated(&tree, CI_PATH, " clippy rustfmt", " clippy")?;
    let broken = mutated(
        &broken,
        STATE_SEAL_WORKFLOW_PATH,
        " clippy rustfmt",
        " clippy",
    )?;
    let report = scan(&[broken])?;
    let flagged = report
        .findings
        .iter()
        .map(|found| (found.file.clone(), found.job.clone()))
        .collect::<BTreeSet<_>>();
    assert_eq!(flagged, clean.with_adds, "{:#?}", report.findings);
    Ok(())
}
