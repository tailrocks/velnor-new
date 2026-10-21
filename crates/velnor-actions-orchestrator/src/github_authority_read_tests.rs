use super::arguments;
use std::{collections::BTreeMap, ffi::OsString};

fn route(args: &[OsString]) -> String {
    args[0].to_str().expect("route").to_owned()
}

#[test]
fn repeated_authority_reads_have_distinct_cache_keys_and_revalidation_headers() {
    let first = arguments("repos/owner/repo/git/ref/heads/main").expect("first");
    let second = arguments("repos/owner/repo/git/ref/heads/main").expect("second");
    assert_ne!(route(&first), route(&second));
    for args in [&first, &second] {
        assert!(
            args.windows(2)
                .any(|pair| pair[0] == "-H" && pair[1] == "Cache-Control: no-cache, no-store")
        );
        assert!(
            args.windows(2)
                .any(|pair| pair[0] == "-H" && pair[1] == "Pragma: no-cache")
        );
    }
}

#[test]
fn existing_queries_and_pages_are_preserved() {
    for original in [
        "repos/owner/repo/actions/runs?head_sha=abc&branch=release%2Fnext&per_page=100&page=2",
        "repos/owner/repo/pulls/1/reviews?per_page=100&page=3",
    ] {
        let args = arguments(original).expect("query");
        assert!(route(&args).starts_with(&format!("{original}&_velnor_read_nonce=")));
    }
}

#[test]
fn ambiguous_or_reserved_routes_fail_closed() {
    for value in [
        "repos/a/b#fragment",
        "https://api.github.com/repos/a/b",
        "repos/a/b?%5Fvelnor_read_nonce=old",
        "repos/a/b?_velnor_read_nonce=old",
        "repos/a/b?x=1&_velnor_read_nonce=old",
        "repos/a/b\\escape",
        "repos/a/b\n",
    ] {
        assert!(arguments(value).is_err(), "{value:?}");
    }
}

fn cached_observation(args: &[OsString], cache: &mut BTreeMap<String, bool>, origin: bool) -> bool {
    let revalidate = args
        .windows(2)
        .any(|pair| pair[0] == "-H" && pair[1] == "Cache-Control: no-cache, no-store");
    if revalidate {
        return origin;
    }
    *cache.entry(route(args)).or_insert(origin)
}

#[test]
fn cache_aware_source_and_unchanged_head_review_reads_observe_new_authority() {
    for endpoint in [
        "repos/owner/repo/branches/main",
        "repos/owner/repo/pulls/1/reviews?per_page=100&page=1",
    ] {
        let mut cache = BTreeMap::new();
        assert!(!cached_observation(
            &arguments(endpoint).expect("before"),
            &mut cache,
            false
        ));
        assert!(cached_observation(
            &arguments(endpoint).expect("after"),
            &mut cache,
            true
        ));
    }
}

#[test]
fn mutations_and_graphql_keep_exact_original_arguments() {
    for raw in [
        vec!["repos/owner/repo/releases", "--method", "POST"],
        vec!["repos/owner/repo/releases/1", "--method", "PATCH"],
        vec![
            "repos/owner/repo/pulls/1/merge",
            "--method",
            "PUT",
            "--raw-field",
            "sha=exact",
        ],
        vec!["repos/owner/repo/releases/assets/1", "--method", "DELETE"],
        vec!["graphql", "--raw-field", "query=query { viewer { login } }"],
    ] {
        let original = raw.into_iter().map(OsString::from).collect::<Vec<_>>();
        assert_eq!(
            super::for_request(original.clone()).expect("unchanged"),
            original
        );
    }
}

#[test]
fn unsupported_get_options_cannot_silently_bypass_freshness() {
    for raw in [
        vec!["repos/owner/repo/pulls/1", "--method", "GET"],
        vec!["repos/owner/repo/pulls/1", "-H", "Accept: application/json"],
        vec![
            "graphql",
            "--raw-field",
            "query=query { viewer { login } }",
            "--method",
            "GET",
        ],
        vec![
            "repos/owner/repo/releases",
            "--method",
            "POST",
            "--method",
            "GET",
        ],
    ] {
        assert!(super::for_request(raw.into_iter().map(OsString::from).collect()).is_err());
    }
}
