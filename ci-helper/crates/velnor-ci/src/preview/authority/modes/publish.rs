// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::{Value, json};

use super::super::files::read_pair;
use super::super::github::{existing_assets, released_pair, verify_tag};
use super::super::model::{BUNDLE, RECEIPT, TARGETS, positive, positive_text};
use super::super::pair::{Ctx, release, verify_pair};
use super::Inputs;
use crate::error::CiError;
use crate::preview::json::{Field, Res, canonical, digest, parse, require};

/// Where the signed pair is: the artifact the attest job uploaded, or the release.
pub(super) fn read_signed(
    ctx: &Ctx<'_>,
    inputs: &Inputs,
    base: &Path,
    source: &Value,
) -> Res<(Vec<u8>, Vec<u8>)> {
    match inputs.req("PAIR_ORIGIN")? {
        "artifact" => read_pair(&base.join("source-preview-signed")),
        "release" => {
            let id = positive_text(inputs.req("PAIR_ID")?)?;
            released_pair(
                ctx.client,
                &ctx.client.get(&format!("/releases/{id}"))?,
                source,
            )
        }
        _ => Err(CiError::new("source preview pair origin")),
    }
}

pub(super) fn publish(
    ctx: &Ctx<'_>,
    inputs: &Inputs,
    base: &Path,
    source: &Value,
    payloads: &BTreeMap<String, Vec<u8>>,
) -> Res<()> {
    let origin_kind = inputs.req("PAIR_ORIGIN")?;
    let (receipt_bytes, bundle) = read_signed(ctx, inputs, base, source)?;
    let receipt = parse(&receipt_bytes, usize::MAX)?;
    if origin_kind == "release" {
        require(
            digest(&receipt_bytes) == inputs.req("RECEIPT_SHA256")?
                && digest(&bundle) == inputs.req("BUNDLE_SHA256")?,
        )?;
    }
    let candidate = inputs.candidate()?;
    verify_pair(
        ctx,
        source,
        &candidate,
        positive(receipt.at("release_id"))?,
        payloads,
        &receipt_bytes,
        &bundle,
    )?;
    publish_verified(ctx, source, &receipt, &receipt_bytes, &bundle, payloads)
}

/// Port of `publish_verified`: upload what the draft lacks, check the tag, publish the
/// draft, and check the published release is immutable and holds exactly those assets.
fn publish_verified(
    ctx: &Ctx<'_>,
    source: &Value,
    receipt: &Value,
    receipt_bytes: &[u8],
    bundle: &[u8],
    payloads: &BTreeMap<String, Vec<u8>>,
) -> Res<()> {
    let client = ctx.client;
    let mut expected: BTreeMap<String, Vec<u8>> = BTreeMap::from([
        (RECEIPT.to_owned(), receipt_bytes.to_vec()),
        (BUNDLE.to_owned(), bundle.to_vec()),
    ]);
    for (key, _, name) in TARGETS {
        expected.insert(
            name.to_owned(),
            payloads.get(key).cloned().unwrap_or_default(),
        );
    }
    let id = positive(receipt.at("release_id"))?;
    let all: std::collections::BTreeSet<String> = expected.keys().cloned().collect();
    let state = client.get(&format!("/releases/{id}"))?;
    if state.at("draft") == &Value::Bool(false) {
        let published = release(ctx, receipt, false)?;
        require(
            published.at("immutable") == &Value::Bool(true)
                && existing_assets(&published, &expected, false)? == all,
        )?;
        return Ok(());
    }
    let present = existing_assets(&release(ctx, receipt, true)?, &expected, false)?;
    for (name, data) in &expected {
        if present.contains(name) {
            continue;
        }
        let item = client.upload(id, name, data)?;
        require(item.at("name").is_s(name) && item.at("size").is_u(data.len() as u64))?;
        require(
            item.at("digest").is_s(&format!("sha256:{}", digest(data)))
                && item.at("state").is_s("uploaded"),
        )?;
    }
    require(existing_assets(&release(ctx, receipt, true)?, &expected, false)? == all)?;
    verify_tag(client, source.at("source_sha").text())?;
    client.send(
        "PATCH",
        &format!("/releases/{id}"),
        &canonical(&json!({"draft": false})),
        &[200],
    )?;
    let published = release(ctx, receipt, false)?;
    require(
        published.at("immutable") == &Value::Bool(true)
            && existing_assets(&published, &expected, false)? == all,
    )
}
