// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::json;

use crate::resolve_cases::{Case, made};
use crate::resolve_world::{Node, Quirk, run_route};

fn receipt_case(name: &'static str, phase: &'static str, change: fn(&mut Node)) -> Case {
    made(name, phase, move |w| change(&mut w.nodes[0]))
}

pub(crate) fn receipts() -> Vec<Case> {
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

pub(crate) fn lineage() -> Vec<Case> {
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

pub(crate) fn deltas() -> Vec<Case> {
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

pub(crate) fn staging() -> Vec<Case> {
    vec![
        made("admission-already-staged", "warm", |w| {
            w.quirk = Some(Quirk::AdmissionAlreadyStaged);
        }),
        made("runner-temp-velnor-is-a-symlink", "warm", |w| {
            w.quirk = Some(Quirk::VelnorDirectoryIsASymlink);
        }),
    ]
}
