//! Read terminal producer outputs from the same exact needs channel as conclusions.

use velnor_actions_contract::{Plan, ProducerRole, parse_strict_json};

use super::required_evidence::producer_roles::ProducerReport;

/// Only declared, executed producers supply terminal observations.
pub(crate) fn read(
    plan: &serde_json::Value,
    needs: Option<&str>,
    errors: &mut Vec<String>,
) -> Vec<ProducerReport> {
    let Ok(plan) = serde_json::from_value::<Plan>(plan.clone()) else {
        return Vec::new();
    };
    if plan.producers.entries.is_empty() {
        return Vec::new();
    }
    let Some(needs) = needs.and_then(|text| parse_strict_json(text).ok()) else {
        errors.push("missing_producer_needs".to_owned());
        return Vec::new();
    };
    let mut reports = Vec::new();
    for admission in &plan.producers.entries {
        let Some(job) = needs.get(&admission.job_id) else {
            continue;
        };
        let conclusion = job
            .as_str()
            .or_else(|| job.get("result").and_then(serde_json::Value::as_str));
        if !matches!(conclusion, Some("success" | "failure")) {
            continue;
        }
        let identity_output = match &admission.role {
            ProducerRole::Source { .. } => "sourceidentity",
            ProducerRole::Tool { .. } => "descriptoridentity",
            ProducerRole::Mbx { .. } => "sourceidentity",
        };
        match parse_report(&admission.job_id, job, identity_output) {
            Some(report) => reports.push(report),
            None => errors.push(format!("missing_producer_report:{}", admission.job_id)),
        }
    }
    reports
}

fn parse_report(
    job_id: &str,
    job: &serde_json::Value,
    identity_output: &str,
) -> Option<ProducerReport> {
    let outputs = job.get("outputs")?;
    let string = |key: &str| outputs.get(key)?.as_str();
    let boolean = |key: &str| match string(key)? {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    };
    let error =
        serde_json::from_value(serde_json::Value::String(string("error")?.to_owned())).ok()?;
    Some(ProducerReport {
        job_id: job_id.to_owned(),
        identity: string(identity_output)?.to_owned(),
        verified: boolean("verified")?,
        cache_available: boolean("cache_available")?,
        error,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_outputs_require_exact_boolean_and_closed_error() {
        let job = serde_json::json!({"outputs": {
            "sourceidentity": "source-key", "verified": "false",
            "cache_available": "false", "error": "PREPARATION_FAILED"
        }});
        assert!(parse_report("producer", &job, "sourceidentity").is_some());
        for (key, value) in [
            ("verified", "TRUE"),
            ("cache_available", "1"),
            ("error", "OTHER"),
            ("error", "CACHE_TRANSPORT_FAILURE"),
        ] {
            let mut changed = job.clone();
            changed["outputs"][key] = serde_json::json!(value);
            assert!(parse_report("producer", &changed, "sourceidentity").is_none());
        }
        assert!(parse_report("producer", &job, "descriptoridentity").is_none());
        for error in [
            "NONE",
            "PREPARATION_FAILED",
            "CACHE_NOT_PUBLISHED",
            "CACHE_TRANSPORT_FAILED",
            "CACHE_TRANSPORT_UNAVAILABLE",
            "PRIVATE_OR_AUTH_REQUIRED",
            "PUBLIC_AUTHORITY_UNAVAILABLE",
            "UNSUPPORTED_REGISTRY",
            "SOURCE_VERIFICATION_FAILED",
        ] {
            let mut changed = job.clone();
            changed["outputs"]["error"] = serde_json::json!(error);
            assert!(parse_report("producer", &changed, "sourceidentity").is_some());
        }
    }
}
