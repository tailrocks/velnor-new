// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_ci::publish::{Request, publish};

use crate::release_fake::{BOT, FakeReleases, INTRUDER, TAG, name, request};
use crate::support::repo;

fn assetless() -> Request {
    Request {
        names: Vec::new(),
        files: BTreeMap::new(),
        assetless: true,
        ..request(&[])
    }
}

fn refusal(fake: &FakeReleases, request: &Request) -> String {
    match publish(fake, &repo(), request) {
        Ok(said) => unreachable!("published: {said}"),
        Err(said) => said.to_string(),
    }
}

#[test]
fn an_assetless_release_publishes_the_one_empty_draft_the_app_created() {
    let fake = FakeReleases::new();
    fake.others(150);
    let id = fake.app_draft();
    let said = publish(&fake, &repo(), &assetless()).map_err(|e| e.to_string());
    assert_eq!(
        said,
        Ok(format!("published release {TAG} ({id}) with 0 files\n"))
    );
    assert!(!fake.is_draft(id));
    assert_eq!(fake.assets(id), BTreeMap::new());
}

#[test]
fn an_assetless_release_refuses_a_draft_that_holds_any_asset() {
    let fake = FakeReleases::new();
    let id = fake.app_draft();
    fake.asset(id, "planted.bin", b"planted");
    assert_eq!(
        refusal(&fake, &assetless()),
        format!("draft release {TAG} ({id}) holds planted.bin, which is not one of its files")
    );
    assert!(fake.is_draft(id));
}

#[test]
fn an_assetless_release_keeps_the_one_draft_and_app_author_guards() {
    let fake = FakeReleases::new();
    let id = fake.release(TAG, true, INTRUDER, "User");
    assert_eq!(
        refusal(&fake, &assetless()),
        format!("draft release {TAG} ({id}) was created by {INTRUDER} (User), not by {BOT}")
    );
    let fake = FakeReleases::new();
    let first = fake.app_draft();
    let second = fake.release(TAG, true, INTRUDER, "User");
    assert_eq!(
        refusal(&fake, &assetless()),
        format!(
            "2 draft releases have the tag {TAG} ({first} by {BOT} (Bot), {second} by {INTRUDER} (User)); only one, created by {BOT}, may"
        )
    );
    assert!(fake.is_draft(first) && fake.is_draft(second));
}

#[test]
fn naming_files_for_an_assetless_release_or_none_for_a_release_with_assets_is_refused() {
    let fake = FakeReleases::new();
    let id = fake.app_draft();
    let mut named = assetless();
    named.names.push(name("velnor-ci-linux-x64"));
    assert_eq!(
        refusal(&fake, &named),
        "an assetless release names no files"
    );
    let mut none = assetless();
    none.assetless = false;
    assert_eq!(refusal(&fake, &none), "name at least one file to publish");
    assert_eq!(fake.requests(), 0);
    assert!(fake.is_draft(id));
}
