// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_ci::changes::{FILE_LIST_CAP, base_files, changed_files};
use velnor_ci::error::CiError;
use velnor_ci::event::{Event, PullRequest, check_default_branch, parse_event};
use velnor_ci::github::{GitHub, Repo, contents_path, pull_files_path};
use velnor_model::RepoPath;

struct Recorded(BTreeMap<String, Result<String, String>>);

impl GitHub for Recorded {
    fn get(&self, path: &str) -> Result<String, CiError> {
        self.0
            .get(path)
            .cloned()
            .unwrap_or_else(|| Err(format!("404 {path}")))
            .map_err(CiError::new)
    }

    fn get_raw(&self, path: &str) -> Result<String, CiError> {
        self.get(path)
    }
}

fn repo() -> Repo {
    Repo::parse("tailrocks/velnor-lab").unwrap_or_else(|e| unreachable!("{e}"))
}

fn pull() -> PullRequest {
    PullRequest {
        number: 7,
        base: "base0".to_owned(),
    }
}

fn page(names: impl Iterator<Item = String>) -> String {
    let files: Vec<String> = names
        .map(|name| format!("{{\"filename\":\"{name}\"}}"))
        .collect();
    format!("[{}]", files.join(","))
}

fn listing(pages: &[(u32, String)]) -> Recorded {
    Recorded(
        pages
            .iter()
            .map(|(number, body)| (pull_files_path(&repo(), 7, *number), Ok(body.clone())))
            .collect(),
    )
}

fn path(text: &str) -> RepoPath {
    RepoPath::new(text).unwrap_or_else(|e| unreachable!("{e}"))
}

#[test]
fn every_page_of_the_file_list_is_read_until_a_short_page() {
    let full = |start: usize| page((start..start + 100).map(|n| format!("src/f{n}.rs")));
    let github = listing(&[
        (1, full(0)),
        (2, full(100)),
        (3, page(["last.rs".to_owned()].into_iter())),
    ]);
    let found = changed_files(&github, &repo(), &pull()).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(found.map(|paths| paths.len()), Some(201));
}

#[test]
fn a_renamed_file_changes_its_old_path_too() {
    let github = listing(&[(
        1,
        "[{\"filename\":\"new/lib.rs\",\"previous_filename\":\"old/lib.rs\",\"status\":\"renamed\"}]".to_owned(),
    )]);
    let found = changed_files(&github, &repo(), &pull())
        .unwrap_or_else(|e| unreachable!("{e}"))
        .unwrap_or_default();
    assert!(found.contains(&path("new/lib.rs")) && found.contains(&path("old/lib.rs")));
}

#[test]
fn a_list_that_reaches_the_cap_or_cannot_be_read_gives_no_file_list() {
    let full = |start: usize| page((start..start + 100).map(|n| format!("f{n}.rs")));
    let pages: Vec<(u32, String)> = (0..30).map(|n| (n + 1, full(n as usize * 100))).collect();
    assert_eq!(pages.len() * 100, FILE_LIST_CAP);
    assert_eq!(changed_files(&listing(&pages), &repo(), &pull()), Ok(None));
    let mut short = pages[..29].to_vec();
    short.push((
        30,
        full(2900).replacen("{\"filename\":\"f2900.rs\"},", "", 1),
    ));
    assert_eq!(
        changed_files(&listing(&short), &repo(), &pull()).map(|found| found.map(|p| p.len())),
        Ok(Some(2999))
    );
    assert!(changed_files(&listing(&[]), &repo(), &pull()).is_err());
    let broken = listing(&[(1, "{\"message\":\"Bad credentials\"}".to_owned())]);
    assert!(changed_files(&broken, &repo(), &pull()).is_err());
}

#[test]
fn the_base_text_comes_from_the_base_commit_and_a_file_that_cannot_be_read_is_left_out() {
    let wanted = [path("Cargo.lock"), path("a b/Cargo.toml")];
    assert_eq!(
        contents_path(&repo(), "a b/Cargo.toml", "base0"),
        "/repos/tailrocks/velnor-lab/contents/a%20b/Cargo.toml?ref=base0"
    );
    let github = Recorded(BTreeMap::from([(
        contents_path(&repo(), "Cargo.lock", "base0"),
        Ok("lock text".to_owned()),
    )]));
    assert_eq!(
        base_files(&github, &repo(), &pull(), &wanted),
        BTreeMap::from([(path("Cargo.lock"), "lock text".to_owned())])
    );
}

#[test]
fn the_event_payload_gives_the_default_branch_and_the_pull_request() {
    let event = parse_event(
        "{\"repository\":{\"default_branch\":\"trunk\"},\"pull_request\":{\"number\":7,\"base\":{\"sha\":\"base0\"}}}",
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(event.default_branch.as_deref(), Some("trunk"));
    assert_eq!(event.pull_request, Some(pull()));
    let push = parse_event("{\"repository\":{\"default_branch\":\"main\"}}")
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(push.pull_request, None);
    assert!(parse_event("[]").is_err());
}

#[test]
fn a_configured_default_branch_that_differs_from_the_repositorys_is_refused_naming_both() {
    let event = |branch: Option<&str>| Event {
        default_branch: branch.map(str::to_owned),
        pull_request: None,
    };
    assert_eq!(check_default_branch("main", &event(Some("main"))), Ok(()));
    let message = check_default_branch("main", &event(Some("trunk")))
        .err()
        .map(|e| e.to_string())
        .unwrap_or_default();
    assert!(
        message.contains("\"main\"") && message.contains("\"trunk\""),
        "{message}"
    );
    assert!(check_default_branch("main", &event(None)).is_err());
}
