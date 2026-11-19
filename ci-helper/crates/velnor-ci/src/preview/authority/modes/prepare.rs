// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::Value;

use super::super::files::{append, create_new, read_file};
use super::super::github::{successful_job, verify_tag};
use super::super::model::{
    BUNDLE, RECEIPT, finalize_receipt, positive, positive_text, receipt_targets,
};
use super::super::pair::{Ctx, Selection, verify_pair};
use super::publish::read_signed;
use super::{ADMIT, DRAFT, Inputs, pair_outputs};
use crate::error::CiError;
use crate::preview::json::{Field, Res, canonical, digest, parse, require};

/// The decision file the draft step left, checked against the source, the payloads and the
/// environment, and the draft and admit jobs it names. Returns the decision and the candidate.
fn read_decision(
    ctx: &Ctx<'_>,
    inputs: &Inputs,
    base: &Path,
    source: &Value,
    payloads: &BTreeMap<String, Vec<u8>>,
) -> Res<(Value, Value)> {
    let bytes = read_file(
        &base.join("source-preview-decision"),
        "source-preview-decision.json",
        65_536,
    )?;
    let decision = parse(&bytes, usize::MAX)?;
    require(canonical(&decision) == bytes)?;
    let origin = source.at("source");
    let key_set = |v: &Value| {
        v.as_object()
            .map(|m| m.keys().cloned().collect::<std::collections::BTreeSet<_>>())
    };
    let wanted = ["source", "candidate", "release_id", "targets", "draft"];
    require(key_set(&decision) == Some(wanted.iter().map(|k| (*k).to_owned()).collect()))?;
    require(decision.at("source") == source)?;
    let draft = decision.at("draft");
    require(key_set(draft).is_some() && key_set(draft) == key_set(origin))?;
    for key in ["workflow_path", "workflow_ref", "workflow_sha", "run_id"] {
        require(draft.at(key) == origin.at(key))?;
    }
    let draft_attempt = positive(draft.at("run_attempt"))?;
    require(
        origin
            .at("run_attempt")
            .as_u64()
            .is_some_and(|a| a <= draft_attempt)
            && draft_attempt <= ctx.run.run_attempt
            && ctx.run.run_attempt <= 51,
    )?;
    let candidate = inputs.candidate()?;
    require(
        decision.at("candidate") == &candidate
            && decision.at("targets") == &receipt_targets(source, &candidate, payloads)?,
    )?;
    let (sha, run_id) = (
        source.at("source_sha").text(),
        positive(origin.at("run_id"))?,
    );
    let first = origin.at("run_attempt").as_u64().unwrap_or(0);
    successful_job(ctx.client, sha, run_id, first, ADMIT, false)?;
    successful_job(ctx.client, sha, run_id, draft_attempt, DRAFT, false)?;
    Ok((decision, candidate))
}

pub(super) fn prepare(
    ctx: &Ctx<'_>,
    inputs: &Inputs,
    base: &Path,
    source: &Value,
    payloads: &BTreeMap<String, Vec<u8>>,
) -> Res<()> {
    let client = ctx.client;
    let (decision, candidate) = read_decision(ctx, inputs, base, source, payloads)?;
    let sha = source.at("source_sha").text();
    let release_id = positive(decision.at("release_id"))?;
    let selected = match inputs.req("SIGN")? {
        "true" => Selection::Sign,
        "false" => {
            let pair_id = positive_text(inputs.req("PAIR_ID")?)?;
            let (receipt, bundle) = read_signed(ctx, inputs, base, source)?;
            require(
                digest(&receipt) == inputs.req("RECEIPT_SHA256")?
                    && digest(&bundle) == inputs.req("BUNDLE_SHA256")?,
            )?;
            let mut pair = verify_pair(
                ctx, source, &candidate, release_id, payloads, &receipt, &bundle,
            )?;
            pair.origin = if inputs.req("PAIR_ORIGIN")? == "artifact" {
                "artifact"
            } else {
                "release"
            };
            pair.artifact_id = Some(pair_id);
            Selection::Reuse(pair)
        }
        _ => return Err(CiError::new("source preview sign")),
    };
    verify_tag(client, sha)?;
    let receipt = match &selected {
        Selection::Sign => finalize_receipt(&decision, ctx.run.run_attempt)?,
        Selection::Reuse(pair) => pair.receipt.clone(),
    };
    create_new(&inputs.temp()?.join(RECEIPT), &receipt)?;
    let mut out = String::new();
    if let Selection::Reuse(pair) = &selected {
        create_new(&inputs.temp()?.join(BUNDLE), &pair.bundle)?;
        out.push_str("sign=false\n");
        out.push_str(&pair_outputs(pair, release_id));
    } else {
        out.push_str("sign=true\n");
    }
    append(inputs.output()?, &out)
}
