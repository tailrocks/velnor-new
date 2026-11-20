// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_model::{JobId, RepoPath, VerdictSpec};
use velnor_render::{ALLOWED_ACTIONS, RenderedJob, RenderedStep, RenderedWorkflow, check_rules};
use velnor_rust::{Need, decide};

use crate::facts::TestResult;
use crate::lint_lane::{IDS, LINT, rendered, repo, runs};
use crate::pipeline::workflow;

pub(crate) fn spec(text: &str) -> Result<VerdictSpec, String> {
    let line = text
        .split("GATE_SPEC: '")
        .nth(1)
        .and_then(|rest| rest.split('\'').next())
        .ok_or("no gate spec")?;
    let words: Vec<&str> = line.split(' ').collect();
    let after = |flag: &str| -> Vec<String> {
        words
            .windows(2)
            .filter(|pair| pair[0] == flag)
            .map(|pair| pair[1].to_owned())
            .collect()
    };
    let plan = after("--plan").pop().ok_or("no plan")?;
    VerdictSpec::parse(&plan, &after("--job"), &after("--tests"))
}

pub(crate) fn needs_of(text: &str) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let document = crate::yaml_tree::document(text)?;
    Ok(document["jobs"]["required"]["needs"]
        .as_array()
        .ok_or("the required job has no needs")?
        .iter()
        .filter_map(|need| need.as_str().map(str::to_owned))
        .collect())
}

pub(crate) fn verdict(
    text: &str,
    result_of: impl Fn(&str) -> Option<&'static str>,
) -> Result<(bool, String), Box<dyn std::error::Error>> {
    let spec = spec(text)?;
    let mut needs = BTreeMap::new();
    for id in needs_of(text)? {
        let Some(result) = result_of(&id) else {
            continue;
        };
        let outputs = if id == "rust" {
            [
                ("rust_root_tests", "run"),
                ("tests_run", "3"),
                ("tests_listed", "3"),
                ("tests_by_package", "pkg=3/3"),
            ]
            .map(|(key, value)| (key.to_owned(), value.to_owned()))
            .into_iter()
            .collect()
        } else {
            BTreeMap::new()
        };
        needs.insert(
            JobId::new(id)?,
            Need {
                result: Some(result.to_owned()),
                outputs,
            },
        );
    }
    let verdict = decide(&spec, &needs, Some("pull_request"));
    Ok((verdict.passed, verdict.table()))
}

#[test]
fn the_one_required_job_needs_every_lint_job_next_to_the_rust_lanes() -> TestResult {
    let text = rendered(LINT)?;
    let needs = needs_of(&text)?;
    assert!(needs.contains(&"rust".to_owned()), "{needs:?}");
    for id in IDS {
        assert!(needs.contains(&id.to_string()), "{id}: {needs:?}");
    }
    let spec = spec(&text)?;
    for id in IDS.iter().copied() {
        let job = spec
            .jobs
            .iter()
            .find(|job| job.id.to_string() == id)
            .ok_or(format!("the gate spec lacks {id}"))?;
        assert!(job.when.is_empty(), "{id} must be expected on every run");
    }
    let document = crate::yaml_tree::document(&text)?;
    let required = document["jobs"]
        .as_object()
        .ok_or("no jobs")?
        .iter()
        .filter(|(_, job)| job["name"] == "Required")
        .count();
    assert_eq!(required, 1);
    Ok(())
}

#[test]
fn the_gate_passes_only_when_every_lint_job_succeeded() -> TestResult {
    let text = rendered(LINT)?;
    let (green, table) = verdict(&text, |_| Some("success"))?;
    assert!(green, "{table}");
    for broken in IDS.iter().copied() {
        for result in ["failure", "cancelled", "skipped"] {
            let (green, table) = verdict(&text, |id| {
                Some(if id == broken { result } else { "success" })
            })?;
            assert!(!green, "{broken} {result}: {table}");
            assert!(table.contains(broken), "{broken} {result}: {table}");
        }
        let (green, table) = verdict(&text, |id| (id != broken).then_some("success"))?;
        assert!(!green, "{broken} missing from needs: {table}");
    }
    Ok(())
}

#[test]
fn the_gate_refuses_a_lint_job_it_was_not_generated_with() -> TestResult {
    let with = rendered(LINT)?;
    let without = rendered("")?;
    let spec = spec(&without)?;
    let mut needs = BTreeMap::new();
    for id in needs_of(&with)? {
        needs.insert(
            JobId::new(id)?,
            Need {
                result: Some("success".to_owned()),
                outputs: BTreeMap::new(),
            },
        );
    }
    let verdict = decide(&spec, &needs, Some("pull_request"));
    assert!(!verdict.passed, "{}", verdict.table());
    Ok(())
}

#[test]
fn a_lint_workflow_obeys_rule_nine_and_adds_no_action_to_the_fixed_list() -> TestResult {
    rendered(LINT)?;
    assert!(!ALLOWED_ACTIONS.contains(&"asamarts/alint"));
    let job = |steps| RenderedWorkflow {
        jobs: vec![RenderedJob {
            id: "lint-alint".to_owned(),
            steps,
            ..RenderedJob::default()
        }],
    };
    let step = |uses: &str| RenderedStep {
        name: "Run alint".to_owned(),
        uses: Some(uses.to_owned()),
        ..RenderedStep::default()
    };
    assert!(
        check_rules(&job(vec![step(&format!(
            "asamarts/alint@{}",
            "a".repeat(40)
        ))]))
        .is_err()
    );
    let shell = RenderedStep {
        name: "Chain".to_owned(),
        run: Some("jactionlint && true".to_owned()),
        ..RenderedStep::default()
    };
    assert!(check_rules(&job(vec![shell])).is_err());
    Ok(())
}

#[test]
fn lint_config_that_could_smuggle_shell_or_name_nothing_is_rejected() -> TestResult {
    let cases: [(String, &str); 13] = [
        ("[[lint]]\nkind = 'unknown'\n".to_owned(), "lint.kind"),
        (
            "[[lint]]\nkind = 'mise-task'\ntask = 'a; rm -rf x'\n".to_owned(),
            "lint.task",
        ),
        (
            "[[lint]]\nkind = 'mise-task'\ntask = '--force'\n".to_owned(),
            "lint.task",
        ),
        ("[[lint]]\nkind = 'mise-task'\n".to_owned(), "lint.task"),
        (
            "[[lint]]\nkind = 'mise-task'\ntask = 'x'\ntools = ['a b']\n".to_owned(),
            "lint.tools",
        ),
        (
            "[[lint]]\nkind = 'mise-task'\ntask = 'x'\ntools = ['missing-tool']\n".to_owned(),
            "missing-tool",
        ),
        (
            "[[lint]]\nkind = 'alint'\nconfig = '.alint.yml && true'\n".to_owned(),
            "lint.config",
        ),
        ("[[lint]]\nkind = 'alint'\n".to_owned(), "lint.config"),
        (
            "[[lint]]\nkind = 'jactionlint'\ntask = 'x'\n".to_owned(),
            "does not take the key task",
        ),
        (
            "[[lint]]\nkind = 'jactionlint'\n\n[[lint]]\nkind = 'jactionlint'\n".to_owned(),
            "lint-jactionlint",
        ),
        (
            "[[lint]]\nkind = 'jactionlint'\nname = '${{ github.ref }}'\n".to_owned(),
            "lint.name",
        ),
        (
            "[[lint]]\nkind = 'jactionlint'\ntimeout_minutes = 0\n".to_owned(),
            "lint.timeout_minutes",
        ),
        (
            "[[lint]]\nkind = 'jactionlint'\nsurprise = 1\n".to_owned(),
            "surprise",
        ),
    ];
    for (config, mention) in cases {
        let error = workflow(&repo(&config)?)
            .err()
            .ok_or(format!("accepted: {config}"))?;
        assert!(error.to_string().contains(mention), "{config}: {error}");
    }
    Ok(())
}

#[test]
fn an_unpinned_jactionlint_is_refused_instead_of_installed_unpinned() -> TestResult {
    let mut repo = repo("[[lint]]\nkind = 'jactionlint'\n")?;
    repo.files.insert(
        RepoPath::new("mise.toml")?,
        b"[wrappers.cargo]\ncommand = \"mbx\"\nenv = { MBX_CARGO_SHIM_MODE = \"1\" }\n\n[tools]\nrust = { version = \"1.99.1\", mr_boxington = true }\nmr-boxington = \"v1.30.0\"\n\"aqua:nextest-rs/nextest/cargo-nextest\" = \"0.9.151\"\n"
            .to_vec(),
    );
    let error = workflow(&repo).err().ok_or("accepted without a pin")?;
    assert!(error.to_string().contains("jactionlint"), "{error}");
    Ok(())
}

#[test]
fn jactionlint_without_a_pinned_shellcheck_turns_shellcheck_off() -> TestResult {
    let mut repo = repo("[[lint]]\nkind = 'jactionlint'\n")?;
    repo.files.insert(
        RepoPath::new("mise.toml")?,
        b"[wrappers.cargo]\ncommand = \"mbx\"\nenv = { MBX_CARGO_SHIM_MODE = \"1\" }\n\n[tools]\nrust = { version = \"1.99.1\", mr_boxington = true }\nmr-boxington = \"v1.30.0\"\n\"aqua:nextest-rs/nextest/cargo-nextest\" = \"0.9.151\"\njactionlint = \"2.4.0\"\n"
            .to_vec(),
    );
    let text = workflow(&repo)?;
    let jobs = crate::yaml_tree::document(&text)?["jobs"].clone();
    assert!(runs(&jobs["lint-jactionlint"])[0].ends_with("--shellcheck="));
    assert_eq!(
        jobs["lint-jactionlint"]["steps"][1]["with"]["install_args"],
        "jactionlint"
    );
    Ok(())
}
