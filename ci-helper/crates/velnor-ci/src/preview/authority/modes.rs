// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::{Value, json};

use super::files::{append, create_new, read_file, read_pair};
use super::github::{
    check_draft_assets, existing_assets, find_release, load_candidate, parse_marker, released_pair,
    successful_job, verify_tag,
};
use super::model::{
    BUNDLE, RECEIPT, Run, TARGETS, candidate_marker, finalize_receipt, metadata, payload_identity,
    positive, positive_text, receipt_targets,
};
use super::pair::{Ctx, Pair, Selection, prepare_pair, release, verify_pair};
use crate::error::CiError;
use crate::preview::MAX_PAYLOAD;
use crate::preview::attest::{Attestor, Authority};
use crate::preview::json::{Field, Res, canonical, digest, parse, require};

const ADMIT: &str = "Source preview publication / Admit source preview";
const DRAFT: &str = "Source preview publication / Draft source preview";

/// Which step of the publication workflow runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Draft,
    Prepare,
    Publish,
}

impl Mode {
    pub fn parse(text: &str) -> Res<Self> {
        match text {
            "draft" => Ok(Self::Draft),
            "prepare" => Ok(Self::Prepare),
            "publish" => Ok(Self::Publish),
            _ => Err(CiError::new("source preview mode")),
        }
    }
}

/// The step's environment, as the workflow passes it.
#[derive(Debug)]
pub struct Inputs {
    pub vars: BTreeMap<String, String>,
}

impl Inputs {
    pub fn req(&self, key: &str) -> Res<&str> {
        self.vars
            .get(key)
            .map(String::as_str)
            .ok_or_else(|| CiError::new("source preview environment"))
    }

    pub fn run(&self) -> Res<Run<'_>> {
        Ok(Run {
            sha: self.req("GITHUB_SHA")?,
            repository: self
                .vars
                .get("GITHUB_REPOSITORY")
                .map_or("", String::as_str),
            event_name: self
                .vars
                .get("GITHUB_EVENT_NAME")
                .map_or("", String::as_str),
            git_ref: self.vars.get("GITHUB_REF").map_or("", String::as_str),
            workflow_ref: self
                .vars
                .get("GITHUB_WORKFLOW_REF")
                .map_or("", String::as_str),
            workflow_sha: self
                .vars
                .get("GITHUB_WORKFLOW_SHA")
                .map_or("", String::as_str),
            run_id: positive_text(self.req("GITHUB_RUN_ID")?)?,
            run_attempt: positive_text(self.req("GITHUB_RUN_ATTEMPT")?)?,
        })
    }

    pub fn authority(&self) -> Res<Authority> {
        Authority::from_env(
            self.req("AUTHORITY_REPOSITORY")?,
            self.req("AUTHORITY_PATH")?,
            self.req("AUTHORITY_REF")?,
            self.req("AUTHORITY_SHA")?,
        )
    }

    pub fn candidate(&self) -> Res<Value> {
        Ok(json!({
            "source_artifact_id": positive_text(self.req("SOURCE_ARTIFACT_ID")?)?,
            "linux_payload_artifact_id": positive_text(self.req("LINUX_PAYLOAD_ARTIFACT_ID")?)?,
            "darwin_payload_artifact_id": positive_text(self.req("DARWIN_PAYLOAD_ARTIFACT_ID")?)?,
        }))
    }

    fn temp(&self) -> Res<&Path> {
        Ok(Path::new(self.req("RUNNER_TEMP")?))
    }

    fn output(&self) -> Res<&Path> {
        Ok(Path::new(self.req("GITHUB_OUTPUT")?))
    }
}

fn pair_outputs(selected: &Pair, release_id: u64) -> String {
    format!(
        "pair_origin={}\npair_id={}\nreceipt_sha256={}\nbundle_sha256={}\n",
        selected.origin,
        selected.artifact_id.unwrap_or(release_id),
        digest(&selected.receipt),
        digest(&selected.bundle)
    )
}

/// Port of the `main` of `schema2_generator_source_preview_publication.py`.
/// `base` is the directory the artifacts were downloaded under (the workspace).
pub fn run(
    mode: Mode,
    client: &dyn super::Client,
    attestor: &dyn Attestor,
    inputs: &Inputs,
    base: &Path,
) -> Res<()> {
    let authority = inputs.authority()?;
    let run = inputs.run()?;
    let raw = read_file(
        &base.join("source-preview-metadata"),
        "source-preview-metadata.json",
        65_536,
    )?;
    let source = metadata(&raw, &run)?;
    let mut payloads = BTreeMap::new();
    for (key, _, name) in TARGETS {
        let data = read_file(
            &base.join(format!("source-preview-payload-{key}")),
            name,
            MAX_PAYLOAD as u64,
        )?;
        payloads.insert(key.to_owned(), data);
    }
    for (key, data) in &payloads {
        payload_identity(key, data)?;
    }
    verify_tag(client, source.at("source_sha").text())?;
    let ctx = Ctx {
        client,
        attestor,
        run: run.clone(),
        authority: &authority,
    };
    match mode {
        Mode::Draft => draft(&ctx, inputs, base, source, payloads),
        Mode::Prepare => prepare(&ctx, inputs, base, &source, &payloads),
        Mode::Publish => publish(&ctx, inputs, base, &source, &payloads),
    }
}

fn draft(
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

/// Where the signed pair is: the artifact the attest job uploaded, or the release.
fn read_signed(
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

fn prepare(
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

fn publish(
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
