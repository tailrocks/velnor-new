// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_ci::publish::publish;

use crate::release_fake::{
    BOT, FakeReleases, INTRUDER, Intrusion, Moment, SUMS, TAG, all_files, published_digests,
    request,
};
use crate::support::repo;

const TAMPERED: &str = "92e78d0b032962f47792a9fa95fd981ef63e1e3ef074d536d6304c75eddbe29f";

fn run(fake: &FakeReleases, files: &[(&str, &str)]) -> Result<String, String> {
    publish(fake, &repo(), &request(files)).map_err(|e| e.to_string())
}

fn refusal(fake: &FakeReleases, files: &[(&str, &str)]) -> String {
    match run(fake, files) {
        Ok(said) => unreachable!("published: {said}"),
        Err(said) => said,
    }
}

fn untouched(fake: &FakeReleases, id: u64) {
    assert!(fake.is_draft(id), "release {id} was published");
    assert_eq!(fake.assets(id), BTreeMap::new(), "release {id} got assets");
}

fn replaced(file: &str, text: &str) -> Vec<(&'static str, String)> {
    all_files()
        .into_iter()
        .map(|(name, body)| {
            (
                name,
                if name == file {
                    text.to_owned()
                } else {
                    body.to_owned()
                },
            )
        })
        .collect()
}

fn borrowed<'a>(files: &'a [(&'static str, String)]) -> Vec<(&'static str, &'a str)> {
    files
        .iter()
        .map(|(name, text)| (*name, text.as_str()))
        .collect()
}

#[test]
fn release_publish_uploads_every_file_and_un_drafts_only_the_app_draft() {
    let fake = FakeReleases::new();
    fake.others(150);
    let other = fake.release("velnor-gen-v0.1.80", true, BOT, "Bot");
    let id = fake.app_draft();
    assert_eq!(
        run(&fake, &all_files()),
        Ok(format!(
            "uploaded velnor-ci-linux-arm64\nuploaded velnor-ci-linux-x64\nuploaded velnor-ci-macos-arm64\nuploaded SHA256SUMS\npublished release {TAG} ({id}) with 4 files\n"
        ))
    );
    assert!(!fake.is_draft(id));
    assert_eq!(fake.assets(id), published_digests());
    untouched(&fake, other);
}

#[test]
fn release_publish_refuses_a_second_draft_with_the_same_tag_on_any_page() {
    let fake = FakeReleases::new();
    let id = fake.app_draft();
    fake.others(120);
    let second = fake.release(TAG, true, INTRUDER, "User");
    assert_eq!(
        refusal(&fake, &all_files()),
        format!(
            "2 draft releases have the tag {TAG} ({id} by {BOT} (Bot), {second} by {INTRUDER} (User)); only one, created by {BOT}, may"
        )
    );
    untouched(&fake, id);
    untouched(&fake, second);

    let fake = FakeReleases::new();
    let first = fake.app_draft();
    let second = fake.app_draft();
    assert!(
        refusal(&fake, &all_files()).starts_with("2 draft releases have the tag"),
        "two drafts by the App"
    );
    untouched(&fake, first);
    untouched(&fake, second);
}

#[test]
fn release_publish_refuses_a_draft_that_the_app_did_not_create() {
    for (login, kind) in [
        (INTRUDER, "User"),
        ("github-actions[bot]", "Bot"),
        (BOT, "User"),
        ("velnor-release-evil[bot]", "Bot"),
        ("other-velnor-release[bot]", "Bot"),
        ("velnor-releas[bot]", "Bot"),
    ] {
        let fake = FakeReleases::new();
        let id = fake.release(TAG, true, login, kind);
        assert_eq!(
            refusal(&fake, &all_files()),
            format!("draft release {TAG} ({id}) was created by {login} ({kind}), not by {BOT}")
        );
        untouched(&fake, id);
    }
}

#[test]
fn release_publish_refuses_a_tag_whose_release_is_already_published() {
    let fake = FakeReleases::new();
    let published = fake.release(TAG, false, BOT, "Bot");
    let id = fake.app_draft();
    assert_eq!(
        refusal(&fake, &all_files()),
        format!("release {TAG} is already published ({published}), and a tag is published once")
    );
    untouched(&fake, id);
}

#[test]
fn release_publish_refuses_a_foreign_asset_before_and_while_it_uploads() {
    let fake = FakeReleases::new();
    let id = fake.app_draft();
    let planted = fake.asset(id, "velnor-ci-linux-x64.sig", b"planted");
    assert_eq!(
        refusal(&fake, &all_files()),
        format!(
            "draft release {TAG} ({id}) holds velnor-ci-linux-x64.sig, which is not one of its files"
        )
    );
    assert!(fake.is_draft(id));
    assert_eq!(
        fake.assets(id).into_keys().collect::<Vec<_>>(),
        ["velnor-ci-linux-x64.sig"],
        "asset {planted}"
    );

    let fake = FakeReleases::new();
    let id = fake.app_draft();
    fake.intrude(Moment::FirstUpload, Intrusion::Add("extra"));
    assert_eq!(
        refusal(&fake, &all_files()),
        format!(
            "draft release {TAG} ({id}) stays a draft: it holds extra, which is not one of its files"
        )
    );
    assert!(fake.is_draft(id));

    let fake = FakeReleases::new();
    let id = fake.app_draft();
    fake.intrude(Moment::Publish, Intrusion::Add("extra"));
    assert_eq!(
        refusal(&fake, &all_files()),
        format!(
            "release {TAG} ({id}) is published, and it holds extra, which is not one of its files; check it by hand"
        )
    );
}

#[test]
fn release_publish_refuses_a_missing_file_before_it_asks_github() {
    let without_macos: Vec<(&str, &str)> = all_files()
        .into_iter()
        .filter(|(name, _)| *name != "velnor-ci-macos-arm64")
        .collect();
    let unlisted: String = SUMS.split_inclusive('\n').take(2).collect();
    let unlisted = replaced("SHA256SUMS", &unlisted);
    let extra = replaced(
        "SHA256SUMS",
        &format!("{SUMS}{}  velnor-ci-windows-x64\n", "0".repeat(64)),
    );
    let without_sums: Vec<(&str, &str)> = all_files()
        .into_iter()
        .filter(|(name, _)| *name != "SHA256SUMS")
        .collect();
    let cases = [
        (without_macos, "velnor-ci-macos-arm64: missing".to_owned()),
        (
            borrowed(&unlisted),
            "velnor-ci-macos-arm64: not listed in SHA256SUMS".to_owned(),
        ),
        (
            borrowed(&extra),
            "velnor-ci-windows-x64: listed in SHA256SUMS and not a file of this release".to_owned(),
        ),
        (without_sums, "SHA256SUMS: missing".to_owned()),
    ];
    for (files, said) in cases {
        let fake = FakeReleases::new();
        let id = fake.app_draft();
        assert_eq!(refusal(&fake, &files), said);
        assert_eq!(fake.requests(), 0, "{said}");
        untouched(&fake, id);
    }
}

#[test]
fn release_publish_refuses_a_file_whose_digest_does_not_match() {
    let fake = FakeReleases::new();
    let id = fake.app_draft();
    let tampered = replaced("velnor-ci-linux-x64", "tampered\n");
    assert_eq!(
        refusal(&fake, &borrowed(&tampered)),
        format!(
            "velnor-ci-linux-x64: FAILED, its SHA-256 is {TAMPERED} and SHA256SUMS lists 2dcc907cafb22be7ea157626eb896a9573a4b7e2ed37aae7f0e6408edd94d528"
        )
    );
    assert_eq!(fake.requests(), 0);
    untouched(&fake, id);

    let fake = FakeReleases::new();
    let id = fake.app_draft();
    fake.intrude(
        Moment::FirstUpload,
        Intrusion::Swap("velnor-ci-linux-arm64"),
    );
    assert_eq!(
        refusal(&fake, &all_files()),
        format!(
            "draft release {TAG} ({id}) stays a draft: its velnor-ci-linux-arm64 is not the 26 bytes with SHA-256 fbd8a7732f157b60fd479fb395efc6062051664f97f928ef552d7aed93145c42 that SHA256SUMS lists"
        )
    );
    assert!(fake.is_draft(id));
}

#[test]
fn release_publish_reads_sha256sums_as_strictly_as_sha256sum_check_strict() {
    let first = SUMS.lines().next().unwrap_or_default();
    let rest: String = SUMS.split_inclusive('\n').skip(1).collect();
    let binary = format!("{} *{}\n{rest}", &first[..64], &first[66..]);
    let fake = FakeReleases::new();
    let id = fake.app_draft();
    assert!(
        run(&fake, &borrowed(&replaced("SHA256SUMS", &binary))).is_ok(),
        "the binary-mode marker of sha256sum is read"
    );
    assert!(!fake.is_draft(id));
    let malformed = [
        (
            format!("{}{}\n{rest}", first[..64].to_uppercase(), &first[64..]),
            "line 1",
        ),
        (
            format!("{} {}\n{rest}", &first[..64], &first[66..]),
            "line 1",
        ),
        (format!("{first}\r\n{rest}"), "line 1"),
        (format!("{first}\n\n{rest}"), "line 2"),
        (
            format!("SHA256 ({}) = {}\n{rest}", &first[66..], &first[..64]),
            "line 1",
        ),
        (SUMS.trim_end().to_owned(), "does not end with a newline"),
        (String::new(), "is empty"),
        (
            format!("{first}\n{SUMS}"),
            "lists velnor-ci-linux-x64 twice",
        ),
    ];
    for (text, said) in malformed {
        let fake = FakeReleases::new();
        let id = fake.app_draft();
        let error = refusal(&fake, &borrowed(&replaced("SHA256SUMS", &text)));
        assert!(error.contains(said), "{text:?}: {error}");
        assert!(error.contains("SHA256SUMS"), "{text:?}: {error}");
        untouched(&fake, id);
    }
}

#[test]
fn release_publish_replaces_a_planted_asset_and_keeps_one_an_earlier_attempt_uploaded() {
    let fake = FakeReleases::new();
    let id = fake.app_draft();
    let planted = fake.asset(id, "velnor-ci-linux-x64", b"planted");
    fake.asset(id, "SHA256SUMS", SUMS.as_bytes());
    assert_eq!(
        run(&fake, &all_files()),
        Ok(format!(
            "uploaded velnor-ci-linux-arm64\nremoved velnor-ci-linux-x64 ({planted}), which did not hold the bytes SHA256SUMS lists\nuploaded velnor-ci-linux-x64\nuploaded velnor-ci-macos-arm64\nkept SHA256SUMS\npublished release {TAG} ({id}) with 4 files\n"
        ))
    );
    assert!(!fake.is_draft(id));
    assert_eq!(fake.assets(id), published_digests());
}
