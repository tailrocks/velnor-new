// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::facts::{TestResult, path};
use crate::job_shape::with_targets;
use crate::pipeline::{pin_helper, view, workflow};
use crate::select::{all, changed, facts, some};
use velnor_gen::expected_with_notes;
use velnor_model::{
    AllTrigger, CargoCommand, DocsRun, EventKind, OptionalSteps, PushAfterMergeGroup, RepoFacts,
    RustCiConfig, RustSelection, SkipReason, StepKind, StepNeed, TargetKind, When,
};
use velnor_rust::{
    CONFIG_PATH, DetectError, Reason, defer_post_merge_docs, defer_unselected_docs, detect, plan,
};

const POST_MERGE: &str = "(github.event_name == 'push' || github.event_name == 'schedule' || github.event_name == 'workflow_dispatch')";

fn step_conditions(
    config: &str,
    named: &dyn Fn(&str) -> bool,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let mut repo = view(&["Cargo.toml", "tools/Cargo.toml"], None)?;
    pin_helper(&mut repo, "0.1.97")?;
    repo.files
        .insert(path(".velnor/rust-ci.toml")?, config.as_bytes().to_vec());
    let yaml: serde_json::Value = crate::yaml_tree::document(&workflow(&repo)?)?;
    let jobs = yaml["jobs"].as_object().ok_or("jobs missing")?;
    let conditions: Vec<String> = jobs
        .values()
        .flat_map(|job| job["steps"].as_array().into_iter().flatten())
        .filter(|step| named(step["name"].as_str().unwrap_or_default()))
        .map(|step| step["if"].as_str().unwrap_or_default().to_owned())
        .collect();
    Ok(conditions)
}

fn documentation_conditions(config: &str) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    step_conditions(config, &|name| name == "Documentation")
}

fn reuse_flags(docs: DocsRun, alone: DocsRun, picked: &dyn Fn(&StepKind) -> bool) -> Vec<bool> {
    let members = vec![with_targets("lib", &[TargetKind::Lib])];
    let mut facts = facts(vec![("Cargo.toml", members)]);
    facts.config.docs_run = docs;
    facts.config.check_alone_run = alone;
    facts.config.push_after_merge_group = PushAfterMergeGroup::SkipWhenQueueRunPassed;
    plan(&facts)
        .jobs
        .iter()
        .flat_map(|job| job.steps.iter())
        .filter(|step| picked(&step.kind))
        .map(|step| step.skip_after_merge_group.is_some())
        .collect()
}

fn is_doc(kind: &StepKind) -> bool {
    matches!(
        kind,
        StepKind::Cargo(CargoCommand::Doc | CargoCommand::DocFeatures { .. })
    )
}

fn is_alone(kind: &StepKind) -> bool {
    matches!(kind, StepKind::Cargo(CargoCommand::CheckAlone { .. }))
}

fn is_clippy(kind: &StepKind) -> bool {
    matches!(kind, StepKind::Cargo(CargoCommand::Clippy { .. }))
}

#[test]
fn a_deferred_step_is_never_reused_from_a_merge_group_run() {
    let docs = reuse_flags(DocsRun::PostMerge, DocsRun::Always, &is_doc);
    assert_ne!(docs.len(), 0);
    assert!(docs.iter().all(|reused| !reused), "{docs:?}");
    let alone = reuse_flags(DocsRun::Always, DocsRun::PostMerge, &is_alone);
    assert_ne!(alone.len(), 0);
    assert!(alone.iter().all(|reused| !reused), "{alone:?}");
}

#[test]
fn deferral_leaves_merge_group_reuse_of_every_other_step() {
    let docs = reuse_flags(DocsRun::Always, DocsRun::Always, &is_doc);
    assert!(docs.iter().all(|reused| *reused), "{docs:?}");
    let alone = reuse_flags(DocsRun::Always, DocsRun::Always, &is_alone);
    assert!(alone.iter().all(|reused| *reused), "{alone:?}");
    let clippy = reuse_flags(DocsRun::PostMerge, DocsRun::PostMerge, &is_clippy);
    assert_ne!(clippy.len(), 0);
    assert!(clippy.iter().all(|reused| *reused), "{clippy:?}");
}

fn alone_whens(mode: Option<&str>) -> Vec<When> {
    let members = vec![with_targets("lib", &[TargetKind::Lib])];
    let mut facts = facts(vec![("Cargo.toml", members)]);
    if let Some(mode) = mode {
        facts.config.check_alone_run = if mode == "post_merge" {
            DocsRun::PostMerge
        } else {
            DocsRun::Always
        };
    }
    plan(&facts)
        .jobs
        .iter()
        .flat_map(|job| job.steps.iter())
        .filter(|step| matches!(step.kind, StepKind::Cargo(CargoCommand::CheckAlone { .. })))
        .map(|step| step.when)
        .collect()
}

#[test]
fn check_alone_runs_on_every_event_unless_configured() {
    for mode in [None, Some("always")] {
        let whens = alone_whens(mode);
        assert_ne!(whens.len(), 0);
        assert!(whens.iter().all(|w| *w == When::NotCancelled));
    }
}

#[test]
fn post_merge_check_alone_defers_only_the_check_alone_steps() {
    let whens = alone_whens(Some("post_merge"));
    assert_ne!(whens.len(), 0);
    assert!(whens.iter().all(|w| *w == When::NotCancelledPostMerge));
}

#[test]
fn the_check_alone_key_parses_and_an_unknown_value_is_refused() -> TestResult {
    let rendered = documentation_conditions("check_alone = \"post_merge\"\n")?;
    assert!(rendered.iter().all(|c| !c.contains("github.event_name")));
    assert!(documentation_conditions("check_alone = \"never\"\n").is_err());
    Ok(())
}

#[test]
fn documentation_runs_on_every_event_unless_configured() -> TestResult {
    for config in ["", "docs = \"always\"\n"] {
        let conditions = documentation_conditions(config)?;
        assert_ne!(conditions.len(), 0);
        assert!(conditions.iter().all(|c| !c.contains("github.event_name")));
    }
    Ok(())
}

#[test]
fn post_merge_documentation_skips_pull_requests_and_merge_groups() -> TestResult {
    let conditions = documentation_conditions("docs = \"post_merge\"\n")?;
    assert_ne!(conditions.len(), 0);
    for condition in &conditions {
        assert!(condition.contains(POST_MERGE), "{condition}");
        assert!(condition.contains("!cancelled()"), "{condition}");
    }
    Ok(())
}

#[test]
fn an_unknown_docs_mode_is_refused() {
    assert!(documentation_conditions("docs = \"never\"\n").is_err());
}

#[test]
fn a_post_merge_mode_needs_a_helper_that_knows_its_key() -> TestResult {
    for (key, floor, too_old) in [
        ("docs", "0.1.97", "0.1.96"),
        ("check_alone", "0.1.97", "0.1.96"),
    ] {
        let config = format!("{key} = \"post_merge\"\n");
        let mut old = view(&["Cargo.toml"], Some(config.as_bytes()))?;
        pin_helper(&mut old, too_old)?;
        let error = expected_with_notes(&old)
            .err()
            .ok_or("an older helper was accepted")?
            .to_string();
        assert!(error.contains(&format!("pin-helper {floor}")), "{error}");
        assert!(error.contains(key), "{error}");
        let mut new = view(&["Cargo.toml"], Some(config.as_bytes()))?;
        pin_helper(&mut new, floor)?;
        workflow(&new)?;
        let mut always = view(
            &["Cargo.toml"],
            Some(format!("{key} = \"always\"\n").as_bytes()),
        )?;
        pin_helper(&mut always, too_old)?;
        workflow(&always)?;
    }
    Ok(())
}

#[test]
fn documentation_the_post_merge_mode_defers_is_reported_as_left_out_before_the_merge() {
    let run = OptionalSteps {
        doctests: StepNeed::Run,
        doctests_known: true,
        docs: StepNeed::Run,
    };
    let mut config = RustCiConfig {
        docs_run: DocsRun::PostMerge,
        ..RustCiConfig::default()
    };
    for kind in [EventKind::PullRequest, EventKind::MergeGroup] {
        let steps = defer_post_merge_docs(&config, kind, run);
        assert_eq!(
            steps.docs,
            StepNeed::Skip(SkipReason::PostMergeOnly),
            "{kind:?}"
        );
        assert_eq!(steps.doctests, StepNeed::Run);
    }
    for kind in [
        EventKind::Push,
        EventKind::Schedule,
        EventKind::WorkflowDispatch,
    ] {
        assert_eq!(defer_post_merge_docs(&config, kind, run), run, "{kind:?}");
    }
    config.docs_run = DocsRun::Always;
    assert_eq!(
        defer_post_merge_docs(&config, EventKind::PullRequest, run),
        run
    );
}

#[test]
fn a_build_phase_setup_still_runs_before_a_pre_merge_check_that_shares_its_job_with_post_merge_docs()
-> TestResult {
    let mut repo = view(&["Cargo.toml", "tools/Cargo.toml"], None)?;
    pin_helper(&mut repo, "0.1.109")?;
    let config = "docs = \"post_merge\"\nshape = \"lint-test\"\n[[test_setup]]\nroot = '.'\nname = 'Prepare the fixture'\nrun = 'true'\nphase = 'build'\n";
    repo.files
        .insert(path(".velnor/rust-ci.toml")?, config.as_bytes().to_vec());
    let yaml: serde_json::Value = crate::yaml_tree::document(&workflow(&repo)?)?;
    let mut checked = 0;
    for job in yaml["jobs"].as_object().ok_or("jobs missing")?.values() {
        let steps = job["steps"].as_array().map_or(&[][..], Vec::as_slice);
        let named = |name: &str| steps.iter().any(|step| step["name"] == name);
        if !named("Lint") || !named("Documentation") {
            continue;
        }
        for setup in steps
            .iter()
            .filter(|step| step["name"] == "Prepare the fixture")
        {
            checked += 1;
            let condition = setup["if"].as_str().unwrap_or_default();
            assert!(
                !condition.contains("github.event_name == 'push'"),
                "{condition}"
            );
        }
    }
    assert_ne!(checked, 0);
    Ok(())
}

const BEFORE_MERGE: [EventKind; 2] = [EventKind::PullRequest, EventKind::MergeGroup];
const AFTER_MERGE: [EventKind; 3] = [
    EventKind::Push,
    EventKind::Schedule,
    EventKind::WorkflowDispatch,
];

fn selected_docs_facts() -> RepoFacts {
    let members = vec![
        with_targets("lib", &[TargetKind::Lib]),
        with_targets("fixtures", &[TargetKind::Test, TargetKind::Example]),
    ];
    let mut repo = facts(vec![("Cargo.toml", members)]);
    repo.config.docs_run = DocsRun::Selected;
    repo
}

fn docs_after_deferring(
    repo: &RepoFacts,
    event: EventKind,
    selection: &RustSelection,
) -> Result<StepNeed, Box<dyn std::error::Error>> {
    let root = repo.rust_roots.first().ok_or("no root")?.manifest.clone();
    let run = OptionalSteps {
        doctests: StepNeed::Skip(SkipReason::NoFencedDocs),
        doctests_known: false,
        docs: StepNeed::Run,
    };
    let steps = defer_unselected_docs(repo, event, &root, selection, run);
    assert_eq!(steps.doctests, run.doctests, "{event:?}");
    Ok(steps.docs)
}

#[test]
fn selected_docs_skip_on_a_pull_request_that_selects_no_documented_package() -> TestResult {
    let repo = selected_docs_facts();
    let selection = some(&[("fixtures", changed("fixtures/tests/a.rs"))]);
    for event in BEFORE_MERGE {
        assert_eq!(
            docs_after_deferring(&repo, event, &selection)?,
            StepNeed::Skip(SkipReason::NoSelectedDocumented),
            "{event:?}"
        );
    }
    assert_eq!(
        SkipReason::NoSelectedDocumented.to_string(),
        "no selected package has documented targets"
    );
    Ok(())
}

#[test]
fn selected_docs_run_on_a_pull_request_that_selects_a_documented_package() -> TestResult {
    let repo = selected_docs_facts();
    let selections = [
        some(&[("lib", changed("lib/src/lib.rs"))]),
        some(&[
            ("fixtures", changed("fixtures/tests/a.rs")),
            ("lib", changed("lib/src/lib.rs")),
        ]),
        all(AllTrigger::WorkspaceManifest),
    ];
    for event in BEFORE_MERGE {
        for selection in &selections {
            assert_eq!(
                docs_after_deferring(&repo, event, selection)?,
                StepNeed::Run,
                "{event:?} {selection:?}"
            );
        }
    }
    Ok(())
}

#[test]
fn selected_docs_always_run_on_push() -> TestResult {
    let repo = selected_docs_facts();
    let selection = some(&[("fixtures", changed("fixtures/tests/a.rs"))]);
    for event in AFTER_MERGE {
        assert_eq!(
            docs_after_deferring(&repo, event, &selection)?,
            StepNeed::Run,
            "{event:?}"
        );
    }
    let mut always = selected_docs_facts();
    always.config.docs_run = DocsRun::Always;
    for event in BEFORE_MERGE {
        assert_eq!(
            docs_after_deferring(&always, event, &selection)?,
            StepNeed::Run,
            "{event:?}"
        );
    }
    Ok(())
}

#[test]
fn an_unknown_docs_value_is_refused() -> TestResult {
    for (key, value) in [("docs", "sometimes"), ("check_alone", "selected")] {
        let config = format!("{key} = \"{value}\"\n");
        let repo = view(&["Cargo.toml"], Some(config.as_bytes()))?;
        match detect(&repo) {
            Err(DetectError::Invalid {
                path,
                reason: Reason::InvalidValue { key: refused, .. },
            }) => assert_eq!((path, refused), (CONFIG_PATH, key), "{key} = {value}"),
            other => return Err(format!("{key} = {value}: {other:?}").into()),
        }
    }
    let repo = view(&["Cargo.toml"], Some(b"docs = \"selected\"\n"))?;
    assert_eq!(detect(&repo)?.config.docs_run, DocsRun::Selected);
    Ok(())
}
