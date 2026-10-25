//! Complete local reusable-workflow graph metrics for explicit test captures.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use saphyr::Yaml;

use crate::RenderedFile;

/// Deterministic summary of all local reusable-workflow calls in a rendered tree.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct WorkflowCallGraphSummary {
    /// All generated workflow document paths, sorted.
    pub workflow_paths: Vec<String>,
    /// Number of generated workflow documents included in the graph.
    pub workflow_count: usize,
    /// Every local reusable-workflow call, sorted by caller, job, then callee.
    pub edges: Vec<WorkflowCallEdge>,
    /// Number of local reusable-workflow call edges.
    pub local_call_edge_count: usize,
    /// Distinct local reusable-workflow targets, sorted.
    pub distinct_local_callees: Vec<String>,
    /// Number of distinct local reusable-workflow targets.
    pub local_distinct_callee_count: usize,
    /// Maximum number of call edges on any path through the complete graph.
    pub max_local_call_depth: usize,
    /// Maximum local workflow count on a path, including its root workflow.
    pub max_local_workflow_levels: usize,
    /// Job-level reusable-workflow calls whose targets are not local files.
    pub external_reusable_calls: Vec<ExternalWorkflowCall>,
}

/// One job-level local reusable-workflow call.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct WorkflowCallEdge {
    /// Workflow file containing the caller job.
    pub caller_path: String,
    /// Calling job id.
    pub job_id: String,
    /// Resolved local target workflow file.
    pub callee_path: String,
}

/// One job-level reusable-workflow call that targets an external reference.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct ExternalWorkflowCall {
    /// Workflow file containing the caller job.
    pub caller_path: String,
    /// Calling job id.
    pub job_id: String,
    /// External `uses` reference, preserved exactly.
    pub uses: String,
}

/// Summarize every workflow file and local call in the supplied final tree.
///
/// # Errors
///
/// Returns an error for malformed workflow data, unresolved local calls,
/// targets without `workflow_call`, or cycles.
pub(super) fn summarize(files: &[RenderedFile]) -> Result<WorkflowCallGraphSummary, String> {
    let mut workflow_paths = BTreeSet::new();
    let mut supports_calls = BTreeMap::new();
    let mut edges = Vec::new();
    let mut external_reusable_calls = Vec::new();

    for file in files.iter().filter(|file| is_workflow_path(&file.path)) {
        if !workflow_paths.insert(file.path.clone()) {
            return Err(format!("duplicate_workflow_path:{}", file.path));
        }
        let document = crate::workflow_routing::parse_authority_document(file)
            .map_err(|error| format!("workflow_parse:{}:{error}", file.path))?;
        let workflow_call = supports_workflow_call(&document, &file.path)?;
        supports_calls.insert(file.path.clone(), workflow_call);
        let (workflow_edges, external_calls) = workflow_calls(&file.path, &document)?;
        edges.extend(workflow_edges);
        external_reusable_calls.extend(external_calls);
    }

    if workflow_paths.is_empty() {
        return Err("workflow_graph_has_no_workflows".to_owned());
    }
    edges.sort_by(|left, right| {
        (&left.caller_path, &left.job_id, &left.callee_path).cmp(&(
            &right.caller_path,
            &right.job_id,
            &right.callee_path,
        ))
    });
    external_reusable_calls.sort_by(|left, right| {
        (&left.caller_path, &left.job_id, &left.uses).cmp(&(
            &right.caller_path,
            &right.job_id,
            &right.uses,
        ))
    });

    let mut callees = BTreeSet::new();
    for edge in &edges {
        match supports_calls.get(&edge.callee_path) {
            Some(true) => (),
            Some(false) => {
                return Err(format!(
                    "local_reusable_workflow_call_missing:{}:{}:{}",
                    edge.caller_path, edge.job_id, edge.callee_path
                ));
            }
            None => {
                return Err(format!(
                    "unresolved_local_reusable_workflow:{}:{}",
                    edge.caller_path, edge.callee_path
                ));
            }
        }
        callees.insert(edge.callee_path.clone());
    }

    let workflow_paths: Vec<_> = workflow_paths.into_iter().collect();
    let workflow_count = workflow_paths.len();
    let local_call_edge_count = edges.len();
    let local_distinct_callee_count = callees.len();
    let max_local_call_depth = graph_depth(&workflow_paths, &edges)?;
    Ok(WorkflowCallGraphSummary {
        workflow_paths,
        workflow_count,
        edges,
        local_call_edge_count,
        distinct_local_callees: callees.into_iter().collect(),
        local_distinct_callee_count,
        max_local_call_depth,
        max_local_workflow_levels: max_local_call_depth + 1,
        external_reusable_calls,
    })
}

fn workflow_calls(
    path: &str,
    document: &Yaml<'_>,
) -> Result<(Vec<WorkflowCallEdge>, Vec<ExternalWorkflowCall>), String> {
    let jobs = field(document, "jobs", path)?
        .and_then(|value| untagged(value).as_mapping())
        .ok_or_else(|| format!("workflow_jobs_missing:{path}"))?;
    let mut ids = BTreeSet::new();
    let mut edges = Vec::new();
    let mut external_calls = Vec::new();

    for (id, job) in jobs {
        let job_id = scalar_text(id).ok_or_else(|| format!("workflow_job_id_not_scalar:{path}"))?;
        if !ids.insert(job_id.to_owned()) {
            return Err(format!("duplicate_workflow_job:{path}:{job_id}"));
        }
        let fields = untagged(job)
            .as_mapping()
            .ok_or_else(|| format!("workflow_job_not_map:{path}:{job_id}"))?;
        if fields.keys().any(|key| scalar_text(key).is_none()) {
            return Err(format!("workflow_job_field_not_scalar:{path}:{job_id}"));
        }
        let Some(uses) = field(job, "uses", path)? else {
            continue;
        };
        let uses = scalar_text(uses)
            .ok_or_else(|| format!("workflow_job_uses_not_scalar:{path}:{job_id}"))?;
        if let Some(target) = local_workflow_target(uses, path, job_id)? {
            edges.push(WorkflowCallEdge {
                caller_path: path.to_owned(),
                job_id: job_id.to_owned(),
                callee_path: target,
            });
        } else {
            external_calls.push(ExternalWorkflowCall {
                caller_path: path.to_owned(),
                job_id: job_id.to_owned(),
                uses: uses.to_owned(),
            });
        }
    }
    Ok((edges, external_calls))
}

fn local_workflow_target(
    uses: &str,
    caller_path: &str,
    job_id: &str,
) -> Result<Option<String>, String> {
    let Some(target) = uses.strip_prefix("./.github/workflows/") else {
        return Ok(None);
    };
    let target = format!(".github/workflows/{target}");
    if target.contains("..") || target.contains('@') || !is_workflow_path(&target) {
        return Err(format!(
            "invalid_local_reusable_workflow:{caller_path}:{job_id}:{uses}"
        ));
    }
    Ok(Some(target))
}

fn supports_workflow_call(document: &Yaml<'_>, path: &str) -> Result<bool, String> {
    let Some(on) = field(document, "on", path)? else {
        return Ok(false);
    };
    Ok(match untagged(on) {
        Yaml::Mapping(_) => field(on, "workflow_call", path)?.is_some(),
        Yaml::Sequence(events) => events
            .iter()
            .any(|event| scalar_text(event) == Some("workflow_call")),
        _ => scalar_text(on) == Some("workflow_call"),
    })
}

fn graph_depth(paths: &[String], edges: &[WorkflowCallEdge]) -> Result<usize, String> {
    let mut outgoing = BTreeMap::<String, Vec<String>>::new();
    let mut incoming_count = paths
        .iter()
        .map(|path| (path.clone(), 0_usize))
        .collect::<BTreeMap<_, _>>();
    for edge in edges {
        let Some(incoming) = incoming_count.get_mut(&edge.callee_path) else {
            return Err(format!(
                "unresolved_local_reusable_workflow:{}:{}",
                edge.caller_path, edge.callee_path
            ));
        };
        *incoming += 1;
        outgoing
            .entry(edge.caller_path.clone())
            .or_default()
            .push(edge.callee_path.clone());
    }

    let mut ready = incoming_count
        .iter()
        .filter_map(|(path, count)| (*count == 0).then_some(path.clone()))
        .collect::<VecDeque<_>>();
    let mut depths = paths
        .iter()
        .map(|path| (path.clone(), 0_usize))
        .collect::<BTreeMap<_, _>>();
    let mut visited = 0;

    while let Some(path) = ready.pop_front() {
        visited += 1;
        let depth = depths
            .get(&path)
            .copied()
            .ok_or_else(|| format!("workflow_graph_node_missing:{path}"))?;
        for callee in outgoing.get(&path).into_iter().flatten() {
            let child_depth = depth + 1;
            let known_depth = depths
                .get_mut(callee)
                .ok_or_else(|| format!("workflow_graph_node_missing:{callee}"))?;
            *known_depth = (*known_depth).max(child_depth);
            let incoming = incoming_count
                .get_mut(callee)
                .ok_or_else(|| format!("workflow_graph_node_missing:{callee}"))?;
            *incoming -= 1;
            if *incoming == 0 {
                ready.push_back(callee.clone());
            }
        }
    }

    if visited != paths.len() {
        return Err("reusable_workflow_cycle".to_owned());
    }
    Ok(depths.values().copied().max().unwrap_or(0))
}

fn field<'a, 'input>(
    value: &'a Yaml<'input>,
    key: &str,
    path: &str,
) -> Result<Option<&'a Yaml<'input>>, String> {
    let Some(entries) = untagged(value).as_mapping() else {
        return Ok(None);
    };
    let mut found = None;
    for (name, value) in entries {
        if scalar_text(name) == Some(key) {
            if found.is_some() {
                return Err(format!("duplicate_workflow_field:{path}:{key}"));
            }
            found = Some(value);
        }
    }
    Ok(found)
}

fn scalar_text<'a>(value: &'a Yaml<'_>) -> Option<&'a str> {
    untagged(value).as_str()
}

fn untagged<'a, 'input>(value: &'a Yaml<'input>) -> &'a Yaml<'input> {
    match value {
        Yaml::Tagged(_, inner) => untagged(inner),
        _ => value,
    }
}

fn is_workflow_path(path: &str) -> bool {
    path.starts_with(".github/workflows/")
        && std::path::Path::new(path)
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| {
                extension.eq_ignore_ascii_case("yml") || extension.eq_ignore_ascii_case("yaml")
            })
}

#[cfg(test)]
mod tests {
    use super::summarize;
    use crate::RenderedFile;

    fn workflow(path: &str, body: &str) -> RenderedFile {
        RenderedFile {
            path: path.to_owned(),
            bytes: body.to_owned(),
        }
    }

    #[test]
    fn summarizes_sorted_full_graph_and_maximum_depth() -> Result<(), String> {
        let files = [
            workflow(
                ".github/workflows/ci.yml",
                "on: push\njobs: {first: {uses: ./.github/workflows/one.yml}, second: {uses: ./.github/workflows/one.yml}, external: {uses: tailrocks/velnor/.github/workflows/check.yml@v1}}",
            ),
            workflow(
                ".github/workflows/one.yml",
                "on: {workflow_call: {}}\njobs: {next: {uses: ./.github/workflows/two.yaml}}",
            ),
            workflow(
                ".github/workflows/two.yaml",
                "on: {workflow_call: {}}\njobs: {work: {runs-on: ubuntu-latest}}",
            ),
            workflow(
                ".github/workflows/standalone.yml",
                "on: workflow_dispatch\njobs: {work: {runs-on: ubuntu-latest}}",
            ),
        ];

        let summary = summarize(&files)?;

        assert_eq!(summary.workflow_count, 4);
        assert_eq!(summary.local_call_edge_count, 3);
        assert_eq!(summary.local_distinct_callee_count, 2);
        assert_eq!(summary.max_local_call_depth, 2);
        assert_eq!(summary.max_local_workflow_levels, 3);
        assert_eq!(
            summary.external_reusable_calls,
            [super::ExternalWorkflowCall {
                caller_path: ".github/workflows/ci.yml".to_owned(),
                job_id: "external".to_owned(),
                uses: "tailrocks/velnor/.github/workflows/check.yml@v1".to_owned(),
            }]
        );
        assert!(summary.workflow_paths.is_sorted());
        assert!(summary.edges.windows(2).all(|pair| {
            (&pair[0].caller_path, &pair[0].job_id, &pair[0].callee_path)
                < (&pair[1].caller_path, &pair[1].job_id, &pair[1].callee_path)
        }));
        Ok(())
    }

    #[test]
    fn rejects_unresolved_local_targets() {
        let unresolved = [workflow(
            ".github/workflows/ci.yml",
            "on: push\njobs: {call: {uses: ./.github/workflows/missing.yml}}",
        )];
        assert!(error_contains(
            &unresolved,
            "unresolved_local_reusable_workflow"
        ));
    }

    #[test]
    fn rejects_cycles_and_malformed_local_calls() {
        let cyclic = [
            workflow(
                ".github/workflows/one.yml",
                "on: {workflow_call: {}}\njobs: {next: {uses: ./.github/workflows/two.yml}}",
            ),
            workflow(
                ".github/workflows/two.yml",
                "on: {workflow_call: {}}\njobs: {next: {uses: ./.github/workflows/one.yml}}",
            ),
        ];
        assert!(error_contains(&cyclic, "reusable_workflow_cycle"));

        let malformed = [workflow(
            ".github/workflows/ci.yml",
            "on: push\njobs: {call: {uses: {invalid: mapping}}}",
        )];
        assert!(error_contains(&malformed, "workflow_job_uses_not_scalar"));
    }

    fn error_contains(files: &[RenderedFile], expected: &str) -> bool {
        matches!(summarize(files), Err(error) if error.contains(expected))
    }
}
