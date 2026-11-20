// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_ci::publish::{Request, publish};

use crate::release_fake::{FILES, FakeReleases, TAG, all_files, name, published_digests, request};
use crate::support::repo;

const OPTIONAL: &str = "velnor-ci-macos-arm64";

fn optional_request(files: &[(&str, &str)]) -> Request {
    let required = FILES.iter().filter(|(file, _)| *file != OPTIONAL);
    Request {
        names: required.map(|(file, _)| name(file)).collect(),
        optional: vec![name(OPTIONAL)],
        ..request(files)
    }
}

fn without_the_optional_file() -> Vec<(&'static str, String)> {
    all_files()
        .into_iter()
        .filter(|(file, _)| *file != OPTIONAL)
        .map(|(file, text)| {
            let text = if file == "SHA256SUMS" {
                text.lines()
                    .filter(|line| !line.ends_with(OPTIONAL))
                    .map(|line| format!("{line}\n"))
                    .collect()
            } else {
                text.to_owned()
            };
            (file, text)
        })
        .collect()
}

fn borrowed<'a>(files: &'a [(&'static str, String)]) -> Vec<(&'static str, &'a str)> {
    files
        .iter()
        .map(|(file, text)| (*file, text.as_str()))
        .collect()
}

fn refusal(fake: &FakeReleases, request: &Request) -> String {
    match publish(fake, &repo(), request) {
        Ok(said) => unreachable!("published: {said}"),
        Err(said) => said.to_string(),
    }
}

#[test]
fn an_optional_file_that_is_missing_is_skipped_with_a_notice() {
    let fake = FakeReleases::new();
    let id = fake.app_draft();
    let files = without_the_optional_file();
    let said =
        publish(&fake, &repo(), &optional_request(&borrowed(&files))).map_err(|e| e.to_string());
    assert_eq!(
        said,
        Ok(format!(
            "skipped optional {OPTIONAL}: not found\nuploaded velnor-ci-linux-arm64\nuploaded velnor-ci-linux-x64\nuploaded SHA256SUMS\npublished release {TAG} ({id}) with 3 files\n"
        ))
    );
    assert!(!fake.is_draft(id));
    let held = fake.assets(id);
    assert_eq!(held.len(), 3);
    assert!(!held.contains_key(OPTIONAL));
}

#[test]
fn an_optional_file_that_is_present_is_published_and_checked_like_a_required_one() {
    let fake = FakeReleases::new();
    let id = fake.app_draft();
    let said = publish(&fake, &repo(), &optional_request(&all_files())).map_err(|e| e.to_string());
    assert_eq!(
        said,
        Ok(format!(
            "uploaded velnor-ci-linux-arm64\nuploaded velnor-ci-linux-x64\nuploaded velnor-ci-macos-arm64\nuploaded SHA256SUMS\npublished release {TAG} ({id}) with 4 files\n"
        ))
    );
    assert!(!fake.is_draft(id));
    assert_eq!(fake.assets(id), published_digests());

    let fake = FakeReleases::new();
    let id = fake.app_draft();
    let tampered: Vec<(&str, &str)> = all_files()
        .into_iter()
        .map(|(file, text)| (file, if file == OPTIONAL { "swapped\n" } else { text }))
        .collect();
    let said = refusal(&fake, &optional_request(&tampered));
    assert!(said.contains(&format!("{OPTIONAL}: FAILED")), "{said}");
    assert!(fake.is_draft(id));
    assert_eq!(fake.assets(id), BTreeMap::new());
}

#[test]
fn an_optional_file_that_the_checksums_list_but_is_missing_stays_a_failure() {
    let fake = FakeReleases::new();
    let id = fake.app_draft();
    let files: Vec<(&str, &str)> = all_files()
        .into_iter()
        .filter(|(file, _)| *file != OPTIONAL)
        .collect();
    let said = refusal(&fake, &optional_request(&files));
    assert!(
        said.contains(&format!(
            "{OPTIONAL}: listed in SHA256SUMS and not a file of this release"
        )),
        "{said}"
    );
    assert!(fake.is_draft(id));
    assert_eq!(fake.assets(id), BTreeMap::new());
}

#[test]
fn an_optional_file_that_is_present_but_unlisted_stays_a_failure() {
    let fake = FakeReleases::new();
    let id = fake.app_draft();
    let files = without_the_optional_file();
    let mut present = borrowed(&files);
    present.push((OPTIONAL, "velnor-ci for macos-arm64\n"));
    let said = refusal(&fake, &optional_request(&present));
    assert!(
        said.contains(&format!("{OPTIONAL}: not listed in SHA256SUMS")),
        "{said}"
    );
    assert!(fake.is_draft(id));
}

#[test]
fn a_required_file_that_is_missing_still_fails_even_beside_an_optional_one() {
    let fake = FakeReleases::new();
    let id = fake.app_draft();
    let files: Vec<(&str, &str)> = all_files()
        .into_iter()
        .filter(|(file, _)| *file != "velnor-ci-linux-arm64")
        .collect();
    let said = refusal(&fake, &optional_request(&files));
    assert!(said.contains("velnor-ci-linux-arm64: missing"), "{said}");
    assert!(fake.is_draft(id));
    assert_eq!(fake.assets(id), BTreeMap::new());
}

#[test]
fn an_optional_file_cannot_repeat_a_named_file_or_be_the_checksums() {
    let fake = FakeReleases::new();
    fake.app_draft();
    let mut twice = optional_request(&all_files());
    twice.names.push(name(OPTIONAL));
    assert_eq!(
        refusal(&fake, &twice),
        format!("file {OPTIONAL} is named twice")
    );
    let mut sums = optional_request(&all_files());
    sums.optional.push(name("SHA256SUMS"));
    assert_eq!(
        refusal(&fake, &sums),
        "SHA256SUMS is published with every release; do not name it as a file"
    );
}

#[test]
fn only_optional_files_never_make_a_release() {
    let fake = FakeReleases::new();
    fake.app_draft();
    let only = Request {
        names: Vec::new(),
        ..optional_request(&all_files())
    };
    assert_eq!(refusal(&fake, &only), "name at least one file to publish");
}
