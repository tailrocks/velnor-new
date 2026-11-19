// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::{Value, json};

use super::super::files::{append, create_new};
use super::super::github::{
    check_draft_assets, find_release, load_candidate, parse_marker, successful_job, verify_tag,
};
use super::super::model::{candidate_marker, positive, receipt_targets};
use super::super::pair::{Ctx, Selection, prepare_pair};
use super::{ADMIT, Inputs, pair_outputs};
use crate::error::CiError;
use crate::preview::json::{Field, Res, canonical, require};

pub(super) fn draft(
    ctx: &Ctx<'_>,
    inputs: &Inputs,
    _base: &Path,
    source: Value,
    payloads: BTreeMap<String, Vec<u8>>,
) -> Res<()> {
    let client = ctx.client;
    let sha = source.at("source_sha").text().to_owned();
    let run_id = positive(source.at("source").at("run_id"))?;
    let attempt = source.at("source").at("run_attempt").as_u64().unwrap_or(0);
    successful_job(client, &sha, run_id, attempt, ADMIT, false)?;
    let candidate = inputs.candidate()?;
    let tag = source.at("tag").text().to_owned();
    let (source, candidate, payloads, marker, item) = match find_release(client, &tag)? {
        None => {
            let marker = candidate_marker(&source, &candidate)?;
            let body = canonical(
                &json!({"tag_name": tag, "target_commitish": sha, "name": tag, "body": marker, "draft": true, "prerelease": true}),
            );
            let item = client.send("POST", "/releases", &body, &[201])?;
            (source, candidate, payloads, marker, item)
        }
        Some(item) => {
            let marker = parse_marker(item.at("body"))?;
            let (source, candidate, payloads) = load_candidate(client, &source, &marker, &ctx.run)?;
            let marker = candidate_marker(&source, &candidate)?;
            require(item.at("body").is_s(&marker))?;
            verify_tag(client, source.at("source_sha").text())?;
            (source, candidate, payloads, marker, item)
        }
    };
    let release_id = positive(item.at("id"))?;
    let draft = {
        let mut run = source.at("source").clone();
        run["run_attempt"] = json!(ctx.run.run_attempt);
        run
    };
    let decision = json!({
        "source": source, "candidate": candidate, "release_id": release_id,
        "targets": receipt_targets(&source, &candidate, &payloads)?, "draft": draft,
    });
    let current = client.get(&format!("/releases/{release_id}"))?;
    require(current.at("id").is_u(release_id) && current.at("tag_name") == source.at("tag"))?;
    require(current.at("body").is_s(&marker) && current.at("prerelease") == &Value::Bool(true))?;
    let is_draft = current
        .at("draft")
        .as_bool()
        .ok_or_else(|| CiError::new("source preview draft"))?;
    require(is_draft || current.at("immutable") == &Value::Bool(true))?;
    check_draft_assets(&current, &payloads)?;
    create_new(
        &inputs.temp()?.join("source-preview-decision.json"),
        &canonical(&decision),
    )?;
    let selected = prepare_pair(ctx, &source, &decision, &payloads)?;
    verify_tag(client, source.at("source_sha").text())?;
    let mut out: String = [
        "darwin_payload_artifact_id",
        "linux_payload_artifact_id",
        "source_artifact_id",
    ]
    .map(|key| format!("{key}={}\n", candidate.at(key)))
    .concat();
    match &selected {
        Selection::Sign => out.push_str("sign=true\n"),
        Selection::Reuse(pair) => {
            out.push_str("sign=false\n");
            out.push_str(&pair_outputs(pair, release_id));
        }
    }
    append(inputs.output()?, &out)
}
