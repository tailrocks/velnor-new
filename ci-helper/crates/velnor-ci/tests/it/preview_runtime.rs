// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::cell::RefCell;

use serde_json::{Value, json};
use velnor_ci::preview::json::digest;
use velnor_ci::preview::runtime::{Host, verify_runtime};

use crate::preview_world::{
    AUTHORITY, FakeAttestor, FakeNet, PARENT, PUBLIC, PUBLIC_RUN, SOURCE, World, darwin_payload,
    linux_payload, lock_blob,
};

pub(crate) type Mutation = (&'static str, fn(&mut World));

fn attestor(world: &World) -> FakeAttestor {
    FakeAttestor {
        output: world.attestation.clone(),
        seen: RefCell::new(Vec::new()),
    }
}

fn run(world: World, host: Host) -> Result<velnor_ci::preview::runtime::Verified, String> {
    let attestor = attestor(&world);
    let net = FakeNet::new(world);
    verify_runtime(&net, &attestor, host, PUBLIC, PUBLIC_RUN, 1).map_err(|e| e.to_string())
}

fn edit(world: &mut World, suffix: &str, change: impl FnOnce(&mut Value)) {
    let path = World::path(suffix);
    let value = world
        .api
        .get_mut(&path)
        .unwrap_or_else(|| unreachable!("{path}"));
    change(value);
}

/// Each mutation breaks one thing the legacy script checks. All of them must be refused.
pub(crate) const MUTATIONS: &[Mutation] = &[
    ("repository id changed", |w| {
        edit(w, "", |v| v["id"] = json!(1))
    }),
    ("default branch is not main", |w| {
        edit(w, "", |v| v["default_branch"] = json!("dev"))
    }),
    ("main moved past the public source", |w| {
        edit(w, "/commits/main", |v| v["sha"] = json!(PARENT))
    }),
    ("public run is for another commit", |w| {
        edit(w, &format!("/actions/runs/{PUBLIC_RUN}/attempts/1"), |v| {
            v["head_sha"] = json!(PARENT)
        })
    }),
    ("public run is a pull request", |w| {
        edit(w, &format!("/actions/runs/{PUBLIC_RUN}/attempts/1"), |v| {
            v["event"] = json!("pull_request")
        })
    }),
    ("public run is another workflow", |w| {
        edit(w, &format!("/actions/runs/{PUBLIC_RUN}/attempts/1"), |v| {
            v["path"] = json!(".github/workflows/ci.yml@refs/heads/main")
        })
    }),
    ("public run failed", |w| {
        edit(w, &format!("/actions/runs/{PUBLIC_RUN}/attempts/1"), |v| {
            v["status"] = json!("completed");
            v["conclusion"] = json!("failure");
        })
    }),
    ("the base lock does not approve the preview", |w| {
        let text = "[runtime-preview-staged]\nschema = 3\n";
        w.api.insert(
            World::path(&format!("/contents/.velnor/generator.lock?ref={PARENT}")),
            lock_blob(text),
        );
    }),
    ("the base lock holds a different preview", |w| {
        let mut other = w.record.clone();
        other["version"] = json!("0.1.83");
        let text: String = std::iter::once("[runtime-preview-staged]\n".to_owned())
            .chain(
                other
                    .as_object()
                    .into_iter()
                    .flatten()
                    .map(|(k, v)| format!("{k} = {v}\n")),
            )
            .collect();
        w.api.insert(
            World::path(&format!("/contents/.velnor/generator.lock?ref={PARENT}")),
            lock_blob(&text),
        );
    }),
    ("the lock is not base64", |w| {
        edit(
            w,
            &format!("/contents/.velnor/generator.lock?ref={PUBLIC}"),
            |v| v["content"] = json!("%%%"),
        )
    }),
    ("the lock size lies", |w| {
        edit(
            w,
            &format!("/contents/.velnor/generator.lock?ref={PUBLIC}"),
            |v| v["size"] = json!(3),
        )
    }),
    ("the preview does not descend from the parent", |w| {
        edit(w, &format!("/compare/{SOURCE}...{PARENT}"), |v| {
            v["status"] = json!("diverged")
        })
    }),
    ("the compare merge base is another commit", |w| {
        edit(w, &format!("/compare/{SOURCE}...{PARENT}"), |v| {
            v["merge_base_commit"]["sha"] = json!(PARENT)
        })
    }),
    ("the preview source is not behind the authority", |w| {
        edit(w, &format!("/compare/{AUTHORITY}...{SOURCE}"), |v| {
            v["status"] = json!("behind")
        })
    }),
    ("ci run failed", |w| {
        edit(w, "/actions/runs/600/attempts/1", |v| {
            v["conclusion"] = json!("failure")
        })
    }),
    ("ci run is on another branch", |w| {
        edit(w, "/actions/runs/600/attempts/1", |v| {
            v["head_branch"] = json!("feature")
        })
    }),
    ("ci run is another repository", |w| {
        edit(w, "/actions/runs/600/attempts/1", |v| {
            v["head_repository"]["full_name"] = json!("evil/velnor-new")
        })
    }),
    ("qualification is not a dispatch", |w| {
        edit(w, "/actions/runs/700/attempts/1", |v| {
            v["event"] = json!("push")
        })
    }),
    ("the Required job failed", |w| {
        edit(
            w,
            "/actions/runs/600/attempts/1/jobs?per_page=100&page=1",
            |v| v["jobs"][0]["conclusion"] = json!("failure"),
        )
    }),
    ("the Required job is for another commit", |w| {
        edit(
            w,
            "/actions/runs/600/attempts/1/jobs?per_page=100&page=1",
            |v| v["jobs"][0]["head_sha"] = json!(PARENT),
        )
    }),
    ("the Required job appears twice", |w| {
        edit(
            w,
            "/actions/runs/600/attempts/1/jobs?per_page=100&page=1",
            |v| {
                let job = v["jobs"][0].clone();
                let mut twin = job;
                twin["id"] = json!(6002);
                v["jobs"]
                    .as_array_mut()
                    .unwrap_or_else(|| unreachable!())
                    .push(twin);
                v["total_count"] = json!(2);
            },
        )
    }),
    ("the job list lies about its total", |w| {
        edit(
            w,
            "/actions/runs/600/attempts/1/jobs?per_page=100&page=1",
            |v| v["total_count"] = json!(2),
        )
    }),
    ("the qualification job is not the pinned one", |w| {
        edit(
            w,
            "/actions/runs/800/attempts/1/jobs?per_page=100&page=1",
            |v| v["jobs"][1]["id"] = json!(1),
        )
    }),
    ("the producer artifact expired", |w| {
        edit(w, "/actions/artifacts/11", |v| v["expired"] = json!(true))
    }),
    ("the producer artifact has another digest", |w| {
        edit(w, "/actions/artifacts/13", |v| {
            v["digest"] = json!("sha256:00")
        })
    }),
    ("the producer artifact belongs to another run", |w| {
        edit(w, "/actions/artifacts/11", |v| {
            v["workflow_run"]["id"] = json!(1)
        })
    }),
    ("the draft job failed", |w| {
        edit(
            w,
            "/actions/runs/500/attempts/2/jobs?per_page=100&page=1",
            |v| v["jobs"][0]["conclusion"] = json!("cancelled"),
        )
    }),
    ("the publisher job id is not the pinned one", |w| {
        edit(w, "/actions/jobs/7003", |v| v["id"] = json!(7004))
    }),
    ("the publisher job ran for another run", |w| {
        edit(w, "/actions/jobs/7003", |v| v["run_id"] = json!(501))
    }),
    ("the tag is lightweight", |w| {
        edit(w, &format!("/git/ref/tags/sdk-preview-{SOURCE}"), |v| {
            v["object"]["type"] = json!("commit")
        })
    }),
    ("the tag points at another commit", |w| {
        edit(
            w,
            &format!("/git/tags/{}", crate::preview_world::TAG_OID),
            |v| v["object"]["sha"] = json!(PARENT),
        )
    }),
    ("the release is not immutable", |w| {
        edit(w, "/releases/9000", |v| v["immutable"] = json!(false))
    }),
    ("the release is a draft", |w| {
        edit(w, "/releases/9000", |v| v["draft"] = json!(true))
    }),
    ("the release has an extra asset", |w| {
        edit(w, "/releases/9000", |v| {
            let extra = json!({"name": "extra", "state": "uploaded", "size": 1, "digest": "sha256:00", "browser_download_url": "x"});
            v["assets"]
                .as_array_mut()
                .unwrap_or_else(|| unreachable!())
                .push(extra);
        })
    }),
    ("a release asset is not uploaded", |w| {
        edit(w, "/releases/9000", |v| {
            v["assets"][2]["state"] = json!("starter")
        })
    }),
    ("a release asset has another url", |w| {
        edit(w, "/releases/9000", |v| {
            v["assets"][2]["browser_download_url"] = json!("https://evil.example/x")
        })
    }),
    ("a payload body differs from its digest", |w| {
        let url = w
            .assets
            .keys()
            .find(|k| k.ends_with("linux-gnu"))
            .cloned()
            .unwrap_or_default();
        let mut tampered = linux_payload();
        tampered[30] = 9;
        w.assets.insert(url, tampered);
    }),
    ("the receipt body differs from its digest", |w| {
        let url = w
            .assets
            .keys()
            .find(|k| k.ends_with("RECEIPT.json"))
            .cloned()
            .unwrap_or_default();
        w.assets.insert(url, b"{}".to_vec());
    }),
    (
        "the attestation certificate names another signer digest",
        |w| {
            w.attestation[0]["verificationResult"]["signature"]["certificate"]["buildSignerDigest"] =
                json!(PARENT)
        },
    ),
    ("the attestation certificate is for another trigger", |w| {
        w.attestation[0]["verificationResult"]["signature"]["certificate"]["buildTrigger"] =
            json!("workflow_dispatch")
    }),
    ("the attestation has a fourth subject", |w| {
        let subject = json!({"name": "extra", "digest": {"sha256": "00"}});
        w.attestation[0]["verificationResult"]["statement"]["subject"]
            .as_array_mut()
            .unwrap_or_else(|| unreachable!())
            .push(subject);
    }),
    ("the attestation covers another payload digest", |w| {
        w.attestation[0]["verificationResult"]["statement"]["subject"][1]["digest"]["sha256"] =
            json!("00")
    }),
    ("there are two attestations", |w| {
        let first = w.attestation[0].clone();
        w.attestation
            .as_array_mut()
            .unwrap_or_else(|| unreachable!())
            .push(first);
    }),
];

#[test]
fn a_valid_world_installs_the_host_payload_and_outputs_both_digests() {
    let verified = run(World::new(), Host::Linux).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(verified.release.linux, linux_payload());
    assert_eq!(verified.release.darwin, darwin_payload());
    assert_eq!(
        verified.outputs(),
        format!(
            "linux_sha256={}\ndarwin_sha256={}\n",
            digest(&linux_payload()),
            digest(&darwin_payload())
        )
    );
    assert_eq!(verified.authority.sha, AUTHORITY);
}

#[test]
fn the_identity_names_the_host_payload_and_the_pinned_authority() {
    let verified = run(World::new(), Host::Darwin).unwrap_or_else(|e| unreachable!("{e}"));
    let identity: Value =
        serde_json::from_slice(&verified.identity()).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(identity["target"], "aarch64-apple-darwin");
    assert_eq!(identity["asset_sha256"], digest(&darwin_payload()));
    assert_eq!(identity["source_commit"], SOURCE);
    assert_eq!(identity["authority"]["authority_sha"], AUTHORITY);
    assert_eq!(identity["authority"]["channel"], "source-preview");
}

#[test]
fn every_broken_check_is_refused() {
    for (name, mutate) in MUTATIONS {
        let mut world = World::new();
        mutate(&mut world);
        assert!(run(world, Host::Linux).is_err(), "accepted: {name}");
    }
}

#[test]
fn the_attestation_is_checked_against_the_pinned_authority_and_the_signer_run() {
    let world = World::new();
    let attestor = attestor(&world);
    let net = FakeNet::new(world);
    verify_runtime(&net, &attestor, Host::Linux, PUBLIC, PUBLIC_RUN, 1)
        .unwrap_or_else(|e| unreachable!("{e}"));
    let seen = attestor.seen.borrow();
    assert_eq!(seen.len(), 1);
    let (subject, authority) = &seen[0];
    assert_eq!(authority.sha, AUTHORITY);
    assert_eq!(
        (subject.signer_run_id, subject.signer_run_attempt),
        (500, 3)
    );
    assert_eq!(subject.source_sha, SOURCE);
}

#[test]
fn nothing_is_installed_when_the_preview_is_not_approved() {
    let mut world = World::new();
    world.api.remove(&World::path(&format!(
        "/contents/.velnor/generator.lock?ref={PARENT}"
    )));
    assert!(run(world, Host::Linux).is_err());
}

#[test]
fn a_lock_without_a_runtime_preview_is_not_a_runtime_verification() {
    let mut world = World::new();
    world.api.insert(
        World::path(&format!("/contents/.velnor/generator.lock?ref={PUBLIC}")),
        lock_blob("[other]\nx = 1\n"),
    );
    world.api.insert(
        World::path(&format!("/contents/.velnor/generator.lock?ref={PARENT}")),
        lock_blob("[other]\nx = 1\n"),
    );
    assert!(run(world, Host::Linux).is_err());
}
