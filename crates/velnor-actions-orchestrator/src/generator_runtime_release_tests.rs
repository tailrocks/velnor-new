//! Pure service-response regressions; fixture digests never become release pins.

use super::*;

fn fixture() -> (serde_json::Value, String, String) {
    let version = env!("CARGO_PKG_VERSION");
    let targets = SUPPORTED_TARGETS;
    let records = targets.iter().enumerate().map(|(index, target)| {
        serde_json::json!({
            "target": target,
            "artifact": format!("https://github.com/{REPOSITORY}/releases/download/v{version}/velnor-actions-{version}-{target}"),
            "sha256": if index == 0 { "a".repeat(64) } else { "b".repeat(64) },
        })
    }).collect::<Vec<_>>();
    let text = serde_json::json!({
        "schema": 1, "version": version, "repository": REPOSITORY,
        "commit": "c".repeat(40), "targets": records,
    })
    .to_string();
    let digest = crate::cover_identity::generator::sha256_hex(text.as_bytes());
    let mut assets = records
        .iter()
        .enumerate()
        .map(|(index, record)| {
            let url = record["artifact"].as_str().expect("fixture URL");
            serde_json::json!({
                "id": index + 2, "name": url.rsplit('/').next(), "state": "uploaded",
                "size": 123, "browser_download_url": url,
                "digest": format!("sha256:{}", record["sha256"].as_str().expect("fixture SHA")),
            })
        })
        .collect::<Vec<_>>();
    assets.push(serde_json::json!({
        "id": 1, "name": MANIFEST_NAME, "state": "uploaded", "size": text.len(),
        "digest": format!("sha256:{digest}"),
        "browser_download_url": format!("https://github.com/{REPOSITORY}/releases/download/v{version}/{MANIFEST_NAME}"),
    }));
    let release = serde_json::json!({
        "id": 9, "tag_name": format!("v{version}"), "draft": false,
        "prerelease": false, "immutable": true, "published_at": "2026-10-03T00:00:00Z",
        "html_url": format!("https://github.com/{REPOSITORY}/releases/tag/v{version}"),
        "target_commitish": "c".repeat(40), "assets": assets,
    });
    (release, text, digest)
}

fn identity(target: &str, sha: char) -> PlanGenerator {
    PlanGenerator {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        target: target.to_owned(),
        sha256: sha.to_string().repeat(64),
    }
}

#[test]
fn distinct_target_binary_digests_share_authenticated_release_membership() {
    let (release, text, digest) = fixture();
    let evidence = AuthenticatedGeneratorRelease::from_service(
        &release,
        &text,
        &digest,
        env!("CARGO_PKG_VERSION"),
        &"c".repeat(40),
    )
    .expect("fixture release");
    let planned = identity("x86_64-unknown-linux-gnu", 'a');
    let actual = identity("aarch64-apple-darwin", 'b');
    assert!(evidence.bind_pair(&planned, &actual).is_ok());
    assert_eq!(
        evidence
            .authorize_execution(&planned, &actual)
            .err()
            .as_deref(),
        Some("generator_executed_workflow_acquisition_missing"),
    );
    for wrong in [
        identity("aarch64-apple-darwin", 'a'),
        identity("x86_64-unknown-linux-gnu", 'b'),
    ] {
        assert!(evidence.bind_pair(&planned, &wrong).is_err());
    }
    let mut wrong = actual;
    wrong.version = "999.1.1".to_owned();
    assert!(evidence.bind_pair(&planned, &wrong).is_err());
}

#[test]
fn tag_resolution_commit_is_required_even_when_release_claims_same_source() {
    let (release, text, digest) = fixture();
    assert!(
        AuthenticatedGeneratorRelease::from_service(
            &release,
            &text,
            &digest,
            env!("CARGO_PKG_VERSION"),
            &"d".repeat(40),
        )
        .is_err()
    );
    let mut branch_hint = release;
    branch_hint["target_commitish"] = serde_json::json!("main");
    assert!(
        AuthenticatedGeneratorRelease::from_service(
            &branch_hint,
            &text,
            &digest,
            env!("CARGO_PKG_VERSION"),
            &"c".repeat(40),
        )
        .is_ok()
    );
}

#[test]
fn service_release_authority_rejects_missing_mutable_foreign_and_changed_evidence() {
    let (release, text, digest) = fixture();
    for (pointer, replacement) in [
        ("/immutable", serde_json::Value::Null),
        ("/immutable", serde_json::json!(false)),
        ("/draft", serde_json::json!(true)),
        ("/prerelease", serde_json::json!(true)),
        (
            "/html_url",
            serde_json::json!("https://github.com/attacker/repo/releases/tag/v0.1.0"),
        ),
        ("/assets/0/digest", serde_json::Value::Null),
        ("/assets/1/id", serde_json::json!(2)),
        (
            "/assets/0/digest",
            serde_json::json!(format!("sha256:{}", "d".repeat(64))),
        ),
        (
            "/assets/0/browser_download_url",
            serde_json::json!("https://attacker.invalid/asset"),
        ),
        (
            "/assets/3/browser_download_url",
            serde_json::json!("https://attacker.invalid/release-manifest.json"),
        ),
    ] {
        let mut wrong = release.clone();
        *wrong.pointer_mut(pointer).expect("fixture field") = replacement;
        assert!(
            AuthenticatedGeneratorRelease::from_service(
                &wrong,
                &text,
                &digest,
                env!("CARGO_PKG_VERSION"),
                &"c".repeat(40),
            )
            .is_err(),
            "{pointer}"
        );
    }
    assert!(
        AuthenticatedGeneratorRelease::from_service(
            &release,
            &(text + " "),
            &digest,
            env!("CARGO_PKG_VERSION"),
            &"c".repeat(40),
        )
        .is_err()
    );
}

#[test]
fn authenticated_partial_target_manifest_still_rejects() {
    let (mut release, text, _) = fixture();
    let mut manifest: serde_json::Value = serde_json::from_str(&text).expect("fixture manifest");
    assert!(
        manifest["targets"]
            .as_array_mut()
            .expect("fixture targets")
            .pop()
            .is_some()
    );
    let text = manifest.to_string();
    let digest = crate::cover_identity::generator::sha256_hex(text.as_bytes());
    release["assets"][3]["digest"] = serde_json::json!(format!("sha256:{digest}"));
    assert_eq!(
        AuthenticatedGeneratorRelease::from_service(
            &release,
            &text,
            &digest,
            env!("CARGO_PKG_VERSION"),
            &"c".repeat(40),
        )
        .err()
        .as_deref(),
        Some("generator_release_targets_incomplete")
    );
}

#[test]
fn duplicated_and_missing_assets_cannot_construct_authority() {
    let (mut release, text, digest) = fixture();
    let duplicate = release["assets"][0].clone();
    release["assets"]
        .as_array_mut()
        .expect("fixture assets")
        .push(duplicate);
    assert!(
        AuthenticatedGeneratorRelease::from_service(
            &release,
            &text,
            &digest,
            env!("CARGO_PKG_VERSION"),
            &"c".repeat(40),
        )
        .is_err()
    );
    release["assets"] = serde_json::json!([]);
    assert!(
        AuthenticatedGeneratorRelease::from_service(
            &release,
            &text,
            &digest,
            env!("CARGO_PKG_VERSION"),
            &"c".repeat(40),
        )
        .is_err()
    );
}

#[test]
fn tag_source_resolver_handles_lightweight_annotated_and_cyclic_refs() {
    let version = env!("CARGO_PKG_VERSION");
    let reference = |kind: &str| {
        serde_json::json!({
            "ref": format!("refs/tags/v{version}"),
            "object": {"type": kind, "sha": "c".repeat(40)},
        })
        .to_string()
    };
    assert_eq!(
        resolve_tag_with(version, |_| Ok(reference("commit"))).ok(),
        Some("c".repeat(40))
    );
    let mut calls = Vec::new();
    let resolved = resolve_tag_with(version, |endpoint| {
        calls.push(endpoint.to_owned());
        if endpoint.contains("/git/ref/") {
            Ok(reference("tag"))
        } else {
            Ok(serde_json::json!({
                "sha": "c".repeat(40), "object": {"type": "commit", "sha": "d".repeat(40)},
            })
            .to_string())
        }
    });
    assert_eq!(resolved.ok(), Some("d".repeat(40)));
    assert_eq!(
        calls,
        [
            format!("repos/{REPOSITORY}/git/ref/tags/v{version}"),
            format!("repos/{REPOSITORY}/git/tags/{}", "c".repeat(40)),
        ]
    );
    for kind in ["tag", "tree", "blob"] {
        assert!(
            resolve_tag_with(version, |endpoint| {
                if endpoint.contains("/git/ref/") {
                    Ok(reference(kind))
                } else {
                    Ok(serde_json::json!({
                        "sha": "c".repeat(40), "object": {"type": "tag", "sha": "c".repeat(40)},
                    })
                    .to_string())
                }
            })
            .is_err()
        );
    }
    for (field, value) in [
        ("/ref", serde_json::json!("refs/tags/latest")),
        ("/object/sha", serde_json::json!("0".repeat(40))),
        ("/object/sha", serde_json::json!("main")),
    ] {
        let mut wrong: serde_json::Value =
            serde_json::from_str(&reference("commit")).expect("fixture ref");
        *wrong.pointer_mut(field).expect("fixture field") = value;
        assert!(resolve_tag_with(version, |_| Ok(wrong.to_string())).is_err());
    }
}
