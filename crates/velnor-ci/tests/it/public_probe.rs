// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_ci::public_probe::{Verdicts, bearer_allowed};
use velnor_model::GitRepository;

fn repository(owner: &str, name: &str) -> GitRepository {
    GitRepository {
        owner: owner.to_owned(),
        name: name.to_owned(),
    }
}

#[test]
fn only_the_exact_github_api_host_over_https_gets_the_token() {
    for url in [
        "https://api.github.com/repos/acme/shared-lib",
        "https://api.github.com",
        "https://api.github.com:443/repos/acme/shared-lib",
    ] {
        assert!(bearer_allowed(url), "{url}");
    }
}

#[test]
fn every_other_url_is_refused_the_token() {
    for url in [
        "https://api.github.com.evil.com/repos/acme/shared-lib",
        "https://evil.com/api.github.com/repos/acme/shared-lib",
        "https://evil.com/?x=https://api.github.com/",
        "http://api.github.com/repos/acme/shared-lib",
        "https://api.github.com:8443/repos/acme/shared-lib",
        "https://api.github.com:/repos/acme/shared-lib",
        "https://user@api.github.com/repos/acme/shared-lib",
        "https://user:pass@api.github.com/repos/acme/shared-lib",
        "https://evil.com@api.github.com/repos/acme/shared-lib",
        "https://api.github.com@evil.com/repos/acme/shared-lib",
        "https://api.github.com.@evil.com/",
        "https://xapi.github.com/repos/acme/shared-lib",
        "https://github.com/repos/acme/shared-lib",
        "https://API.GITHUB.COM/repos/acme/shared-lib",
        "https://api.github.com\\@evil.com/",
        "http://127.0.0.1:8080",
        "//api.github.com/repos/acme/shared-lib",
        "api.github.com/repos/acme/shared-lib",
        "ftp://api.github.com/repos/acme/shared-lib",
        "",
    ] {
        assert!(!bearer_allowed(url), "{url}");
    }
}

#[test]
fn a_verdict_skips_the_lookup_only_when_it_says_public() {
    let verdicts = Verdicts::parse(r#"{"acme/shared-lib":true,"acme/closed":false}"#)
        .unwrap_or_else(|| unreachable!("valid verdicts"));
    assert!(verdicts.proves_public(&repository("acme", "shared-lib")));
    assert!(verdicts.proves_public(&repository("ACME", "Shared-Lib")));
    assert!(!verdicts.proves_public(&repository("acme", "closed")));
    assert!(!verdicts.proves_public(&repository("acme", "unknown")));
}

#[test]
fn an_unreadable_verdict_is_no_verdict() {
    for text in [
        "",
        "not json",
        "[]",
        r#"{"acme/shared-lib":"true"}"#,
        r#"{"acme/shared-lib":1}"#,
        r#"{"acme/shared-lib":true"#,
        "null",
    ] {
        assert!(Verdicts::parse(text).is_none(), "{text}");
    }
}

#[test]
fn verdicts_round_trip_sorted_and_lower_case() {
    let mut verdicts = Verdicts::default();
    verdicts.record(&repository("Zed", "Lib"), true);
    verdicts.record(&repository("acme", "closed"), false);
    let text = verdicts.render();
    assert_eq!(text, r#"{"acme/closed":false,"zed/lib":true}"#);
    assert_eq!(Verdicts::parse(&text), Some(verdicts));
}
