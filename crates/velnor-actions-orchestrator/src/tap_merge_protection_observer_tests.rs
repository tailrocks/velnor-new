//! Independent observer identity, fixed read routes and writer-requester controls.
use super::*;
use crate::tap_merge::github::ApiResponse;
use serde_json::json;
use std::collections::BTreeMap;

fn scope_api(mut changes: BTreeMap<String, Value>) -> TapMergeGitHub {
    let defaults = [
        ("apps/observer", json!({"id":31,"slug":"observer"})),
        ("repos/owner/tap", json!({"id":41,"full_name":"owner/tap"})),
        (
            "installation/repositories?per_page=100&page=1",
            json!({"total_count":1,"repositories":[{"id":41,"full_name":"owner/tap"}]}),
        ),
    ];
    for (key, value) in defaults {
        changes.entry(key.to_owned()).or_insert(value);
    }
    TapMergeGitHub::with_test_transport(move |args| {
        assert!(!args.iter().any(|arg| arg == "--method"));
        let route = args.first().expect("route").to_string_lossy();
        let value = changes.get(route.as_ref()).expect("fixed identity route");
        Ok(ApiResponse {
            status: 200,
            body: serde_json::to_vec(value).expect("JSON"),
        })
    })
}

#[test]
fn observer_authentication_requires_exact_app_and_single_selected_tap() {
    authenticate_scope(&scope_api(BTreeMap::new()), "owner/tap", 41, 31, "observer")
        .expect("scope");
    for (route, value) in [
        ("apps/observer", json!({"id":32,"slug":"observer"})),
        ("apps/observer", json!({"id":31,"slug":"foreign"})),
        ("repos/owner/tap", json!({"id":42,"full_name":"owner/tap"})),
        (
            "repos/owner/tap",
            json!({"id":41,"full_name":"foreign/tap"}),
        ),
        (
            "installation/repositories?per_page=100&page=1",
            json!({"total_count":2,"repositories":[{"id":41,"full_name":"owner/tap"}]}),
        ),
        (
            "installation/repositories?per_page=100&page=1",
            json!({"total_count":1,"repositories":[{"id":41,"full_name":"foreign/tap"}]}),
        ),
    ] {
        let api = scope_api(BTreeMap::from([(route.to_owned(), value)]));
        assert!(authenticate_scope(&api, "owner/tap", 41, 31, "observer").is_err());
    }
}

#[test]
fn observer_exposes_only_its_bound_classic_read_and_forbidden_is_not_absent() {
    for status in [404, 403] {
        let api = TapMergeGitHub::with_test_transport(move |args| {
            assert_eq!(args.len(), 1);
            assert_eq!(args[0], "repos/owner/tap/branches/main/protection");
            Ok(ApiResponse {
                status,
                body: b"{}".to_vec(),
            })
        });
        let observer = ProtectionObserver {
            api,
            repository: "owner/tap".to_owned(),
            encoded_branch: "main".to_owned(),
        };
        assert!(observer.classic_protection("foreign/tap", "main").is_err());
        assert!(observer.classic_protection("owner/tap", "other").is_err());
        let result = observer.classic_protection("owner/tap", "main");
        if status == 404 {
            assert!(result.expect("authenticated absence").is_none());
        } else {
            assert!(result.is_err());
        }
    }
}

#[test]
fn observer_token_must_differ_from_writer_source_and_empty() {
    validate_distinct_tokens("observer", "writer", "source").expect("independent");
    for token in ["", "writer", "source"] {
        assert!(validate_distinct_tokens(token, "writer", "source").is_err());
    }
}

#[test]
fn observer_app_installation_and_deadline_are_independent_without_a_renewed_budget() {
    validate_identity(31, "observer", 51, 17, 52).expect("independent identities");
    for (app, slug, install) in [
        (0, "observer", 51),
        (31, "observer", 0),
        (17, "observer", 51),
        (15_368, "observer", 51),
        (31, "observer", 52),
        (31, "bad/slug", 51),
    ] {
        assert!(validate_identity(app, slug, install, 17, 52).is_err());
    }
    let deadline = Instant::now() + std::time::Duration::from_secs(5);
    let api = TapMergeGitHub::with_deadline("observer".to_owned(), deadline);
    assert_eq!(api.deadline(), deadline);
}
