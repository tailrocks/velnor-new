// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::{Value, json};
use velnor_ci::github::pull_files_path;
use velnor_ci::image_deploy::changes::{Selection, glob_matches, parse_matches, select};

use crate::support::{FakeGitHub, repo};

const NUMBER: u64 = 17;

fn files(names: &[&str], previous: &[(&str, &str)]) -> Vec<Value> {
    names
        .iter()
        .map(|name| {
            let before = previous
                .iter()
                .find(|(now, _)| now == name)
                .map(|(_, before)| *before);
            json!({"filename": name, "previous_filename": before})
        })
        .collect()
}

fn pull(pages: &[Vec<Value>]) -> FakeGitHub {
    let mut fake = FakeGitHub::default();
    for (index, page) in pages.iter().enumerate() {
        let number = u32::try_from(index + 1).unwrap_or(u32::MAX);
        fake.respond(
            pull_files_path(&repo(), NUMBER, number),
            &Value::Array(page.clone()).to_string(),
        );
    }
    fake
}

fn matches() -> Vec<(String, String)> {
    [
        ("api", "docker/api/**"),
        ("api", "**/Cargo.toml"),
        ("worker", "docker/worker/**"),
        ("worker", "**/Cargo.toml"),
    ]
    .map(|(image, glob)| (image.to_owned(), glob.to_owned()))
    .to_vec()
}

fn selected(selection: &Selection) -> Vec<(&str, bool)> {
    selection
        .images
        .iter()
        .map(|(name, picked)| (name.as_str(), *picked))
        .collect()
}

#[test]
fn a_path_filter_glob_stays_inside_a_segment_with_a_star_and_crosses_them_with_two() {
    for (pattern, path, expected) in [
        ("**/Cargo.toml", "Cargo.toml", true),
        ("**/Cargo.toml", "a/b/Cargo.toml", true),
        ("**/.cargo/**", ".cargo/config.toml", true),
        ("docker/api/**", "docker/api/Dockerfile", true),
        ("docker/api/**", "docker/apix/Dockerfile", false),
        ("docker/*", "docker/api/Dockerfile", false),
        ("docker/*", "docker/api", true),
        ("**/rust-toolchain*", "x/rust-toolchain.toml", true),
        ("Dockerfile", "Dockerfile", true),
        ("Dockerfile", "docker/Dockerfile", false),
        ("**/*.rs", "src/a/b.rs", true),
    ] {
        assert_eq!(glob_matches(pattern, path), expected, "{pattern} {path}");
    }
}

#[test]
fn a_pull_request_selects_only_the_images_whose_paths_it_changes() {
    let github = pull(&[files(&["docker/api/Dockerfile", "README.md"], &[])]);
    let selection = select(&github, &repo(), "pull_request", Some(NUMBER), &matches());
    assert_eq!(
        selection.map(|chosen| selected(&chosen)
            .into_iter()
            .map(|(n, p)| (n.to_owned(), p))
            .collect::<Vec<_>>()),
        Ok(vec![("api".to_owned(), true), ("worker".to_owned(), false)])
    );
    let shared = pull(&[files(&["crates/x/Cargo.toml"], &[])]);
    let chosen = select(&shared, &repo(), "pull_request", Some(NUMBER), &matches());
    assert!(chosen.is_ok_and(|chosen| chosen.images.iter().all(|(_, picked)| *picked)));
}

#[test]
fn a_file_renamed_out_of_an_image_still_selects_it() {
    let github = pull(&[files(
        &["docs/old-worker.txt"],
        &[("docs/old-worker.txt", "docker/worker/notes.txt")],
    )]);
    let chosen = select(&github, &repo(), "pull_request", Some(NUMBER), &matches());
    assert!(chosen.is_ok_and(|chosen| {
        chosen.images == vec![("api".to_owned(), false), ("worker".to_owned(), true)]
    }));
}

#[test]
fn any_other_event_and_a_pull_request_too_large_to_list_select_every_image() {
    let none = FakeGitHub::default();
    let chosen = select(&none, &repo(), "push", None, &matches());
    assert!(chosen.is_ok_and(|chosen| chosen.images.iter().all(|(_, picked)| *picked)));
    let full: Vec<Vec<Value>> = (0..30)
        .map(|_| files(&vec!["docs/a.md"; 100], &[]))
        .collect();
    let big = pull(&full);
    let chosen = select(&big, &repo(), "pull_request", Some(NUMBER), &matches());
    assert!(chosen.is_ok_and(|chosen| chosen.images.iter().all(|(_, picked)| *picked)));
}

#[test]
fn a_pull_request_event_without_a_number_or_with_an_unreadable_listing_is_an_error() {
    let none = FakeGitHub::default();
    assert!(select(&none, &repo(), "pull_request", None, &matches()).is_err());
    assert!(select(&none, &repo(), "pull_request", Some(NUMBER), &matches()).is_err());
}

#[test]
fn the_match_list_of_the_workflow_survives_blank_entries_and_refuses_a_malformed_one() {
    let lines = |text: &str| -> Vec<String> { text.split('\n').map(str::to_owned).collect() };
    let parsed = parse_matches(&lines("api=docker/api/**\nworker=**/Cargo.toml\n"));
    assert_eq!(
        parsed.ok(),
        Some(vec![
            ("api".to_owned(), "docker/api/**".to_owned()),
            ("worker".to_owned(), "**/Cargo.toml".to_owned()),
        ])
    );
    assert!(parse_matches(&lines("api")).is_err());
    assert!(parse_matches(&lines("=x")).is_err());
    assert!(parse_matches(&lines("\n\n")).is_err());
}
