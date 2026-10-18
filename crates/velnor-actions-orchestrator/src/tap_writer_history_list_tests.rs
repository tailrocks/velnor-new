use super::super::TapGitHubApi;
use super::all_pulls;
use crate::tap_prepare::writer::github::ScriptedResponse;

#[test]
fn pull_listing_rejects_more_than_the_requested_page_size() {
    let entries = (1..=101).map(pull_summary).collect::<Vec<_>>();
    assert_listing_rejected(&entries);
}

#[test]
fn pull_listing_rejects_duplicate_numbers() {
    assert_listing_rejected(&[pull_summary(12), pull_summary(12)]);
}

fn assert_listing_rejected(entries: &[serde_json::Value]) {
    let api = scripted_listing(entries);
    assert!(all_pulls(&api).is_err());
    assert!(api.scripted_is_empty());
}

fn scripted_listing(entries: &[serde_json::Value]) -> TapGitHubApi {
    let response = ScriptedResponse {
        method: "GET",
        route: "repos/owner/homebrew-tap/pulls?state=all&per_page=100&page=1".to_owned(),
        body: serde_json::to_vec(&entries).expect("pull list JSON"),
    };
    TapGitHubApi::scripted("owner/homebrew-tap".to_owned(), [response])
}

fn pull_summary(number: u64) -> serde_json::Value {
    serde_json::json!({
        "number": number,
        "title": "historical candidate",
        "body": null,
        "state": "closed",
        "user": {"id": 7, "login": "writer[bot]"},
        "head": {"sha": "a".repeat(40), "label": "owner:branch", "ref": "branch", "repo": {"full_name": "owner/homebrew-tap"}},
        "base": {"sha": "b".repeat(40), "label": "owner:main", "ref": "main", "repo": {"full_name": "owner/homebrew-tap"}}
    })
}
