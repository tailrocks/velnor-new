// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_ci::publish::{Request, publish};

use crate::release_fake::{
    BOT, FakeReleases, Intrusion, Moment, REPO, TAG, all_files, name, request,
};
use crate::support::repo;

fn refusal(fake: &FakeReleases, request: &Request) -> String {
    match publish(fake, &repo(), request) {
        Ok(said) => unreachable!("published: {said}"),
        Err(said) => said.to_string(),
    }
}

#[test]
fn release_publish_keeps_the_draft_when_a_file_vanishes_after_its_upload() {
    let fake = FakeReleases::new();
    let id = fake.app_draft();
    fake.intrude(
        Moment::FirstUpload,
        Intrusion::Remove("velnor-ci-linux-arm64"),
    );
    assert_eq!(
        refusal(&fake, &request(&all_files())),
        format!("draft release {TAG} ({id}) stays a draft: it does not hold velnor-ci-linux-arm64")
    );
    assert!(fake.is_draft(id));
}

#[test]
fn release_publish_refuses_a_draft_whose_upload_url_names_another_release() {
    let fake = FakeReleases::new();
    let other = fake.release("velnor-gen-v0.1.80", true, BOT, "Bot");
    let id = fake.app_draft();
    fake.upload_into(id, other);
    assert_eq!(
        refusal(&fake, &request(&all_files())),
        format!(
            "draft release {TAG} ({id}) has the upload URL https://uploads.github.com/repos/{REPO}/releases/{other}/assets{{?name,label}}, which does not name it"
        )
    );
    for release in [id, other] {
        assert!(fake.is_draft(release));
        assert_eq!(fake.assets(release), BTreeMap::new());
    }
}

#[test]
fn release_publish_fails_when_github_answers_the_publish_with_a_draft() {
    let fake = FakeReleases::new();
    let id = fake.app_draft();
    fake.intrude(Moment::Publish, Intrusion::KeepDraft);
    assert_eq!(
        refusal(&fake, &request(&all_files())),
        format!("GitHub did not publish draft release {TAG} ({id})")
    );
    assert!(fake.is_draft(id));
}

#[test]
fn release_publish_refuses_sha256sums_named_as_one_of_the_files() {
    let fake = FakeReleases::new();
    let id = fake.app_draft();
    let mut named = request(&all_files());
    named.names.push(name("SHA256SUMS"));
    assert_eq!(
        refusal(&fake, &named),
        "SHA256SUMS is published with every release; do not name it as a file"
    );
    assert_eq!(fake.requests(), 0);
    assert!(fake.is_draft(id));
}
