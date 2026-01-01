//! Optional metadata from finalized report-producer jobs.

use std::collections::BTreeSet;
use std::path::Path;

use super::artifact_build_context::read_artifact_build_context;
use velnor_actions_contract::ids::job_ids::validate_job_id;
use velnor_actions_contract::parse_strict_json;
use velnor_actions_contract_workflow::{
    ARTIFACT_BUILD_OBSERVATIONS_FILENAME, ArtifactBuildRunContext, Plan,
    TASK_REPORT_PRODUCERS_EXPECTED_ENV, canonical_plan_digest,
};

/// Whether the renderer emitted its typed report-producer inventory.
pub(super) enum ExpectedProducerChannel {
    /// Legacy workflow without paired report producers.
    Absent,
    /// Environment value could not be decoded as UTF-8.
    InvalidEncoding,
    /// Static JSON array emitted from the finalized workflow graph.
    Present(String),
}

impl ExpectedProducerChannel {
    pub(super) fn from_environment() -> Self {
        match std::env::var_os(TASK_REPORT_PRODUCERS_EXPECTED_ENV) {
            None => Self::Absent,
            Some(value) => value
                .to_str()
                .map_or(Self::InvalidEncoding, |text| Self::Present(text.to_owned())),
        }
    }

    pub(super) const fn requires_context(&self) -> bool {
        matches!(self, Self::Present(_))
    }
}

/// Producer metadata inputs collected while the request is assembled.
pub(super) struct AssemblyChannels<'a> {
    pub(super) producer_expected: ExpectedProducerChannel,
    pub(super) runtime_identity_override: Option<(&'a ArtifactBuildRunContext, &'a str)>,
}

impl AssemblyChannels<'_> {
    pub(super) fn from_environment() -> Self {
        Self {
            producer_expected: ExpectedProducerChannel::from_environment(),
            runtime_identity_override: None,
        }
    }

    pub(super) const fn legacy() -> Self {
        Self {
            producer_expected: ExpectedProducerChannel::Absent,
            runtime_identity_override: None,
        }
    }
}

/// Inputs used to build one fan-in sidecar.
pub(super) struct FanInInput<'a> {
    pub(super) channel: ExpectedProducerChannel,
    pub(super) needs: Option<&'a str>,
    pub(super) required_job_ids: &'a [String],
    pub(super) plan_value: &'a serde_json::Value,
    pub(super) run_key: &'a str,
    pub(super) run_context: Option<&'a ArtifactBuildRunContext>,
    pub(super) server_url: Option<&'a str>,
}

/// Read the optional producer artifact inventory used by existing artifact builds.
pub(super) fn artifact_build_observations(
    required: bool,
    run_dir: &Path,
    errors: &mut Vec<String>,
) -> Vec<serde_json::Value> {
    if !required {
        return Vec::new();
    }
    let value = super::read_json(
        run_dir,
        ARTIFACT_BUILD_OBSERVATIONS_FILENAME,
        "artifact_build_observations",
        true,
        errors,
    );
    if let serde_json::Value::Array(items) = value {
        items
    } else {
        errors.push("artifact_build_observations_not_array".to_owned());
        Vec::new()
    }
}

/// Reuse the existing checked run identity for artifacts or report outputs.
pub(super) fn runtime_identity(
    artifact_build_required: bool,
    channel: &ExpectedProducerChannel,
    plan: &serde_json::Value,
    run_key: &str,
    override_identity: Option<(&ArtifactBuildRunContext, &str)>,
    errors: &mut Vec<String>,
) -> (Option<ArtifactBuildRunContext>, Option<String>) {
    if !artifact_build_required && !channel.requires_context() {
        return (None, None);
    }
    if let Some((context, server_url)) = override_identity {
        return (Some(context.clone()), Some(server_url.to_owned()));
    }
    let context = read_artifact_build_context(plan, run_key, errors);
    let server_url = channel
        .requires_context()
        .then(|| std::env::var("GITHUB_SERVER_URL").ok())
        .flatten();
    (context, server_url)
}

/// Build a verdict-neutral sidecar from the authoritative `needs` channel.
pub(super) fn build(input: FanInInput<'_>, errors: &mut Vec<String>) -> Option<serde_json::Value> {
    let FanInInput {
        channel,
        needs,
        required_job_ids,
        plan_value,
        run_key,
        run_context,
        server_url,
    } = input;
    let expected = parse_expected(channel, required_job_ids, errors)?;
    let needs = parse_actual_needs(needs, errors)?;
    if !contains_expected_jobs(&needs, &expected, errors) {
        return None;
    }
    let outputs = parse_successful_outputs(&needs, &expected, errors)?;
    let context = run_context?;
    if server_url != Some("https://github.com") {
        errors.push("unsupported_task_report_output_origin".to_owned());
        return None;
    }
    if !context_matches_run(context, run_key) {
        errors.push("invalid_task_report_output_context".to_owned());
        return None;
    }
    let plan = parse_plan(plan_value, run_key, errors)?;
    let Ok(digest) = canonical_plan_digest(&plan) else {
        errors.push("invalid_task_report_output_plan_digest".to_owned());
        return None;
    };
    let value = serde_json::json!({
        "schema": 1,
        "origin": "github_com",
        "run": context,
        "head_sha": plan.head,
        "plan_digest": digest,
        "expected_workflow_job_keys": expected,
        "producers": outputs,
    });
    Some(value)
}

fn parse_expected(
    channel: ExpectedProducerChannel,
    required_job_ids: &[String],
    errors: &mut Vec<String>,
) -> Option<Vec<String>> {
    let text = match channel {
        ExpectedProducerChannel::Absent => return None,
        ExpectedProducerChannel::InvalidEncoding => {
            errors.push("invalid_task_report_producers_expected_encoding".to_owned());
            return None;
        }
        ExpectedProducerChannel::Present(text) => text,
    };
    let Ok(keys) = serde_json::from_str::<Vec<String>>(&text) else {
        errors.push("invalid_task_report_producers_expected".to_owned());
        return None;
    };
    let required: BTreeSet<_> = required_job_ids.iter().map(String::as_str).collect();
    if keys.is_empty()
        || keys.windows(2).any(|pair| pair[0] >= pair[1])
        || keys
            .iter()
            .any(|key| validate_job_id(key).is_err() || !required.contains(key.as_str()))
    {
        errors.push("invalid_task_report_producers_expected".to_owned());
        return None;
    }
    Some(keys)
}

fn parse_actual_needs(
    needs: Option<&str>,
    errors: &mut Vec<String>,
) -> Option<serde_json::Map<String, serde_json::Value>> {
    let Some(needs) = needs.filter(|text| !text.trim().is_empty()) else {
        errors.push("missing_task_report_producer_needs".to_owned());
        return None;
    };
    let map = parse_strict_json(needs)
        .ok()
        .and_then(|value| value.as_object().cloned());
    if let Some(map) = map {
        Some(map)
    } else {
        errors.push("invalid_task_report_producer_needs".to_owned());
        None
    }
}

fn contains_expected_jobs(
    needs: &serde_json::Map<String, serde_json::Value>,
    expected: &[String],
    errors: &mut Vec<String>,
) -> bool {
    let missing: Vec<_> = expected
        .iter()
        .filter(|key| !needs.contains_key(*key))
        .collect();
    if missing.is_empty() {
        true
    } else {
        errors.extend(
            missing
                .into_iter()
                .map(|key| format!("missing_task_report_producer:{key}")),
        );
        false
    }
}

fn parse_successful_outputs(
    needs: &serde_json::Map<String, serde_json::Value>,
    expected: &[String],
    errors: &mut Vec<String>,
) -> Option<Vec<serde_json::Value>> {
    for key in expected {
        match producer_result(needs.get(key)) {
            Some("success") => {}
            Some("failure" | "cancelled" | "skipped") => return None,
            _ => {
                errors.push(format!("invalid_task_report_producer_result:{key}"));
                return None;
            }
        }
    }
    let mut outputs = Vec::with_capacity(expected.len());
    for key in expected {
        let output = output_for(needs.get(key), key, errors)?;
        outputs.push(output);
    }
    Some(outputs)
}

fn producer_result(entry: Option<&serde_json::Value>) -> Option<&str> {
    let value = entry?;
    value
        .as_str()
        .or_else(|| value.get("result").and_then(serde_json::Value::as_str))
}

fn output_for(
    entry: Option<&serde_json::Value>,
    key: &str,
    errors: &mut Vec<String>,
) -> Option<serde_json::Value> {
    let Some(entry) = entry else {
        errors.push(format!("missing_task_report_producer:{key}"));
        return None;
    };
    let Some(outputs) = entry.get("outputs").and_then(serde_json::Value::as_object) else {
        errors.push(format!("missing_task_report_outputs:{key}"));
        return None;
    };
    let artifact_id = outputs
        .get("task_report_artifact_id")
        .and_then(canonical_positive_id);
    let check_run_id = outputs
        .get("task_report_check_run_id")
        .and_then(canonical_positive_id);
    let (Some(artifact_id), Some(check_run_id)) = (artifact_id, check_run_id) else {
        errors.push(format!("invalid_task_report_output_id:{key}"));
        return None;
    };
    Some(serde_json::json!({
        "workflow_job_key": key,
        "conclusion": "success",
        "artifact_id": artifact_id,
        "check_run_id": check_run_id,
    }))
}

fn canonical_positive_id(value: &serde_json::Value) -> Option<i64> {
    let text = value.as_str()?;
    let id = text.parse::<i64>().ok()?;
    (id > 0 && id.to_string() == text).then_some(id)
}

fn parse_plan(value: &serde_json::Value, run_key: &str, errors: &mut Vec<String>) -> Option<Plan> {
    let Ok(plan) = serde_json::from_value::<Plan>(value.clone()) else {
        errors.push("invalid_task_report_output_plan".to_owned());
        return None;
    };
    if plan.schema != Plan::SCHEMA || plan.run_key != run_key || !is_commit_sha(&plan.head) {
        errors.push("invalid_task_report_output_plan".to_owned());
        return None;
    }
    Some(plan)
}

fn is_commit_sha(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn context_matches_run(context: &ArtifactBuildRunContext, run_key: &str) -> bool {
    let valid_id = |value: &str| {
        value
            .parse::<u64>()
            .is_ok_and(|id| id > 0 && id.to_string() == value)
    };
    valid_id(&context.repository_id)
        && valid_id(&context.run_id)
        && context.run_attempt > 0
        && run_key == format!("r{}-a{}", context.run_id, context.run_attempt)
        && velnor_actions_orchestrator_core::origin::validate_repository_slug(&context.repository)
            .is_some()
}
