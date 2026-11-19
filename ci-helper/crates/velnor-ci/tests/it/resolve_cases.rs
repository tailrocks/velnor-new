// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! The recorded cases: the successes of every phase and one case per refusal the legacy
//! `resolve-qualification-v1` op has.

use serde_json::{Value, json};

use crate::resolve_world::{
    MEMBER, Node, Quirk, REPO, WORKFLOW, World, artifact_route, branch_route, data_dir, list_route,
    repo_route, run_route, zip_route, zip_with,
};

pub(crate) type Case = (&'static str, World);

fn made(name: &'static str, phase: &str, change: impl FnOnce(&mut World)) -> Case {
    let mut world = World::new(phase);
    change(&mut world);
    (name, world)
}

fn drop_key(value: &mut Value, key: &str) {
    if let Some(object) = value.as_object_mut() {
        object.remove(key);
    }
}

fn request_text(world: &World) -> String {
    world.request.to_string()
}

pub(crate) fn cases() -> Vec<Case> {
    let mut all = successes();
    all.extend(requests());
    all.extend(dispatches());
    all.extend(repository_and_branch());
    all.extend(runs());
    all.extend(artifacts());
    all.extend(archives());
    all.extend(receipts());
    all.extend(lineage());
    all.extend(deltas());
    all.extend(staging());
    all
}

fn successes() -> Vec<Case> {
    vec![
        made("cold-has-no-predecessor", "cold", |_| {}),
        made("control-has-no-predecessor", "control", |_| {}),
        made("warm", "warm", |_| {}),
        made("third", "third", |_| {}),
        made("useful-delta", "useful_delta", |_| {}),
        made("run-path-with-branch-suffix", "warm", |w| {
            w.api_edit(&run_route(1001), |v| {
                v["path"] = json!(format!("{WORKFLOW}@main"));
            });
        }),
        made("run-path-with-ref-suffix", "warm", |w| {
            w.api_edit(&run_route(1001), |v| {
                v["path"] = json!(format!("{WORKFLOW}@refs/heads/main"));
            });
        }),
    ]
}

fn requests() -> Vec<Case> {
    let raw = |name: &'static str, bytes: &'static [u8]| {
        made(name, "warm", move |w| {
            w.request_bytes = Some(bytes.to_vec());
        })
    };
    let field = |name: &'static str, change: fn(&mut Value)| {
        made(name, "warm", move |w| change(&mut w.request))
    };
    vec![
        made("request-at-another-path", "warm", |w| {
            w.request_name = Some("other.json".to_owned());
        }),
        made("request-is-a-symlink", "warm", |w| {
            w.quirk = Some(Quirk::RequestIsASymlink);
        }),
        raw("request-not-utf8", b"\xff\xfe"),
        raw("request-duplicate-key", br#"{"schema":1,"schema":1}"#),
        raw("request-not-json", b"{"),
        made("request-trailing-content", "warm", |w| {
            w.request_bytes = Some(format!("{} x", request_text(w)).into_bytes());
        }),
        made("request-nested-too-deep", "warm", |w| {
            w.request_bytes =
                Some(format!("{{\"a\":{}{}}}", "[".repeat(130), "]".repeat(130)).into_bytes());
        }),
        field("request-unknown-field", |v| v["extra"] = json!(1)),
        field("request-without-repository", |v| {
            drop_key(v, "repository");
        }),
        field("request-schema-is-a-string", |v| v["schema"] = json!("1")),
        field("request-schema-2", |v| v["schema"] = json!(2)),
        field("request-op-is-not-plan", |v| v["op"] = json!("plan-v2")),
        field("request-event-is-push", |v| v["event"] = json!("push")),
        field("request-root-is-not-dot", |v| v["root"] = json!("sub")),
        field("request-repository-differs", |v| {
            v["repository"] = json!("other/repo");
        }),
        field("request-head-differs", |v| {
            v["head"] = json!("b".repeat(40));
        }),
        made("no-runner-temp", "warm", |w| w.unset.push("RUNNER_TEMP")),
    ]
}

fn dispatches() -> Vec<Case> {
    let context = |name: &'static str, phase: &'static str, change: fn(&mut Value)| {
        made(name, phase, move |w| w.dispatch(change))
    };
    vec![
        context("dispatch-campaign-invalid", "warm", |c| {
            c["campaign"] = json!("Bad Camp");
        }),
        context("dispatch-sha-uppercase", "warm", |c| {
            let upper = c["source_sha"].as_str().unwrap_or("").to_uppercase();
            c["source_sha"] = json!(upper);
            c["workflow_sha"] = json!(upper);
        }),
        context("dispatch-sha-differs-from-workflow-sha", "warm", |c| {
            c["workflow_sha"] = json!("c".repeat(40));
        }),
        context("dispatch-ref-unprotected", "warm", |c| {
            c["ref_protected"] = json!(false);
        }),
        context("dispatch-workflow-ref-wrong", "warm", |c| {
            c["workflow_ref"] = json!(format!("{REPO}/.github/workflows/x.yml@refs/heads/main"));
        }),
        context("dispatch-run-id-zero", "warm", |c| c["run_id"] = json!(0)),
        context("dispatch-cold-with-predecessor", "cold", |c| {
            c["predecessor"] = json!({"run_id": 5, "run_attempt": 1});
        }),
        context("dispatch-warm-without-predecessor", "warm", |c| {
            drop_key(c, "predecessor");
        }),
        context("dispatch-predecessor-run-zero", "warm", |c| {
            c["predecessor"] = json!({"run_id": 0, "run_attempt": 1});
        }),
        context(
            "dispatch-default-branch-is-not-the-repository-default",
            "warm",
            |c| {
                c["default_branch"] = json!("dev");
                c["git_ref"] = json!("refs/heads/dev");
                c["workflow_ref"] = json!(format!("{REPO}/{WORKFLOW}@refs/heads/dev"));
            },
        ),
        context("dispatch-unknown-field", "warm", |c| c["extra"] = json!(1)),
        made("runner-run-id-differs", "warm", |w| {
            w.env.insert("GITHUB_RUN_ID".to_owned(), "2002".to_owned());
        }),
        made("runner-repository-differs", "warm", |w| {
            w.env
                .insert("GITHUB_REPOSITORY".to_owned(), "other/repo".to_owned());
        }),
        made("runner-campaign-input-differs", "warm", |w| {
            w.payload["inputs"]["campaign"] = json!("camp-other");
        }),
        made("runner-predecessor-input-differs", "warm", |w| {
            w.payload["inputs"]["predecessor_run_id"] = json!("1099");
        }),
    ]
}

fn repository_and_branch() -> Vec<Case> {
    vec![
        made("repository-api-unavailable", "warm", |w| {
            w.api_drop(&repo_route());
        }),
        made("repository-api-not-json", "warm", |w| {
            w.api_raw(&repo_route(), b"nope");
        }),
        made("repository-api-other-name", "warm", |w| {
            w.api_edit(&repo_route(), |v| v["full_name"] = json!("other/repo"));
        }),
        made("repository-api-without-default-branch", "warm", |w| {
            w.api_edit(&repo_route(), |v| {
                drop_key(v, "default_branch");
            });
        }),
        made("repository-api-default-branch-differs", "warm", |w| {
            w.api_edit(&repo_route(), |v| v["default_branch"] = json!("dev"));
        }),
        made("repository-api-default-branch-invalid", "warm", |w| {
            w.api_edit(&repo_route(), |v| v["default_branch"] = json!("-x"));
        }),
        made("branch-api-unavailable", "warm", |w| {
            w.api_drop(&branch_route());
        }),
        made("branch-api-not-json", "warm", |w| {
            w.api_raw(&branch_route(), b"[");
        }),
        made("branch-unprotected", "warm", |w| {
            w.api_edit(&branch_route(), |v| v["protected"] = json!(false));
        }),
        made("branch-protection-missing", "warm", |w| {
            w.api_edit(&branch_route(), |v| {
                drop_key(v, "protected");
            });
        }),
    ]
}

fn run_case(name: &'static str, change: fn(&mut Value)) -> Case {
    made(name, "warm", move |w| w.api_edit(&run_route(1001), change))
}

fn runs() -> Vec<Case> {
    vec![
        made("run-api-unavailable", "warm", |w| {
            w.api_drop(&run_route(1001));
        }),
        made("run-api-not-json", "warm", |w| {
            w.api_raw(&run_route(1001), b"{");
        }),
        run_case("run-id-differs", |v| v["id"] = json!(7)),
        run_case("run-attempt-differs", |v| v["run_attempt"] = json!(2)),
        run_case("run-event-is-push", |v| v["event"] = json!("push")),
        run_case("run-not-completed", |v| v["status"] = json!("in_progress")),
        run_case("run-failed", |v| v["conclusion"] = json!("failure")),
        run_case("run-without-conclusion", |v| {
            drop_key(v, "conclusion");
        }),
        run_case("run-on-another-branch", |v| v["head_branch"] = json!("dev")),
        run_case("run-without-branch", |v| v["head_branch"] = json!("")),
        run_case("run-from-another-repository", |v| {
            v["head_repository"]["full_name"] = json!("other/repo");
        }),
        run_case("run-without-head-repository", |v| {
            drop_key(v, "head_repository");
        }),
        run_case("run-head-sha-short", |v| v["head_sha"] = json!("abc")),
        run_case("run-without-head-sha", |v| {
            drop_key(v, "head_sha");
        }),
        run_case("run-of-another-workflow", |v| {
            v["path"] = json!(".github/workflows/other.yml");
        }),
        run_case("run-workflow-with-another-ref", |v| {
            v["path"] = json!(format!("{WORKFLOW}@dev"));
        }),
        run_case("run-without-event", |v| {
            drop_key(v, "event");
        }),
        run_case("run-without-workflow-path", |v| {
            drop_key(v, "path");
        }),
    ]
}

fn listed(name: &'static str, change: fn(&mut Value)) -> Case {
    made(name, "warm", move |w| w.api_edit(&list_route(1001), change))
}

fn by_id(name: &'static str, change: fn(&mut Value)) -> Case {
    made(name, "warm", move |w| {
        w.api_edit(&artifact_route(1001), change);
    })
}

fn artifacts() -> Vec<Case> {
    vec![
        made("artifact-list-unavailable", "warm", |w| {
            w.api_drop(&list_route(1001));
        }),
        made("artifact-list-not-json", "warm", |w| {
            w.api_raw(&list_route(1001), b"x");
        }),
        listed("artifact-list-count-differs", |v| {
            v["total_count"] = json!(2);
        }),
        listed("artifact-list-missing", |v| {
            drop_key(v, "artifacts");
        }),
        listed("artifact-list-without-the-receipt", |v| {
            v["artifacts"][0]["name"] = json!("other");
        }),
        listed("artifact-list-with-two-receipts", |v| {
            let copy = v["artifacts"][0].clone();
            if let Some(list) = v["artifacts"].as_array_mut() {
                list.push(copy);
            }
            v["total_count"] = json!(2);
        }),
        listed("artifact-without-run", |v| {
            drop_key(&mut v["artifacts"][0], "workflow_run");
        }),
        listed("artifact-without-id", |v| {
            drop_key(&mut v["artifacts"][0], "id");
        }),
        listed("artifact-id-zero", |v| v["artifacts"][0]["id"] = json!(0)),
        listed("artifact-expired", |v| {
            v["artifacts"][0]["expired"] = json!(true);
        }),
        listed("artifact-without-expiry", |v| {
            drop_key(&mut v["artifacts"][0], "expired");
        }),
        listed("artifact-empty", |v| {
            v["artifacts"][0]["size_in_bytes"] = json!(0);
        }),
        listed("artifact-too-big", |v| {
            v["artifacts"][0]["size_in_bytes"] = json!(600_000);
        }),
        listed("artifact-without-digest", |v| {
            drop_key(&mut v["artifacts"][0], "digest");
        }),
        listed("artifact-without-run-branch", |v| {
            v["artifacts"][0]["workflow_run"]["head_branch"] = json!("");
        }),
        listed("artifact-without-run-id", |v| {
            drop_key(&mut v["artifacts"][0]["workflow_run"], "id");
        }),
        by_id("artifact-by-id-unavailable-body", |v| *v = json!("x")),
        by_id("artifact-by-id-changed", |v| {
            v["digest"] = json!(format!("sha256:{}", "0".repeat(64)));
        }),
        made("artifact-by-id-unavailable", "warm", |w| {
            w.api_drop(&artifact_route(1001));
        }),
        made("artifact-of-another-run", "warm", |w| {
            for route in [list_route(1001), artifact_route(1001)] {
                w.api_edit(&route, |v| {
                    let entry = if v["artifacts"].is_array() {
                        &mut v["artifacts"][0]
                    } else {
                        v
                    };
                    entry["workflow_run"]["head_sha"] = json!("d".repeat(40));
                });
            }
        }),
    ]
}

fn archives() -> Vec<Case> {
    let zipped = |name: &'static str, bytes: Vec<u8>| {
        made(name, "warm", move |w| w.nodes[0].zip_override = Some(bytes))
    };
    let document = |schema: u64| {
        let world = World::new("warm");
        json!({"schema": schema, "producer": world.nodes[0].producer, "receipt": world.nodes[0].receipt})
    };
    let mut corrupt = zip_with(MEMBER, b"AAAAAAAAAAAAAAAA");
    corrupt[30 + MEMBER.len()] ^= 0xff;
    vec![
        made("archive-unavailable", "warm", |w| {
            w.api_drop(&zip_route(1001));
        }),
        made("archive-differs-from-its-digest", "warm", |w| {
            w.api_raw(&zip_route(1001), b"xx");
        }),
        zipped("archive-not-a-zip", b"not a zip file at all".to_vec()),
        zipped("archive-empty", Vec::new()),
        zipped(
            "archive-with-a-duplicate-member",
            std::fs::read(data_dir().join("receipt-duplicate.zip")).unwrap_or_default(),
        ),
        zipped(
            "archive-member-has-another-name",
            zip_with("other.json", b"{}"),
        ),
        zipped("archive-member-is-corrupt", corrupt),
        zipped("archive-receipt-not-json", zip_with(MEMBER, b"not json")),
        zipped(
            "archive-document-schema-3",
            zip_with(MEMBER, document(3).to_string().as_bytes()),
        ),
    ]
}

fn receipt_case(name: &'static str, phase: &'static str, change: fn(&mut Node)) -> Case {
    made(name, phase, move |w| change(&mut w.nodes[0]))
}

fn receipts() -> Vec<Case> {
    vec![
        receipt_case("receipt-unknown-field", "warm", |n| {
            n.receipt["extra"] = json!(1);
        }),
        receipt_case("receipt-run-differs", "warm", |n| {
            n.receipt["run"]["run_id"] = json!(999);
        }),
        receipt_case("producer-run-differs", "warm", |n| {
            n.producer["run"]["run_id"] = json!(999);
        }),
        receipt_case("receipt-plan-id-of-another-run", "warm", |n| {
            n.receipt["plan_id"] = json!("plan-r9-a1");
        }),
        receipt_case("receipt-plan-id-malformed", "warm", |n| {
            n.receipt["plan_id"] = json!("nope");
        }),
        receipt_case("receipt-configuration-digest-malformed", "warm", |n| {
            n.receipt["configuration_digest"] = json!("b3-xyz");
        }),
        receipt_case("receipt-campaign-empty", "warm", |n| {
            n.receipt["campaign"] = json!("");
        }),
        receipt_case("receipt-campaign-has-a-control-character", "warm", |n| {
            n.receipt["campaign"] = json!("a\nb");
        }),
        receipt_case("receipt-source-sha-uppercase", "warm", |n| {
            n.receipt["source_sha"] = json!("A".repeat(40));
        }),
        receipt_case("receipt-schema-1", "warm", |n| {
            n.receipt["schema"] = json!(1);
        }),
        receipt_case("receipt-without-lanes", "warm", |n| {
            n.receipt["lanes"] = json!([]);
        }),
        receipt_case("receipt-lanes-unsorted", "warm", |n| {
            if let Some(lanes) = n.receipt["lanes"].as_array_mut() {
                lanes.reverse();
            }
        }),
        receipt_case("receipt-delta-outside-useful-delta", "warm", |n| {
            n.receipt["source_delta"] = json!({
                "base_source_sha": "a".repeat(40), "source_sha": "b".repeat(40),
                "changed_paths": ["a"], "diff_digest": "b3-x", "base_is_ancestor": true
            });
        }),
        receipt_case("receipt-useful-delta-without-delta", "warm", |n| {
            n.receipt["phase"] = json!("useful_delta");
        }),
        receipt_case("producer-repository-empty", "warm", |n| {
            n.producer["repository"] = json!("");
        }),
        receipt_case("producer-sha-uppercase", "warm", |n| {
            n.producer["source_sha"] = json!("E".repeat(40));
        }),
        receipt_case("receipt-predecessor-link-without-run", "warm", |n| {
            n.receipt["predecessor"] = json!({
                "run": {"run_id": 0, "run_attempt": 1}, "receipt_digest": "b3-x",
                "artifact_id": 1, "artifact_digest": "x"
            });
        }),
    ]
}

fn lineage() -> Vec<Case> {
    vec![
        made("lineage-link-names-another-artifact", "third", |w| {
            w.edit(0, |n| {
                n.receipt["predecessor"]["artifact_digest"] =
                    json!(format!("sha256:{}", "1".repeat(64)));
            });
        }),
        made("lineage-link-names-another-receipt", "third", |w| {
            w.edit(0, |n| {
                n.receipt["predecessor"]["receipt_digest"] =
                    json!(format!("b3-{}", "2".repeat(64)));
            });
        }),
        made("lineage-link-names-a-run-without-a-record", "third", |w| {
            w.edit(0, |n| n.receipt["predecessor"]["run"]["run_id"] = json!(1));
        }),
        made("lineage-is-too-deep", "useful_delta", |w| {
            w.edit(2, |n| {
                n.receipt["predecessor"] = json!({
                    "run": {"run_id": 999, "run_attempt": 1}, "receipt_digest": format!("b3-{}", "3".repeat(64)),
                    "artifact_id": 1, "artifact_digest": format!("sha256:{}", "4".repeat(64))
                });
            });
        }),
        made("lineage-record-of-an-older-run-is-missing", "third", |w| {
            w.api_drop(&run_route(1001));
        }),
    ]
}

fn deltas() -> Vec<Case> {
    vec![
        made("delta-base-is-not-an-ancestor", "useful_delta", |w| {
            let side = w.shas.side.clone();
            w.chain_source(&side);
        }),
        made("delta-base-is-not-in-the-repository", "useful_delta", |w| {
            w.chain_source(&"1".repeat(40));
        }),
        made(
            "delta-source-is-not-in-the-repository",
            "useful_delta",
            |w| {
                w.dispatch_source(&"2".repeat(40));
            },
        ),
        made("delta-is-empty", "useful_delta", |w| {
            let source = w.shas.source.clone();
            w.chain_source(&source);
        }),
        made("delta-has-too-many-paths", "useful_delta", |w| {
            let many = w.shas.many.clone();
            w.dispatch_source(&many);
        }),
        made("delta-base-sha-malformed", "useful_delta", |w| {
            w.chain_source("zz");
        }),
        made("delta-base-sha-not-hex", "useful_delta", |w| {
            w.edit(0, |n| n.receipt["source_sha"] = json!("g".repeat(40)));
        }),
    ]
}

fn staging() -> Vec<Case> {
    vec![
        made("admission-already-staged", "warm", |w| {
            w.quirk = Some(Quirk::AdmissionAlreadyStaged);
        }),
        made("runner-temp-velnor-is-a-symlink", "warm", |w| {
            w.quirk = Some(Quirk::VelnorDirectoryIsASymlink);
        }),
    ]
}
