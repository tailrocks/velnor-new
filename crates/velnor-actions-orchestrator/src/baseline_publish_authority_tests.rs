use super::*;
use std::collections::BTreeMap;

fn runner() -> BTreeMap<&'static str, String> {
    BTreeMap::from([
        ("GITHUB_EVENT_NAME", "push".to_owned()),
        ("GITHUB_REF_PROTECTED", "true".to_owned()),
        ("GITHUB_REPOSITORY", "o/r".to_owned()),
        ("GITHUB_REF", "refs/heads/testmain".to_owned()),
        ("GITHUB_SHA", "a".repeat(40)),
        ("GITHUB_WORKFLOW_SHA", "a".repeat(40)),
        (
            "GITHUB_WORKFLOW_REF",
            "o/r/.github/workflows/ci.yml@refs/heads/testmain".to_owned(),
        ),
    ])
}

fn request() -> PublishRequest {
    serde_json::from_value(serde_json::json!({
        "schema": 1,
        "event": "push",
        "head": "a".repeat(40),
        "repository": "o/r",
        "git_ref": "refs/heads/testmain",
        "default_branch": "testmain",
    }))
    .expect("request")
}

#[test]
fn only_actual_protected_push_can_publish() {
    let request = request();
    let original = runner();
    assert!(
        RunnerPublicationAuthority::from_reader(
            |key| original.get(key).cloned(),
            Some("testmain".to_owned())
        )
        .verify(&request)
        .is_ok()
    );
    for protected in [None, Some("false"), Some("TRUE"), Some("1"), Some("")] {
        let mut values = original.clone();
        match protected {
            Some(value) => {
                values.insert("GITHUB_REF_PROTECTED", value.to_owned());
            }
            None => {
                values.remove("GITHUB_REF_PROTECTED");
            }
        }
        assert!(
            RunnerPublicationAuthority::from_reader(
                |key| values.get(key).cloned(),
                Some("testmain".to_owned())
            )
            .verify(&request)
            .is_err(),
            "{protected:?}"
        );
    }
    for event in [
        "pull_request",
        "merge_group",
        "workflow_dispatch",
        "schedule",
        "",
    ] {
        let mut values = original.clone();
        values.insert("GITHUB_EVENT_NAME", event.to_owned());
        assert!(
            RunnerPublicationAuthority::from_reader(
                |key| values.get(key).cloned(),
                Some("testmain".to_owned())
            )
            .verify(&request)
            .is_err(),
            "{event}"
        );
    }
}

#[test]
fn staged_request_cannot_choose_runner_source() {
    let request = request();
    for key in [
        "GITHUB_REPOSITORY",
        "GITHUB_REF",
        "GITHUB_SHA",
        "GITHUB_WORKFLOW_SHA",
        "GITHUB_WORKFLOW_REF",
    ] {
        for value in [None, Some("other")] {
            let mut values = runner();
            match value {
                Some(value) => {
                    values.insert(key, value.to_owned());
                }
                None => {
                    values.remove(key);
                }
            }
            assert!(
                RunnerPublicationAuthority::from_reader(
                    |name| values.get(name).cloned(),
                    Some("testmain".to_owned())
                )
                .verify(&request)
                .is_err(),
                "{key}: {value:?}"
            );
        }
    }
}

#[test]
fn staged_default_branch_cannot_promote_a_protected_topic() {
    let mut values = runner();
    values.insert("GITHUB_REF", "refs/heads/topic".to_owned());
    values.insert(
        "GITHUB_WORKFLOW_REF",
        "o/r/.github/workflows/ci.yml@refs/heads/topic".to_owned(),
    );
    let mut forged = request();
    forged.git_ref = Some("refs/heads/topic".to_owned());
    forged.default_branch = Some("topic".to_owned());
    let authority = RunnerPublicationAuthority::from_reader(
        |key| values.get(key).cloned(),
        Some("testmain".to_owned()),
    );
    assert!(authority.verify(&forged).is_err());
    for branch in [None, Some(""), Some("bad branch"), Some("topic")] {
        let authority = RunnerPublicationAuthority::from_reader(
            |key| runner().get(key).cloned(),
            branch.map(str::to_owned),
        );
        assert!(authority.verify(&request()).is_err(), "{branch:?}");
    }
}

#[test]
fn repository_slug_case_does_not_change_authority() {
    let mut values = runner();
    values.insert("GITHUB_REPOSITORY", "O/R".to_owned());
    values.insert(
        "GITHUB_WORKFLOW_REF",
        "O/R/.github/workflows/ci.yml@refs/heads/testmain".to_owned(),
    );
    let authority = RunnerPublicationAuthority::from_reader(
        |key| values.get(key).cloned(),
        Some("testmain".to_owned()),
    );
    assert!(authority.verify(&request()).is_ok());
}
