// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_ci::error::CiError;
use velnor_ci::github::{GitHub, release_by_tag_path, tag_ref_path};
use velnor_ci::release::PlainName;
use velnor_ci::release_recover::{Recovery, recover};

use crate::support::{FakeGitHub, repo};

const SHA: &str = "2222222222222222222222222222222222222222";
const RELEASES: &str = "/repos/tailrocks/velnor-new/releases?per_page=100&page=1";

fn name(text: &str) -> PlainName {
    PlainName::new("package", text).unwrap_or_else(|e| unreachable!("{e}"))
}

fn tag(text: &str) -> PlainName {
    PlainName::tag(text).unwrap_or_else(|e| unreachable!("{e}"))
}

fn expected() -> BTreeMap<String, String> {
    BTreeMap::from([
        ("app".to_owned(), "app-v1.0.0".to_owned()),
        ("tools".to_owned(), "tools-v2.0.0".to_owned()),
    ])
}

fn release(tag: &str, draft: bool, login: &str, kind: &str) -> String {
    format!(
        r#"{{"id":7,"tag_name":"{tag}","draft":{draft},"author":{{"login":"{login}","type":"{kind}"}},"upload_url":"https://uploads.github.com/repos/tailrocks/velnor-new/releases/7/assets{{?name,label}}"}}"#
    )
}

fn tagged(fake: &mut FakeGitHub, tag: &str) {
    fake.respond(
        tag_ref_path(&repo(), tag),
        &format!(r#"{{"object":{{"type":"commit","sha":"{SHA}"}}}}"#),
    );
}

fn unpublished(fake: &mut FakeGitHub, tag: &str, releases: &[String]) {
    tagged(fake, tag);
    fake.missing(release_by_tag_path(&repo(), tag));
    fake.respond(RELEASES.to_owned(), &format!("[{}]", releases.join(",")));
}

fn recovery_of(github: &dyn GitHub, recorded: &[(&str, &str)]) -> Result<Recovery, String> {
    let recorded = recorded
        .iter()
        .map(|(package, tag_name)| (name(package), tag(tag_name)))
        .collect();
    recover(
        github,
        &repo(),
        &name("velnor-release"),
        &expected(),
        &recorded,
    )
    .map_err(|e| e.to_string())
}

fn run(fake: &FakeGitHub, recorded: &[(&str, &str)]) -> Result<Vec<(String, String)>, String> {
    recovery_of(fake, recorded).map(|recovery| {
        recovery
            .found
            .iter()
            .map(|(package, tag)| (package.to_string(), tag.to_string()))
            .collect()
    })
}

struct ReadOnlyToken<'a>(&'a FakeGitHub);

impl GitHub for ReadOnlyToken<'_> {
    fn get(&self, path: &str) -> Result<String, CiError> {
        let body = self.0.get(path)?;
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&body) else {
            return Ok(body);
        };
        match value {
            serde_json::Value::Array(releases) => Ok(serde_json::Value::Array(
                releases.into_iter().filter(|r| !is_draft(r)).collect(),
            )
            .to_string()),
            release if is_draft(&release) => {
                Err(CiError::missing(format!("GET {path}: http status: 404")))
            }
            _ => Ok(body),
        }
    }
}

fn is_draft(release: &serde_json::Value) -> bool {
    release.get("draft").and_then(serde_json::Value::as_bool) == Some(true)
}

fn app_draft(tag: &str) -> String {
    release(tag, true, "velnor-release[bot]", "Bot")
}

#[test]
fn a_draft_the_app_created_for_a_tag_release_plz_skipped_is_recovered() {
    let mut fake = FakeGitHub::default();
    unpublished(&mut fake, "app-v1.0.0", &[app_draft("app-v1.0.0")]);
    assert_eq!(
        run(&fake, &[("tools", "tools-v2.0.0")]),
        Ok(vec![("app".to_owned(), "app-v1.0.0".to_owned())])
    );
}

#[test]
fn every_package_release_plz_did_not_record_is_recovered_when_it_has_a_draft() {
    let mut fake = FakeGitHub::default();
    let drafts = [app_draft("app-v1.0.0"), app_draft("tools-v2.0.0")];
    unpublished(&mut fake, "app-v1.0.0", &drafts);
    unpublished(&mut fake, "tools-v2.0.0", &drafts);
    assert_eq!(
        run(&fake, &[]),
        Ok(vec![
            ("app".to_owned(), "app-v1.0.0".to_owned()),
            ("tools".to_owned(), "tools-v2.0.0".to_owned()),
        ])
    );
}

#[test]
fn a_recorded_package_is_not_looked_at_again() {
    let fake = FakeGitHub::default();
    assert_eq!(
        run(&fake, &[("app", "app-v1.0.0"), ("tools", "tools-v2.0.0")]),
        Ok(Vec::new())
    );
}

#[test]
fn a_tag_that_does_not_exist_or_a_release_already_published_recovers_nothing() {
    let mut fake = FakeGitHub::default();
    fake.missing(tag_ref_path(&repo(), "app-v1.0.0"));
    tagged(&mut fake, "tools-v2.0.0");
    fake.respond(
        release_by_tag_path(&repo(), "tools-v2.0.0"),
        &release("tools-v2.0.0", false, "velnor-release[bot]", "Bot"),
    );
    assert_eq!(run(&fake, &[]), Ok(Vec::new()));
}

#[test]
fn a_tag_with_no_release_at_all_recovers_nothing() {
    let mut fake = FakeGitHub::default();
    unpublished(&mut fake, "app-v1.0.0", &[]);
    assert_eq!(run(&fake, &[("tools", "tools-v2.0.0")]), Ok(Vec::new()));
}

#[test]
fn a_draft_somebody_else_created_or_a_second_draft_is_refused() {
    let mut fake = FakeGitHub::default();
    unpublished(
        &mut fake,
        "app-v1.0.0",
        &[release("app-v1.0.0", true, "tailrocks-reviewer", "User")],
    );
    assert_eq!(
        run(&fake, &[("tools", "tools-v2.0.0")]),
        Err("draft release app-v1.0.0 (7) was created by tailrocks-reviewer (User), not by velnor-release[bot]".to_owned())
    );
    let mut fake = FakeGitHub::default();
    unpublished(
        &mut fake,
        "app-v1.0.0",
        &[app_draft("app-v1.0.0"), app_draft("app-v1.0.0")],
    );
    let said = run(&fake, &[("tools", "tools-v2.0.0")])
        .err()
        .unwrap_or_default();
    assert!(
        said.starts_with("2 draft releases have the tag app-v1.0.0"),
        "{said}"
    );
}

#[test]
fn a_failed_lookup_that_is_not_a_missing_page_is_refused() {
    let fake = FakeGitHub::default();
    let said = run(&fake, &[("tools", "tools-v2.0.0")])
        .err()
        .unwrap_or_default();
    assert!(said.contains("no recorded response"), "{said}");
}

#[test]
fn a_tag_with_no_draft_is_reported_so_the_skip_is_visible() {
    let mut fake = FakeGitHub::default();
    unpublished(&mut fake, "app-v1.0.0", &[]);
    let recovery = recovery_of(&fake, &[("tools", "tools-v2.0.0")]).unwrap_or_default();
    assert!(recovery.found.is_empty());
    assert_eq!(
        recovery
            .without_draft
            .iter()
            .map(|(package, tag)| (package.to_string(), tag.to_string()))
            .collect::<Vec<_>>(),
        vec![("app".to_owned(), "app-v1.0.0".to_owned())]
    );
}

#[test]
fn a_token_that_cannot_see_drafts_recovers_nothing_while_one_with_push_access_does() {
    let mut fake = FakeGitHub::default();
    unpublished(&mut fake, "app-v1.0.0", &[app_draft("app-v1.0.0")]);
    let with_push = recovery_of(&fake, &[("tools", "tools-v2.0.0")]).unwrap_or_default();
    assert_eq!(with_push.found.len(), 1);
    assert!(with_push.without_draft.is_empty());
    let read_only =
        recovery_of(&ReadOnlyToken(&fake), &[("tools", "tools-v2.0.0")]).unwrap_or_default();
    assert!(read_only.found.is_empty());
    assert_eq!(read_only.without_draft.len(), 1);
}
