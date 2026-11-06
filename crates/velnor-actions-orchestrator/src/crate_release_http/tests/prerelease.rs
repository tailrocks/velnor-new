// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn release_creation_preserves_the_selected_package_prerelease_state() {
    for (version, prerelease) in [
        ("0.1.1", false),
        ("0.1.1+build.5", false),
        ("0.1.1-alpha.1", true),
        ("0.1.1-alpha.1+build.5", true),
    ] {
        let tag = format!("demo-crate-v{version}");
        let response = serde_json::json!({"tag_name":tag,"draft":false,"prerelease":prerelease,"html_url":format!("https://github.com/example/demo-crate/releases/tag/{tag}")});
        let (origin, worker) = server(vec![reply("201 Created", &response.to_string())]);
        client(origin)
            .create_release(
                "example/demo-crate",
                &tag,
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                version,
            )
            .expect("create release");
        let requests = worker.join().expect("server");
        let body: serde_json::Value =
            serde_json::from_str(requests[0].split_once("\r\n\r\n").expect("body").1)
                .expect("JSON body");
        assert_eq!(body["prerelease"], prerelease, "version {version}");
    }
}

#[test]
fn release_evidence_rejects_wrong_or_missing_prerelease_state() {
    for (version, expected) in [("0.1.1+build.5", false), ("0.1.1-alpha.1+build.5", true)] {
        let tag = format!("demo-crate-v{version}");
        for actual in [Some(expected), Some(!expected), None] {
            let mut response = serde_json::json!({"id":12,"tag_name":tag,"draft":false,"html_url":format!("https://github.com/example/demo-crate/releases/tag/{tag}")});
            if let Some(prerelease) = actual {
                response["prerelease"] = prerelease.into();
            }
            let (origin, worker) = server(vec![reply("200 OK", &response.to_string())]);
            let github = crate::crate_release_github::NativeGitHubAdmissionRead {
                reader: client(origin),
            };
            let evidence = crate::crate_release_github::read_github_release_evidence(
                "example/demo-crate",
                &tag,
                version,
                &github,
            );
            assert_eq!(
                evidence.is_ok(),
                actual == Some(expected),
                "version {version}, state {actual:?}"
            );
            assert_eq!(worker.join().expect("server").len(), 1);
        }
    }
}

#[test]
fn rejects_invalid_writes_before_network() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("listen");
    listener.set_nonblocking(true).expect("nonblocking");
    let client = client(format!(
        "http://{}",
        listener.local_addr().expect("address")
    ));
    for (repo, tag, sha) in [
        (
            "bad/repo/extra",
            "demo-crate-v0.1.1",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ),
        (
            "example/demo-crate",
            "../escape",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ),
        (
            "example/demo-crate",
            "v1.lock",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ),
        ("example/demo-crate", "v1", "not-a-sha"),
    ] {
        let result = client.create_tag(repo, tag, sha);
        assert!(result.is_err());
        assert!(!format!("{result:?}").contains(TOKEN));
        assert!(client.create_release(repo, tag, sha, "0.1.1").is_err());
    }
    assert!(
        client
            .create_release(
                "example/demo-crate",
                "demo-crate-v0.1.1",
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "not-semver"
            )
            .is_err()
    );
    assert_eq!(
        listener
            .accept()
            .expect_err("invalid write must not send")
            .kind(),
        std::io::ErrorKind::WouldBlock
    );
}
