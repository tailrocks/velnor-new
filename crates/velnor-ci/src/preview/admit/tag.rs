// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use serde_json::{Value, json};

use super::clock::{checked_time, tagger_time};
use super::decimal;
use crate::preview::authority::Client;
use crate::preview::authority::model::positive;
use crate::preview::authority::modes::Inputs;
use crate::preview::json::{Field, Res, canonical, hex64, is_hex, parse, require};
use crate::preview::login::Login;
use crate::preview::{REPOSITORY, REPOSITORY_ID, SIGNER_WORKFLOW};

const CREATION_RULESET: u64 = 26_696_396;
const IMMUTABLE_TAG_RULESET: u64 = 24_397_132;
const PRECHECK_SECONDS: i64 = 900;
const ANNOTATION_KEYS: [&str; 7] = [
    "schema",
    "repository",
    "artifact",
    "source_sha",
    "version",
    "targets",
    "immutability",
];
const PRODUCER_KEYS: [&str; 5] = [
    "producer_run_id",
    "producer_run_attempt",
    "payload_artifact_id",
    "payload_archive_sha256",
    "qualification_job_id",
];

fn keys(value: &Value) -> BTreeSet<&str> {
    value
        .as_object()
        .map_or_else(BTreeSet::new, |m| m.keys().map(String::as_str).collect())
}

/// Every entry of the ruleset's `rules`, which must be objects, by its type.
fn rule_types(record: &Value) -> Res<BTreeSet<Option<&str>>> {
    let rules = match record.get("rules") {
        None => return Ok(BTreeSet::new()),
        Some(Value::Array(rules)) => rules,
        Some(_) => return Err(crate::error::CiError::new("source preview rules")),
    };
    require(rules.iter().all(Value::is_object))?;
    Ok(rules.iter().map(|r| r.at("type").as_str()).collect())
}

/// Port of `verify_tag_rules`: the two rulesets are active, the creation of preview tags is
/// the owner's alone, and no preview tag can be changed or deleted.
pub fn verify_tag_rules(client: &dyn Client, owner_id: u64) -> Res<()> {
    let creation = client.get(&format!("/rulesets/{CREATION_RULESET}"))?;
    let frozen = client.get(&format!("/rulesets/{IMMUTABLE_TAG_RULESET}"))?;
    for record in [&creation, &frozen] {
        require(record.at("target").is_s("tag") && record.at("enforcement").is_s("active"))?;
    }
    let owner = json!([{"actor_id": owner_id, "actor_type": "User", "bypass_mode": "always"}]);
    require(
        creation.at("id").is_u(CREATION_RULESET)
            && creation.at("bypass_actors") == &owner
            && creation.at("conditions").at("ref_name")
                == &json!({"include": ["refs/tags/sdk-preview-*"], "exclude": []})
            && rule_types(&creation)? == BTreeSet::from([Some("creation")]),
    )?;
    let types = rule_types(&frozen)?;
    require(
        frozen.at("id").is_u(IMMUTABLE_TAG_RULESET)
            && frozen.at("bypass_actors") == &json!([])
            && frozen.at("conditions").at("ref_name")
                == &json!({"include": ["~ALL"], "exclude": []})
            && types.contains(&Some("update"))
            && types.contains(&Some("deletion")),
    )
}

/// Port of `annotation`: the tag message is the canonical record of the owner's
/// precheck and of the producers of both payloads, made within a minute of the tag and
/// at most 900 seconds before this run was created.
pub fn annotation(
    message: &Value,
    source: &str,
    tagger_date: &str,
    now: i64,
    version: &str,
) -> Res<Value> {
    let message = message.as_str().unwrap_or("");
    require(message.len() <= 8192)?;
    let record = parse(message.as_bytes(), usize::MAX)?;
    let text = canonical(&record);
    require(keys(&record) == BTreeSet::from(ANNOTATION_KEYS))?;
    require(message.as_bytes() == text || message.as_bytes() == [text, b"\n".to_vec()].concat())?;
    require(
        record.at("schema").is_u(2)
            && record.at("repository").is_s(REPOSITORY)
            && record.at("artifact").is_s("generator")
            && record.at("source_sha").is_s(source)
            && record.at("version").is_s(version),
    )?;
    let targets = record.at("targets");
    require(keys(targets) == BTreeSet::from(["linux", "darwin"]))?;
    let mut ids = BTreeSet::new();
    for target in targets.as_object().into_iter().flat_map(|m| m.values()) {
        require(keys(target) == BTreeSet::from(PRODUCER_KEYS))?;
        for key in [
            "producer_run_id",
            "producer_run_attempt",
            "payload_artifact_id",
            "qualification_job_id",
        ] {
            positive(target.at(key))?;
        }
        require(hex64(target.at("payload_archive_sha256")))?;
        ids.insert(positive(target.at("payload_artifact_id"))?);
    }
    require(ids.len() == 2)?;
    let precheck = record.at("immutability");
    require(
        keys(precheck) == BTreeSet::from(["enabled", "checked_at"])
            && precheck.at("enabled") == &Value::Bool(true),
    )?;
    let checked = checked_time(precheck.at("checked_at").text())?;
    let tag_time = tagger_time(tagger_date)?;
    require((0..=60).contains(&(tag_time - checked)))?;
    require((0..=PRECHECK_SECONDS).contains(&(now - checked)))?;
    Ok(record)
}

/// Port of `tag_reference`: the object id of the annotated tag `sdk-preview-<source>`.
pub fn tag_reference(client: &dyn Client, source: &str) -> Res<String> {
    let tag = format!("sdk-preview-{source}");
    let reference = client.get(&format!("/git/ref/tags/{tag}"))?;
    let target = reference.at("object");
    require(
        reference.at("ref").is_s(&format!("refs/tags/{tag}"))
            && target.at("type").is_s("tag")
            && is_hex(target.at("sha").text(), 40),
    )?;
    Ok(target.at("sha").text().to_owned())
}

/// Port of `verify_default_ancestry`: `main` is at or ahead of the source commit.
pub fn verify_default_ancestry(client: &dyn Client, source: &str) -> Res<()> {
    let main = client.get("/git/ref/heads/main")?;
    let main = main.at("object");
    require(main.at("type").is_s("commit") && is_hex(main.at("sha").text(), 40))?;
    let comparison = client.get(&format!("/compare/{source}...{}", main.at("sha").text()))?;
    require(
        matches!(
            comparison.at("status").as_str(),
            Some("ahead" | "identical")
        ) && comparison.at("merge_base_commit").at("sha").is_s(source),
    )
}

#[derive(Debug)]
pub struct Proof {
    pub record: Value,
    pub tag_sha: String,
    pub reference: String,
}

/// Port of `admit_tag`: this run is the owner's push of the annotated tag
/// `sdk-preview-<source>`, from the owner's account, on a repository whose rulesets
/// keep the tag the owner's and unchanged.
pub fn admit_tag(
    client: &dyn Client,
    inputs: &Inputs,
    owner_id: u64,
    source: &str,
    version: &str,
) -> Res<Proof> {
    let tag = format!("sdk-preview-{source}");
    let reference = format!("{REPOSITORY}/{SIGNER_WORKFLOW}@refs/tags/{tag}");
    let var = |key: &str| inputs.vars.get(key).map_or("", String::as_str);
    require(
        is_hex(source, 40)
            && var("GITHUB_REPOSITORY") == REPOSITORY
            && var("GITHUB_EVENT_NAME") == "push"
            && var("GITHUB_REF") == format!("refs/tags/{tag}")
            && var("GITHUB_WORKFLOW_REF") == reference
            && var("GITHUB_WORKFLOW_SHA") == source
            && var("GITHUB_ACTOR_ID") == owner_id.to_string(),
    )?;
    // The login is the run's triggering actor; the account it names must be the configured release owner id.
    let login = Login::parse(var("GITHUB_TRIGGERING_ACTOR"))?;
    let owner = client.api(&login.path())?;
    require(owner.at("id").is_u(owner_id) && owner.at("login").is_s(login.as_str()))?;
    let run_id = decimal(inputs, "GITHUB_RUN_ID")?;
    let attempt = decimal(inputs, "GITHUB_RUN_ATTEMPT")?;
    let run = client.get(&format!("/actions/runs/{run_id}"))?;
    let head = run.at("head_repository");
    require(
        run.at("id").is_u(run_id)
            && positive(run.at("run_attempt"))? >= attempt
            && run.at("head_sha").is_s(source)
            && run.at("head_branch").is_s(&tag)
            && run.at("event").is_s("push")
            && run.at("path").text().split('@').next() == Some(SIGNER_WORKFLOW)
            && head.at("full_name").is_s(REPOSITORY)
            && head.at("id").is_u(REPOSITORY_ID),
    )?;
    let now = checked_time(run.at("created_at").text())?;
    verify_tag_rules(client, owner_id)?;
    let tag_sha = tag_reference(client, source)?;
    let value = client.get(&format!("/git/tags/{tag_sha}"))?;
    require(
        value.at("sha").is_s(&tag_sha)
            && value.at("tag").is_s(&tag)
            && value.at("object")
                == &json!({"type": "commit", "sha": source,
                           "url": format!("https://api.github.com/repos/{REPOSITORY}/git/commits/{source}")}),
    )?;
    let record = annotation(
        value.at("message"),
        source,
        value.at("tagger").at("date").text(),
        now,
        version,
    )?;
    verify_default_ancestry(client, source)?;
    Ok(Proof {
        record,
        tag_sha,
        reference,
    })
}
