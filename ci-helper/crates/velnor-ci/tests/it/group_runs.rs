// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_model::{
    AllTrigger, EventKind, GroupEvent, GroupRoot, JobId, PackageName, PathGlob, RepoPath,
    RustSelection, SelectionReason, TestGroup,
};
use velnor_rust::group_runs;

fn group(paths: &[&str], events: &[GroupEvent]) -> TestGroup {
    TestGroup {
        name: JobId::new("database").unwrap_or_else(|e| unreachable!("{e}")),
        filter: "package(app)".to_owned(),
        needs: Vec::new(),
        test_threads: None,
        timeout_minutes: None,
        runner: None,
        paths: paths
            .iter()
            .map(|path| PathGlob::new(*path).unwrap_or_else(|e| unreachable!("{e}")))
            .collect(),
        events: events.to_vec(),
        root: GroupRoot::OnlyRoot,
    }
}

fn changed(paths: &[&str]) -> BTreeSet<RepoPath> {
    paths
        .iter()
        .map(|path| RepoPath::new(*path).unwrap_or_else(|e| unreachable!("{e}")))
        .collect()
}

fn some() -> RustSelection {
    let app = PackageName::new("app").unwrap_or_else(|e| unreachable!("{e}"));
    let changed = RepoPath::new("app/src/lib.rs").unwrap_or_else(|e| unreachable!("{e}"));
    RustSelection::Some(BTreeMap::from([(app, SelectionReason::Changed(changed))]))
}

fn everything() -> RustSelection {
    RustSelection::All(AllTrigger::Event)
}

#[test]
fn group_runs_for_a_listed_event_a_matching_path_or_a_full_selection_and_skips_otherwise() {
    let keyed = group(
        &["app/migrations/**"],
        &[GroupEvent::Push, GroupEvent::Schedule],
    );
    let rows = [
        (
            "listed event",
            &keyed,
            EventKind::Schedule,
            changed(&["app/src/lib.rs"]),
            some(),
            true,
            "event schedule is listed in events",
        ),
        (
            "matching path",
            &keyed,
            EventKind::PullRequest,
            changed(&["README.md", "app/migrations/0001_init.sql"]),
            some(),
            true,
            "changed path app/migrations/0001_init.sql matches paths app/migrations/**",
        ),
        (
            "everything selected",
            &keyed,
            EventKind::PullRequest,
            changed(&["Cargo.lock"]),
            everything(),
            true,
            "every package is selected",
        ),
        (
            "another event and another path",
            &keyed,
            EventKind::PullRequest,
            changed(&["app/src/lib.rs"]),
            some(),
            false,
            "event pull_request is not listed in events and no changed path matches paths",
        ),
        (
            "an event of another group does not count",
            &keyed,
            EventKind::MergeGroup,
            changed(&["app/src/lib.rs"]),
            some(),
            false,
            "event merge_group is not listed in events and no changed path matches paths",
        ),
        (
            "nothing selected, even for a listed event",
            &keyed,
            EventKind::Push,
            changed(&["app/migrations/0001_init.sql"]),
            RustSelection::None,
            false,
            "no package is selected, so no archive is built",
        ),
        (
            "neither key set, a selected package",
            &group(&[], &[]),
            EventKind::PullRequest,
            changed(&["app/src/lib.rs"]),
            some(),
            true,
            "paths and events are not set, so the group runs with its root",
        ),
        (
            "neither key set, nothing selected",
            &group(&[], &[]),
            EventKind::PullRequest,
            changed(&["README.md"]),
            RustSelection::None,
            false,
            "no package is selected, so no archive is built",
        ),
    ];
    for (row, group, event, paths, selection, run, reason) in rows {
        let decision = group_runs(group, event, &paths, &selection);
        assert_eq!(
            (decision.run, decision.reason.as_str()),
            (run, reason),
            "{row}"
        );
    }
}
