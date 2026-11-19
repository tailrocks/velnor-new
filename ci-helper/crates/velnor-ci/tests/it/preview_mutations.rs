// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::{Value, json};

use crate::preview_world::{
    AUTHORITY, PARENT, PUBLIC, PUBLIC_RUN, SOURCE, World, linux_payload, lock_blob,
};

pub(crate) type Mutation = (&'static str, fn(&mut World));

fn put(world: &mut World, suffix: &str, pointer: &str, value: Value) {
    edit(world, suffix, |v| {
        if let Some(slot) = v.pointer_mut(pointer) {
            *slot = value;
        } else {
            unreachable!("{suffix} {pointer}");
        }
    });
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
    ("repository id changed", |w| put(w, "", "/id", json!(1))),
    ("default branch is not main", |w| {
        put(w, "", "/default_branch", json!("dev"));
    }),
    ("main moved past the public source", |w| {
        put(w, "/commits/main", "/sha", json!(PARENT));
    }),
    ("public run is for another commit", |w| {
        edit(w, &format!("/actions/runs/{PUBLIC_RUN}/attempts/1"), |v| {
            v["head_sha"] = json!(PARENT);
        });
    }),
    ("public run is a pull request", |w| {
        edit(w, &format!("/actions/runs/{PUBLIC_RUN}/attempts/1"), |v| {
            v["event"] = json!("pull_request");
        });
    }),
    ("public run is another workflow", |w| {
        edit(w, &format!("/actions/runs/{PUBLIC_RUN}/attempts/1"), |v| {
            v["path"] = json!(".github/workflows/ci.yml@refs/heads/main");
        });
    }),
    ("public run failed", |w| {
        edit(w, &format!("/actions/runs/{PUBLIC_RUN}/attempts/1"), |v| {
            v["status"] = json!("completed");
            v["conclusion"] = json!("failure");
        });
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
            |v| {
                v["content"] = json!("%%%");
            },
        );
    }),
    ("the lock size lies", |w| {
        edit(
            w,
            &format!("/contents/.velnor/generator.lock?ref={PUBLIC}"),
            |v| {
                v["size"] = json!(3);
            },
        );
    }),
    ("the preview does not descend from the parent", |w| {
        edit(w, &format!("/compare/{SOURCE}...{PARENT}"), |v| {
            v["status"] = json!("diverged");
        });
    }),
    ("the compare merge base is another commit", |w| {
        edit(w, &format!("/compare/{SOURCE}...{PARENT}"), |v| {
            v["merge_base_commit"]["sha"] = json!(PARENT);
        });
    }),
    ("the preview source is not behind the authority", |w| {
        edit(w, &format!("/compare/{AUTHORITY}...{SOURCE}"), |v| {
            v["status"] = json!("behind");
        });
    }),
    ("ci run failed", |w| {
        edit(w, "/actions/runs/600/attempts/1", |v| {
            v["conclusion"] = json!("failure");
        });
    }),
    ("ci run is on another branch", |w| {
        edit(w, "/actions/runs/600/attempts/1", |v| {
            v["head_branch"] = json!("feature");
        });
    }),
    ("ci run is another repository", |w| {
        edit(w, "/actions/runs/600/attempts/1", |v| {
            v["head_repository"]["full_name"] = json!("evil/velnor-new");
        });
    }),
    ("qualification is not a dispatch", |w| {
        edit(w, "/actions/runs/700/attempts/1", |v| {
            v["event"] = json!("push");
        });
    }),
    ("the Required job failed", |w| {
        edit(
            w,
            "/actions/runs/600/attempts/1/jobs?per_page=100&page=1",
            |v| {
                v["jobs"][0]["conclusion"] = json!("failure");
            },
        );
    }),
    ("the Required job is for another commit", |w| {
        edit(
            w,
            "/actions/runs/600/attempts/1/jobs?per_page=100&page=1",
            |v| {
                v["jobs"][0]["head_sha"] = json!(PARENT);
            },
        );
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
        );
    }),
    ("the job list lies about its total", |w| {
        edit(
            w,
            "/actions/runs/600/attempts/1/jobs?per_page=100&page=1",
            |v| {
                v["total_count"] = json!(2);
            },
        );
    }),
    ("the qualification job is not the pinned one", |w| {
        edit(
            w,
            "/actions/runs/800/attempts/1/jobs?per_page=100&page=1",
            |v| {
                v["jobs"][1]["id"] = json!(1);
            },
        );
    }),
    ("the producer artifact expired", |w| {
        put(w, "/actions/artifacts/11", "/expired", json!(true));
    }),
    ("the producer artifact has another digest", |w| {
        edit(w, "/actions/artifacts/13", |v| {
            v["digest"] = json!("sha256:00");
        });
    }),
    ("the producer artifact belongs to another run", |w| {
        edit(w, "/actions/artifacts/11", |v| {
            v["workflow_run"]["id"] = json!(1);
        });
    }),
    ("the draft job failed", |w| {
        edit(
            w,
            "/actions/runs/500/attempts/2/jobs?per_page=100&page=1",
            |v| {
                v["jobs"][0]["conclusion"] = json!("cancelled");
            },
        );
    }),
    ("the publisher job id is not the pinned one", |w| {
        put(w, "/actions/jobs/7003", "/id", json!(7004));
    }),
    ("the publisher job ran for another run", |w| {
        put(w, "/actions/jobs/7003", "/run_id", json!(501));
    }),
    ("the tag is lightweight", |w| {
        edit(w, &format!("/git/ref/tags/sdk-preview-{SOURCE}"), |v| {
            v["object"]["type"] = json!("commit");
        });
    }),
    ("the tag points at another commit", |w| {
        edit(
            w,
            &format!("/git/tags/{}", crate::preview_world::TAG_OID),
            |v| {
                v["object"]["sha"] = json!(PARENT);
            },
        );
    }),
    ("the release is not immutable", |w| {
        put(w, "/releases/9000", "/immutable", json!(false));
    }),
    ("the release is a draft", |w| {
        put(w, "/releases/9000", "/draft", json!(true));
    }),
    ("the release has an extra asset", |w| {
        edit(w, "/releases/9000", |v| {
            let extra = json!({"name": "extra", "state": "uploaded", "size": 1, "digest": "sha256:00", "browser_download_url": "x"});
            v["assets"]
                .as_array_mut()
                .unwrap_or_else(|| unreachable!())
                .push(extra);
        });
    }),
    ("a release asset is not uploaded", |w| {
        edit(w, "/releases/9000", |v| {
            v["assets"][2]["state"] = json!("starter");
        });
    }),
    ("a release asset has another url", |w| {
        edit(w, "/releases/9000", |v| {
            v["assets"][2]["browser_download_url"] = json!("https://evil.example/x");
        });
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
                json!(PARENT);
        },
    ),
    ("the attestation certificate is for another trigger", |w| {
        w.attestation[0]["verificationResult"]["signature"]["certificate"]["buildTrigger"] =
            json!("workflow_dispatch");
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
            json!("00");
    }),
    ("there are two attestations", |w| {
        let first = w.attestation[0].clone();
        w.attestation
            .as_array_mut()
            .unwrap_or_else(|| unreachable!())
            .push(first);
    }),
];
