use velnor_actions_contract::ScaleSetSelector;

use crate::{RenderError, RenderedFile, yaml::Yaml as RoutingYaml};

use super::{
    WorkflowRoutingPolicy, invalid, inventory_values, route_workflow_yaml_inner_with_paired_source,
};

use crate::schema2::QUALIFICATION_WORKFLOW;

pub(crate) const BUILD_JOB_ID: &str = "paired-sdk-build";
pub(crate) const TLS_JOB_ID: &str = "paired-sdk-tls";
pub(crate) const BUILD_CONDITION: &str =
    "inputs.mode == 'paired-sdk-build' || inputs.mode == 'paired-sdk-tls'";
pub(crate) const TLS_CONDITION: &str = "inputs.mode == 'paired-sdk-tls'";

impl WorkflowRoutingPolicy {
    pub(crate) fn paired_sdk_selector(&self) -> Option<&ScaleSetSelector> {
        self.selector()
    }
}

pub(super) fn is_tls_consumer(path: &str, job_id: &str) -> bool {
    path == QUALIFICATION_WORKFLOW && job_id == TLS_JOB_ID
}

pub(super) fn is_builder(path: &str, job_id: &str) -> bool {
    path == QUALIFICATION_WORKFLOW && job_id == BUILD_JOB_ID
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct QualificationSource {
    builder: RoutingYaml,
    consumer: RoutingYaml,
    hosted_label: String,
    selector_labels: Vec<String>,
}

pub(crate) fn bind_policy(
    policy: &WorkflowRoutingPolicy,
    routed: RoutedQualification,
    file: &RenderedFile,
) -> Result<WorkflowRoutingPolicy, RenderError> {
    let mut policy = policy.clone();
    policy.paired_sdk = Some(routed.bind(file)?);
    Ok(policy)
}

impl QualificationSource {
    pub(crate) fn from_generated_jobs(
        builder: (String, RoutingYaml),
        consumer: (String, RoutingYaml),
        hosted_label: String,
        selector: &ScaleSetSelector,
    ) -> Result<Self, RenderError> {
        if builder.0 != BUILD_JOB_ID || consumer.0 != TLS_JOB_ID {
            return Err(invalid("paired_sdk_source_job_ids".to_owned()));
        }
        validate_source_job(&builder.1, BUILD_CONDITION, Some(&hosted_label), None)?;
        validate_source_job(&consumer.1, TLS_CONDITION, None, Some(BUILD_JOB_ID))?;
        if field(&consumer.1, "runs-on") != Some(&RoutingYaml::Flow(selector.labels().to_vec())) {
            return Err(invalid("paired_sdk_source_selector".to_owned()));
        }
        Ok(Self {
            builder: builder.1,
            consumer: consumer.1,
            hosted_label,
            selector_labels: selector.labels().to_vec(),
        })
    }

    pub(super) fn validate_source(
        &self,
        path: &str,
        jobs: &[(String, RoutingYaml)],
    ) -> Result<(), RenderError> {
        if path != QUALIFICATION_WORKFLOW {
            return Err(invalid("paired_sdk_source_wrong_workflow".to_owned()));
        }
        require_one_job(jobs, BUILD_JOB_ID, &self.builder)?;
        require_one_job(jobs, TLS_JOB_ID, &self.consumer)
    }

    pub(super) fn validate_routed(
        &self,
        path: &str,
        workflow: &RoutingYaml,
        policy: &WorkflowRoutingPolicy,
    ) -> Result<(), RenderError> {
        let jobs = routing_jobs(workflow)?;
        require_one_job(&jobs, BUILD_JOB_ID, &self.builder)?;
        require_one_job(&jobs, TLS_JOB_ID, &self.consumer)?;
        if policy
            .selector()
            .map(velnor_actions_contract::ScaleSetSelector::labels)
            != Some(self.selector_labels.as_slice())
        {
            return Err(invalid("paired_sdk_routed_selector".to_owned()));
        }
        if path != QUALIFICATION_WORKFLOW {
            return Err(invalid("paired_sdk_routed_wrong_workflow".to_owned()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RoutedQualification {
    source: QualificationSource,
    workflow: RoutingYaml,
    version: String,
}

impl RoutedQualification {
    fn bind(self, file: &RenderedFile) -> Result<BoundQualification, RenderError> {
        if file.path != QUALIFICATION_WORKFLOW {
            return Err(invalid("paired_sdk_snapshot_wrong_path".to_owned()));
        }
        let expected = crate::render::fallback::render_checked_workflow(
            QUALIFICATION_WORKFLOW,
            &self.workflow,
            &self.version,
        )?;
        if file.bytes != expected {
            return Err(invalid("paired_sdk_snapshot_bytes_mismatch".to_owned()));
        }
        Ok(BoundQualification {
            bytes: file.bytes.clone(),
            hosted_label: self.source.hosted_label,
            selector_labels: self.source.selector_labels,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct BoundQualification {
    bytes: String,
    hosted_label: String,
    selector_labels: Vec<String>,
}

pub(crate) fn route_qualification(
    path: &str,
    workflow: RoutingYaml,
    version: &str,
    policy: &WorkflowRoutingPolicy,
    source: &QualificationSource,
) -> Result<(RoutingYaml, RoutedQualification), RenderError> {
    let source_jobs = routing_jobs(&workflow)?;
    source.validate_source(path, &source_jobs)?;
    let routed = route_workflow_yaml_inner_with_paired_source(
        path,
        workflow,
        policy,
        None,
        None,
        Some(source),
    )?;
    source.validate_routed(path, &routed, policy)?;
    Ok((
        routed.clone(),
        RoutedQualification {
            source: source.clone(),
            workflow: routed,
            version: version.to_owned(),
        },
    ))
}

pub(super) fn validate_snapshot(
    files: &[RenderedFile],
    policy: &WorkflowRoutingPolicy,
) -> Result<(), RenderError> {
    let Some(bound) = &policy.paired_sdk else {
        return Ok(());
    };
    let matching = files
        .iter()
        .filter(|file| file.path == QUALIFICATION_WORKFLOW)
        .collect::<Vec<_>>();
    if matching.len() != 1 || matching[0].bytes != bound.bytes {
        return Err(invalid(
            "paired_sdk_final_workflow_snapshot_mismatch".to_owned(),
        ));
    }
    validate_bound_jobs(matching[0], bound)
}

pub(super) fn validate_job_location(
    path: &str,
    job_id: &str,
    policy: &WorkflowRoutingPolicy,
) -> Result<(), RenderError> {
    if matches!(job_id, BUILD_JOB_ID | TLS_JOB_ID)
        && (path != QUALIFICATION_WORKFLOW || policy.paired_sdk.is_none())
    {
        return Err(invalid("paired_sdk_job_outside_qualification".to_owned()));
    }
    Ok(())
}

pub(super) fn validate_source_presence(
    path: &str,
    jobs: &[(String, RoutingYaml)],
    source: Option<&QualificationSource>,
) -> Result<(), RenderError> {
    if source.is_none()
        && jobs
            .iter()
            .any(|(job_id, _)| matches!(job_id.as_str(), BUILD_JOB_ID | TLS_JOB_ID))
    {
        return Err(invalid("paired_sdk_job_without_source".to_owned()));
    }
    if source.is_some() && path != QUALIFICATION_WORKFLOW {
        return Err(invalid("paired_sdk_source_wrong_workflow".to_owned()));
    }
    Ok(())
}

pub(super) fn route_builder_fields(
    source: Option<&QualificationSource>,
    path: &str,
    job_id: &str,
    fields: &mut [(String, RoutingYaml)],
    runs_on_index: Option<usize>,
) -> Result<Option<bool>, RenderError> {
    if source.is_none() || !is_builder(path, job_id) {
        return Ok(None);
    }
    let Some(index) = runs_on_index else {
        return Err(invalid(format!(
            "workflow_job_runs_on_missing:{path}:{job_id}"
        )));
    };
    super::labels::validate_hosted_runner(&fields[index].1, fields, path, job_id)?;
    Ok(Some(false))
}

pub(super) fn is_containerless_tls_consumer(
    path: &str,
    job_id: &str,
    fields: &saphyr::Mapping<'_>,
    runs_on: &saphyr::Yaml<'_>,
    container: Option<&saphyr::Yaml<'_>>,
    policy: &WorkflowRoutingPolicy,
) -> Result<bool, RenderError> {
    let Some(bound) = &policy.paired_sdk else {
        return Ok(false);
    };
    if job_id != TLS_JOB_ID {
        return Ok(false);
    }
    if path != QUALIFICATION_WORKFLOW || container.is_some() {
        return Err(invalid("paired_sdk_tls_consumer_placement".to_owned()));
    }
    if !saphyr_sequence_matches(runs_on, &bound.selector_labels)? {
        return Err(invalid("paired_sdk_tls_consumer_selector".to_owned()));
    }
    let job = saphyr::Yaml::Mapping(fields.clone());
    if inventory_values::mapping_get(&job, "if").and_then(inventory_values::scalar_text)
        != Some(TLS_CONDITION)
    {
        return Err(invalid("paired_sdk_tls_consumer_condition".to_owned()));
    }
    let needs = inventory_values::mapping_get(&job, "needs")
        .and_then(saphyr::Yaml::as_sequence)
        .ok_or_else(|| invalid("paired_sdk_tls_consumer_needs".to_owned()))?;
    if needs.len() != 1 || inventory_values::scalar_text(&needs[0]) != Some(BUILD_JOB_ID) {
        return Err(invalid("paired_sdk_tls_consumer_needs".to_owned()));
    }
    Ok(true)
}

fn validate_bound_jobs(file: &RenderedFile, bound: &BoundQualification) -> Result<(), RenderError> {
    let document = super::parse::parse_final_workflow(file)?;
    let jobs = inventory_values::mapping_get(&document, "jobs")
        .and_then(saphyr::Yaml::as_mapping)
        .ok_or_else(|| invalid("paired_sdk_snapshot_jobs_missing".to_owned()))?;
    validate_parsed_job(jobs, BUILD_JOB_ID)?;
    validate_parsed_job(jobs, TLS_JOB_ID)?;
    let builder = find_parsed_job(jobs, BUILD_JOB_ID)?;
    if inventory_values::mapping_get(builder, "runs-on").and_then(inventory_values::scalar_text)
        != Some(bound.hosted_label.as_str())
        || inventory_values::mapping_get(builder, "container").is_some()
    {
        return Err(invalid("paired_sdk_builder_placement".to_owned()));
    }
    let consumer = find_parsed_job(jobs, TLS_JOB_ID)?;
    if inventory_values::mapping_get(consumer, "container").is_some() {
        return Err(invalid("paired_sdk_tls_consumer_container".to_owned()));
    }
    Ok(())
}

fn validate_parsed_job(jobs: &saphyr::Mapping<'_>, id: &str) -> Result<(), RenderError> {
    let count = jobs
        .iter()
        .filter(|(key, _)| inventory_values::scalar_text(key) == Some(id))
        .count();
    if count != 1 {
        return Err(invalid(format!("paired_sdk_job_count:{id}")));
    }
    Ok(())
}

fn find_parsed_job<'a, 'yaml>(
    jobs: &'a saphyr::Mapping<'yaml>,
    id: &str,
) -> Result<&'a saphyr::Yaml<'yaml>, RenderError> {
    jobs.iter()
        .find_map(|(key, value)| (inventory_values::scalar_text(key) == Some(id)).then_some(value))
        .ok_or_else(|| invalid(format!("paired_sdk_job_missing:{id}")))
}

fn validate_source_job(
    job: &RoutingYaml,
    condition: &str,
    runner: Option<&str>,
    needs: Option<&str>,
) -> Result<(), RenderError> {
    if field(job, "if") != Some(&RoutingYaml::str(condition)) || field(job, "container").is_some() {
        return Err(invalid("paired_sdk_source_job_contract".to_owned()));
    }
    if runner.is_some_and(|runner| field(job, "runs-on") != Some(&RoutingYaml::str(runner))) {
        return Err(invalid("paired_sdk_source_runner".to_owned()));
    }
    if let Some(needed) = needs
        && field(job, "needs") != Some(&RoutingYaml::Seq(vec![RoutingYaml::str(needed)]))
    {
        return Err(invalid("paired_sdk_source_needs".to_owned()));
    }
    Ok(())
}

fn field<'a>(job: &'a RoutingYaml, name: &str) -> Option<&'a RoutingYaml> {
    let RoutingYaml::Map(fields) = job else {
        return None;
    };
    fields
        .iter()
        .find_map(|(key, value)| (key == name).then_some(value))
}

fn routing_jobs(workflow: &RoutingYaml) -> Result<Vec<(String, RoutingYaml)>, RenderError> {
    let RoutingYaml::Map(_) = workflow else {
        return Err(invalid("paired_sdk_workflow_not_map".to_owned()));
    };
    let Some(RoutingYaml::Map(jobs)) = field(workflow, "jobs") else {
        return Err(invalid("paired_sdk_workflow_jobs_missing".to_owned()));
    };
    Ok(jobs.clone())
}

fn require_one_job(
    jobs: &[(String, RoutingYaml)],
    id: &str,
    expected: &RoutingYaml,
) -> Result<(), RenderError> {
    let matches = jobs
        .iter()
        .filter(|(job_id, _)| job_id == id)
        .collect::<Vec<_>>();
    if matches.len() != 1 || matches[0].1 != *expected {
        return Err(invalid(format!("paired_sdk_source_job_mismatch:{id}")));
    }
    Ok(())
}

fn saphyr_sequence_matches(
    value: &saphyr::Yaml<'_>,
    expected: &[String],
) -> Result<bool, RenderError> {
    let sequence = value
        .as_sequence()
        .ok_or_else(|| invalid("paired_sdk_tls_consumer_runs_on".to_owned()))?;
    if sequence.len() != expected.len() {
        return Ok(false);
    }
    for (item, expected) in sequence.iter().zip(expected) {
        let actual = inventory_values::scalar_text(item)
            .ok_or_else(|| invalid("paired_sdk_tls_consumer_runs_on".to_owned()))?;
        if actual != expected {
            return Ok(false);
        }
    }
    Ok(true)
}

#[cfg(test)]
#[path = "workflow_routing_paired_sdk_tests.rs"]
mod tests;
