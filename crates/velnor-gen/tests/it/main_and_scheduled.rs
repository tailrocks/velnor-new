// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::cell::Cell;

use velnor_gen::notes;
use velnor_model::{
    AllTrigger, ChangeSet, CronExpr, EventKind, FullRun, OnMain, RepoFacts, RepositoryIdentity,
    RustSelection,
};
use velnor_rust::{CONFIG_PATH, DetectError, Reason, detect, plan, select_event};

use crate::facts::{TestResult, lib, repo};
use crate::select::{all, changed, member, path, some};

fn detect_with(config: &str, repository: &str) -> Result<RepoFacts, DetectError> {
    let mut view = repo(
        vec![(
            "Cargo.toml",
            vec![lib("a", "a/Cargo.toml").unwrap_or_else(|e| unreachable!("{e}"))],
        )],
        &[],
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    view.repository = RepositoryIdentity::Named(repository.to_owned());
    view.tracked_paths.insert(CONFIG_PATH);
    view.files.insert(CONFIG_PATH, config.as_bytes().to_vec());
    detect(&view)
}

fn two_packages(on_main: OnMain) -> RepoFacts {
    let mut facts = crate::select::facts(vec![(
        "Cargo.toml",
        vec![
            member("app", "app", &[]),
            member("lib", "lib", &[]),
            member("tool", "tool", &[]),
        ],
    )]);
    facts.config.on_main = on_main;
    facts
}

fn change(files: &[&str]) -> ChangeSet {
    ChangeSet {
        paths: files.iter().map(|file| path(file)).collect(),
        ..ChangeSet::default()
    }
}

fn root_selection(
    facts: &RepoFacts,
    event: EventKind,
    diff: Option<ChangeSet>,
) -> (RustSelection, bool) {
    let asked = Cell::new(false);
    let (selections, _) = select_event(facts, event, || {
        asked.set(true);
        diff
    });
    let selection = selections
        .get(&path("Cargo.toml"))
        .cloned()
        .unwrap_or_else(|| unreachable!("no root"));
    (selection, asked.get())
}

#[derive(Clone, Copy)]
enum Expect {
    Nothing,
    AppOnly,
    Everything,
}

fn selection_of(expect: Expect) -> RustSelection {
    match expect {
        Expect::Nothing => RustSelection::None,
        Expect::AppOnly => some(&[("app", changed("app/src/lib.rs"))]),
        Expect::Everything => all(AllTrigger::Event),
    }
}

#[test]
fn each_event_selects_as_its_on_main_value_says() {
    let docs = || Some(change(&["docs/guide.md"]));
    let app = || Some(change(&["app/src/lib.rs"]));
    let diff_events = [
        EventKind::PullRequest,
        EventKind::Push,
        EventKind::MergeGroup,
    ];
    let main_events = [EventKind::Push, EventKind::MergeGroup];
    let full_events = [EventKind::Schedule, EventKind::WorkflowDispatch];
    let mut rows = Vec::new();
    for event in diff_events {
        rows.push((event, OnMain::Affected, docs(), Expect::Nothing, true));
        rows.push((event, OnMain::Affected, app(), Expect::AppOnly, true));
    }
    rows.push((
        EventKind::PullRequest,
        OnMain::Everything,
        docs(),
        Expect::Nothing,
        true,
    ));
    rows.push((
        EventKind::PullRequest,
        OnMain::Everything,
        app(),
        Expect::AppOnly,
        true,
    ));
    for event in main_events {
        rows.push((event, OnMain::Everything, docs(), Expect::Everything, false));
    }
    for event in full_events {
        for on_main in [OnMain::Affected, OnMain::Everything] {
            rows.push((event, on_main, docs(), Expect::Everything, false));
        }
    }
    for (event, on_main, diff, expect, reads_diff) in rows {
        let (selection, asked) = root_selection(&two_packages(on_main), event, diff);
        assert_eq!(selection, selection_of(expect), "{event:?} {on_main:?}");
        assert_eq!(asked, reads_diff, "{event:?} {on_main:?}");
    }
}

#[test]
fn a_diff_that_cannot_be_read_selects_everything_with_the_base_unavailable_for_a_diff_event() {
    let unavailable = all(AllTrigger::BaseUnavailable);
    for (event, on_main) in [
        (EventKind::PullRequest, OnMain::Affected),
        (EventKind::Push, OnMain::Affected),
        (EventKind::MergeGroup, OnMain::Affected),
    ] {
        let (selection, _) = root_selection(&two_packages(on_main), event, None);
        assert_eq!(selection, unavailable, "{event:?}");
    }
    let (selection, _) = root_selection(&two_packages(OnMain::Everything), EventKind::Push, None);
    assert_eq!(selection, all(AllTrigger::Event));
}

#[test]
fn on_main_and_full_run_default_to_affected_and_nightly_and_accept_every_documented_value()
-> TestResult {
    let defaults = detect_with("", "r")?.config;
    assert_eq!(defaults.on_main, OnMain::Affected);
    assert_eq!(defaults.full_run, FullRun::Nightly);
    let rows = [
        ("on_main = \"affected\"", OnMain::Affected, FullRun::Nightly),
        (
            "on_main = \"everything\"",
            OnMain::Everything,
            FullRun::Nightly,
        ),
        ("full_run = \"off\"", OnMain::Affected, FullRun::Off),
        ("full_run = \"hourly\"", OnMain::Affected, FullRun::Hourly),
        ("full_run = \"nightly\"", OnMain::Affected, FullRun::Nightly),
        ("full_run = \"weekly\"", OnMain::Affected, FullRun::Weekly),
        (
            "full_run = \"0 */3 * * *\"",
            OnMain::Affected,
            FullRun::Cron(CronExpr::new("0 */3 * * *")?),
        ),
        (
            "on_main = \"everything\"\nfull_run = \"off\"",
            OnMain::Everything,
            FullRun::Off,
        ),
    ];
    for (text, on_main, full_run) in rows {
        let config = detect_with(text, "r")?.config;
        assert_eq!(
            (config.on_main, config.full_run),
            (on_main, full_run),
            "{text}"
        );
    }
    Ok(())
}

#[test]
fn an_invalid_on_main_or_full_run_is_a_config_error_naming_the_value() {
    let rows = [
        ("on_main = \"always\"", "on_main", "always"),
        ("full_run = \"daily\"", "full_run", "daily"),
        ("full_run = \"0 0 * *\"", "full_run", "0 0 * *"),
        ("full_run = \"0 0 * * * *\"", "full_run", "0 0 * * * *"),
        ("full_run = \"60 0 * * *\"", "full_run", "60 0 * * *"),
        ("full_run = \"0 24 * * *\"", "full_run", "0 24 * * *"),
        ("full_run = \"0 0 0 * *\"", "full_run", "0 0 0 * *"),
        ("full_run = \"0 0 * 13 *\"", "full_run", "0 0 * 13 *"),
        ("full_run = \"0 0 * * 7\"", "full_run", "0 0 * * 7"),
        ("full_run = \"0 0 * * MON\"", "full_run", "0 0 * * MON"),
        ("full_run = \"5-1 0 * * *\"", "full_run", "5-1 0 * * *"),
        ("full_run = \"60/2 0 * * *\"", "full_run", "60/2 0 * * *"),
        ("full_run = \"*/0 0 * * *\"", "full_run", "*/0 0 * * *"),
        ("full_run = \"0  0 * * *\"", "full_run", "0  0 * * *"),
        ("full_run = \"@daily\"", "full_run", "@daily"),
    ];
    for (text, key, value) in rows {
        let error = detect_with(text, "r").err();
        let expected = DetectError::Invalid {
            path: CONFIG_PATH,
            reason: Reason::InvalidValue {
                key,
                value: value.to_owned(),
            },
        };
        assert_eq!(error, Some(expected), "{text}");
    }
}

#[test]
fn a_cron_expression_accepts_lists_ranges_and_steps_in_every_field() {
    for text in [
        "0 */3 * * *",
        "15,45 8-18/2 1,15 */2 1-5",
        "59 23 31 12 6",
        "0 0 * * 0",
        "20/15 * * * *",
        "0 5/2 * * *",
    ] {
        assert!(CronExpr::new(text).is_ok(), "{text}");
    }
}

fn schedule_of(config: &str, repository: &str) -> Option<CronExpr> {
    let facts = detect_with(config, repository).unwrap_or_else(|e| unreachable!("{e}"));
    plan(&facts).triggers.schedule
}

#[test]
fn a_preset_schedule_is_fixed_per_repository_and_differs_between_repositories() {
    let nightly = |name: &str| schedule_of("full_run = \"nightly\"", name);
    assert_eq!(nightly("alpha"), nightly("alpha"));
    let distinct: std::collections::BTreeSet<_> =
        (0..40).map(|n| nightly(&format!("repo-{n}"))).collect();
    assert!(distinct.len() > 20, "{distinct:?}");
    let weekly = schedule_of("full_run = \"weekly\"", "alpha")
        .unwrap_or_else(|| unreachable!("no schedule"));
    let hourly = schedule_of("full_run = \"hourly\"", "alpha")
        .unwrap_or_else(|| unreachable!("no schedule"));
    let fields: Vec<&str> = weekly.as_str().split(' ').collect();
    assert_eq!(&fields[2..4], ["*", "*"], "{weekly}");
    assert!(hourly.as_str().ends_with(" * * * *"), "{hourly}");
    assert_eq!(schedule_of("full_run = \"off\"", "alpha"), None);
    assert_eq!(
        schedule_of("full_run = \"7 1 * * 3\"", "alpha"),
        CronExpr::new("7 1 * * 3").ok()
    );
}

#[test]
fn generate_notes_only_a_repository_that_never_selects_everything() -> TestResult {
    let note = |text: &str| -> Result<usize, Box<dyn std::error::Error>> {
        let mut view = repo(vec![("Cargo.toml", vec![lib("a", "a/Cargo.toml")?])], &[])?;
        view.tracked_paths.insert(CONFIG_PATH);
        view.files.insert(CONFIG_PATH, text.as_bytes().to_vec());
        Ok(notes(&view)?.len())
    };
    assert_eq!(note("full_run = \"off\"")?, 1);
    assert_eq!(note("")?, 0);
    assert_eq!(note("full_run = \"off\"\non_main = \"everything\"")?, 0);
    assert_eq!(note("full_run = \"weekly\"")?, 0);
    Ok(())
}
