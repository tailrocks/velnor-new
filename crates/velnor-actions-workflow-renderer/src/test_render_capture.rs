//! Test-only capture of complete marked workflow renders.
//!
//! This module is available only through the `test-render-capture` feature.

use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;

use serde::Serialize;

/// Canonical and selected marked YAML captured at the render boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RenderCapture {
    /// Canonical document before the conditional sharing fallback.
    pub canonical: String,
    /// Document selected for the unchanged production size guard.
    pub selected: String,
}

/// One source-bound child workflow emitted only by the explicit capture harness.
#[derive(Clone, Debug, Serialize)]
pub struct SourceJobWorkflowRecord {
    /// Original finalized job id retained inside its child workflow.
    pub source_id: String,
    /// Rust crate-group owner shared by the hosted and Scale Set jobs.
    pub group_id: String,
    /// Lane suffix retained from the original source job.
    pub lane: String,
    /// Flat GitHub Actions reusable-workflow path.
    pub callee_path: String,
    /// Typed lane class and Linux architecture proved for this finalized job.
    pub runner_class: String,
    /// Source selector spelling (`runs-on` token or hosted label).
    pub configured_runner: String,
    /// Final `runs-on` labels serialized in the selected parent workflow.
    pub final_runner_labels: Vec<String>,
    /// Exact child bytes before external capture writes them.
    #[serde(skip_serializing)]
    pub child_yaml: String,
    /// Child YAML byte length.
    pub child_bytes: usize,
    /// SHA-256 of the rendered child YAML.
    pub child_sha256: String,
    /// The five required source-derived caller bindings.
    pub caller_inputs: Vec<SourceJobWorkflowInput>,
}

/// One closed plan value passed from the caller to a generated child.
#[derive(Clone, Debug, Serialize)]
pub struct SourceJobWorkflowInput {
    /// Required workflow-call input name.
    pub name: String,
    /// Exact caller-side GitHub expression.
    pub expression: String,
}

/// Complete source-bound child census for the explicit diagnostic harness.
#[derive(Clone, Debug, Serialize)]
pub struct SourceJobWorkflowCapture {
    /// Validation result; this value is created only after every child passes.
    pub validation: &'static str,
    /// Exact allowlisted source IDs derived from typed Rust crate owners.
    pub source_job_ids: Vec<String>,
    /// Selected parent bytes before replacing the typed Rust jobs.
    pub original_parent_bytes: usize,
    /// Selected diagnostic parent bytes after source-bound extraction.
    pub diagnostic_parent_bytes: usize,
    /// Signed byte reduction from the selected parent to the extracted parent.
    pub parent_bytes_saved: i64,
    /// SHA-256 of the diagnostic parent workflow.
    pub diagnostic_parent_sha256: String,
    /// Unique generated child workflows in the extracted source subgraph.
    pub extracted_unique_callee_count: usize,
    /// Maximum reusable-workflow call depth in the extracted source subgraph.
    pub extracted_max_call_depth: usize,
    /// Distinct workflow targets called directly by the diagnostic parent.
    pub parent_direct_workflow_targets: Vec<String>,
    /// Direct parent targets not included in this extracted-child subgraph.
    pub unmeasured_parent_workflow_targets: Vec<String>,
    /// Whether the full staged tree call graph was available at this capture point.
    pub full_graph_complete: bool,
    /// Full reachable target count, absent when separate workflow files are unavailable.
    pub full_graph_unique_callee_count: Option<usize>,
    /// Full reachable call depth, absent when separate workflow files are unavailable.
    pub full_graph_max_call_depth: Option<usize>,
    /// Unsupported matrix/context cases; nonempty results are never recorded as success.
    pub exceptions: Vec<String>,
    /// Rendered child workflows, in deterministic source-id order.
    pub records: Vec<SourceJobWorkflowRecord>,
}

type SourceJobWorkflowResult = Result<SourceJobWorkflowCapture, String>;

/// Compare generated workflow documents after parsing their shared YAML form.
///
/// # Errors
///
/// Returns the parser diagnostic when either workflow document is invalid.
pub fn workflow_yaml_is_semantically_equal(
    canonical: &str,
    selected: &str,
) -> Result<bool, String> {
    let canonical_file = crate::tree::RenderedFile {
        path: ".github/workflows/ci.yml".to_owned(),
        bytes: canonical.to_owned(),
    };
    let selected_file = crate::tree::RenderedFile {
        path: ".github/workflows/ci.yml".to_owned(),
        bytes: selected.to_owned(),
    };
    let canonical_yaml = crate::workflow_routing::parse_authority_document(&canonical_file)
        .map_err(|error| error.to_string())?;
    let selected_yaml = crate::workflow_routing::parse_authority_document(&selected_file)
        .map_err(|error| error.to_string())?;
    Ok(canonical_yaml == selected_yaml)
}

thread_local! {
    static CAPTURE: RefCell<Option<RenderCapture>> = const { RefCell::new(None) };
    static FULL_TREE_CAPTURE: Cell<bool> = const { Cell::new(false) };
    static SOURCE_JOB_IDS: RefCell<Option<BTreeSet<String>>> = const { RefCell::new(None) };
    static SOURCE_JOB_CAPTURE: RefCell<Option<SourceJobWorkflowResult>> = const { RefCell::new(None) };
}

struct FullTreeCaptureScope(bool);

impl Drop for FullTreeCaptureScope {
    fn drop(&mut self) {
        FULL_TREE_CAPTURE.with(|enabled| enabled.set(self.0));
    }
}

struct SourceJobCaptureScope {
    previous_ids: Option<BTreeSet<String>>,
    previous_capture: Option<SourceJobWorkflowResult>,
}

impl Drop for SourceJobCaptureScope {
    fn drop(&mut self) {
        SOURCE_JOB_IDS.with(|ids| *ids.borrow_mut() = self.previous_ids.take());
        SOURCE_JOB_CAPTURE.with(|capture| {
            *capture.borrow_mut() = self.previous_capture.take();
        });
    }
}

/// Clear the current thread's previous render capture.
pub fn clear() {
    CAPTURE.with(|capture| *capture.borrow_mut() = None);
}

/// Take the latest complete marked render from this thread.
#[must_use]
pub fn take() -> Option<RenderCapture> {
    CAPTURE.with(|capture| capture.borrow_mut().take())
}

/// Run diagnostic capture within a nested, unwind-safe capture scope.
///
/// Production per-file size limits remain active. The caller must also check
/// the complete workflow tree after rendering.
pub fn with_full_tree_capture<T>(capture: impl FnOnce() -> T) -> T {
    let previous = FULL_TREE_CAPTURE.with(|enabled| enabled.replace(true));
    let _scope = FullTreeCaptureScope(previous);
    capture()
}

/// Run source-job capture with an exact caller-provided owner-ID allowlist.
///
/// The renderer records children only while this scope and full-tree capture
/// are both active. Nested scopes restore the prior thread-local state.
pub fn with_source_job_workflow_capture<T>(
    source_ids: BTreeSet<String>,
    capture: impl FnOnce() -> T,
) -> T {
    let previous_ids = SOURCE_JOB_IDS.with(|ids| ids.borrow_mut().replace(source_ids));
    let previous_capture = SOURCE_JOB_CAPTURE.with(|value| value.borrow_mut().take());
    let _scope = SourceJobCaptureScope {
        previous_ids,
        previous_capture,
    };
    capture()
}

/// Take the latest source-bound child census result from this thread.
#[must_use]
pub fn take_source_job_workflow_capture() -> Option<Result<SourceJobWorkflowCapture, String>> {
    SOURCE_JOB_CAPTURE.with(|capture| capture.borrow_mut().take())
}

pub(crate) fn source_job_workflow_ids() -> Option<BTreeSet<String>> {
    if !full_tree_capture_enabled() {
        return None;
    }
    SOURCE_JOB_IDS.with(|ids| ids.borrow().clone())
}

pub(crate) fn begin_source_job_workflow_attempt() {
    if source_job_workflow_ids().is_some() {
        SOURCE_JOB_CAPTURE.with(|capture| *capture.borrow_mut() = None);
    }
}

pub(crate) fn record_source_job_workflow_capture(result: SourceJobWorkflowResult) {
    if source_job_workflow_ids().is_some() {
        SOURCE_JOB_CAPTURE.with(|capture| *capture.borrow_mut() = Some(result));
    }
}

#[cfg(any(test, feature = "test-render-capture"))]
pub(crate) fn full_tree_capture_enabled() -> bool {
    FULL_TREE_CAPTURE.with(Cell::get)
}

pub(crate) fn record(path: &str, canonical: String, selected: String) {
    if path != ".github/workflows/ci.yml" {
        return;
    }
    CAPTURE.with(|capture| {
        *capture.borrow_mut() = Some(RenderCapture {
            canonical,
            selected,
        });
    });
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::panic::{AssertUnwindSafe, catch_unwind};

    use super::{
        begin_source_job_workflow_attempt, full_tree_capture_enabled,
        record_source_job_workflow_capture, source_job_workflow_ids,
        take_source_job_workflow_capture, with_full_tree_capture, with_source_job_workflow_capture,
    };

    #[test]
    fn full_tree_capture_scope_restores_nested_state() {
        assert!(!full_tree_capture_enabled());

        let result = with_full_tree_capture(|| {
            assert!(full_tree_capture_enabled());
            with_full_tree_capture(|| assert!(full_tree_capture_enabled()));
            assert!(full_tree_capture_enabled());
            7
        });

        assert_eq!(result, 7);
        assert!(!full_tree_capture_enabled());
    }

    #[test]
    fn full_tree_capture_scope_restores_state_after_panic() {
        assert!(!full_tree_capture_enabled());

        let result = catch_unwind(AssertUnwindSafe(|| {
            with_full_tree_capture(|| panic!("test unwind"));
        }));

        assert!(result.is_err());
        assert!(!full_tree_capture_enabled());
    }

    #[test]
    fn source_job_capture_is_opt_in_and_attempt_results_do_not_reuse() {
        let ids = BTreeSet::from(["rust-example__hosted".to_owned()]);
        assert!(source_job_workflow_ids().is_none());
        with_source_job_workflow_capture(ids.clone(), || {
            assert!(source_job_workflow_ids().is_none());
            with_full_tree_capture(|| {
                assert_eq!(source_job_workflow_ids(), Some(ids));
                record_source_job_workflow_capture(Err("first attempt".to_owned()));
                assert!(take_source_job_workflow_capture().is_some());
                record_source_job_workflow_capture(Err("stale attempt".to_owned()));
                begin_source_job_workflow_attempt();
                assert!(take_source_job_workflow_capture().is_none());
            });
        });
        assert!(source_job_workflow_ids().is_none());
        assert!(take_source_job_workflow_capture().is_none());
    }
}
