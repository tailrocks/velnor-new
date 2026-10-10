//! Exact-consumer diagnostic harness. This test is deliberately ignored and
//! runs only with the private `test-render-capture` feature enabled.

use std::collections::BTreeSet;
use std::path::PathBuf;

use crate::cover_identity::generator::sha256_hex;
use velnor_actions_contract::WorkflowTask;
use velnor_actions_workflow_renderer::render::test_render_capture::full_tree_capture_guard;
use velnor_actions_workflow_renderer::tree::RenderedTree;

/// The exact currently published helper release pinned by the consumer input.
const CAPTURE_CONSUMER_RELEASE_VERSION: &str = "0.1.4";

#[test]
#[ignore = "requires VELNOR_CAPTURE_ROOT and external VELNOR_CAPTURE_OUT"]
fn capture_exact_consumer_marked_workflow_before_size_guard()
-> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::var_os("VELNOR_CAPTURE_ROOT").ok_or("missing root")?);
    let output = PathBuf::from(std::env::var_os("VELNOR_CAPTURE_OUT").ok_or("missing output")?);
    let root = root.canonicalize()?;
    std::fs::create_dir_all(&output)?;
    let output = output.canonicalize()?;
    if output.starts_with(&root) {
        return Err("capture output must be outside the consumer checkout".into());
    }
    if std::fs::read_dir(&output)?.next().is_some() {
        return Err("capture output directory must be empty".into());
    }

    let preparation = crate::prepare::prepare_for_capture(&root, CAPTURE_CONSUMER_RELEASE_VERSION)?;
    assert_eq!(
        preparation.config.workflow.policy,
        velnor_actions_contract::WorkflowPolicy::ConsumerV1
    );
    let task_helper_versions = preparation
        .workflow
        .ir
        .jobs
        .values()
        .flat_map(|job| &job.steps)
        .filter_map(|step| match &step.kind {
            velnor_actions_contract::StepKind::TaskExecution {
                report_helper_version,
                ..
            } => Some(report_helper_version.as_str()),
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(
        task_helper_versions,
        BTreeSet::from([CAPTURE_CONSUMER_RELEASE_VERSION]),
        "every produced task wrapper must use the same validated consumer helper release"
    );
    let configured_tasks = preparation
        .config
        .workflow
        .tasks
        .iter()
        .map(WorkflowTask::id)
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    assert_eq!(
        configured_tasks,
        BTreeSet::from([
            "construct-upstream-assets".to_owned(),
            "native-desktop-ci".to_owned(),
            "native-swift-format".to_owned(),
            "native-swiftlint".to_owned(),
        ]),
        "capture input must retain all four configured consumer tasks"
    );
    let workspace_members = preparation
        .discovery
        .workspaces
        .iter()
        .flat_map(|workspace| workspace.record.members.iter().map(String::as_str))
        .collect::<BTreeSet<_>>();
    assert_eq!(
        workspace_members.len(),
        150,
        "capture input must retain every Cargo workspace member"
    );

    let _capture_guard = full_tree_capture_guard();
    let rendered = crate::render_staged_tree_with(&preparation, None);
    let capture = velnor_actions_workflow_renderer::render::test_render_capture::take();
    let capture = capture.ok_or_else(|| match &rendered {
        Ok(_) => "render boundary did not record a workflow".to_owned(),
        Err(error) => format!("render failed before workflow capture: {error}"),
    })?;
    let canonical_bytes = capture.canonical.len();
    let selected_bytes = capture.selected.len();
    assert_required_task_coverage(&capture.selected, &configured_tasks);
    std::fs::write(output.join("canonical.yml"), capture.canonical.as_bytes())?;
    std::fs::write(output.join("selected.yml"), capture.selected.as_bytes())?;
    let mut shared_bytes = 0usize;
    let mut shared_paths = BTreeSet::new();
    for file in &capture.shared {
        if !shared_paths.insert(&file.path) {
            return Err(format!("duplicate shared action path: {}", file.path).into());
        }
        let safe = velnor_actions_workflow_renderer::guard::validate_tree_path(&file.path)?;
        let destination = output.join("shared").join(safe.as_str());
        let parent = destination.parent().ok_or("shared output has no parent")?;
        std::fs::create_dir_all(parent)?;
        std::fs::write(&destination, file.bytes.as_bytes())?;
        shared_bytes += file.bytes.len();
        eprintln!(
            "pre-cap shared file: path={} bytes={}",
            file.path,
            file.bytes.len()
        );
    }
    let tree = capture
        .tree
        .as_ref()
        .ok_or("complete generated tree was not captured before the size guard")?;
    let tree_bytes = write_tree_capture(tree, &output)?;
    let validation_runtime = output.join("validation-runtime");
    std::fs::create_dir_all(&validation_runtime)?;
    crate::validate_zizmor::write_zizmor_config(&validation_runtime, tree)?;
    let zizmor = std::fs::read(validation_runtime.join(".zizmor.yml"))?;
    let zizmor_sha256 = sha256_hex(&zizmor);
    std::fs::write(
        output.join("validation-runtime-inventory.tsv"),
        format!(
            "path\tbytes\tsha256\n.zizmor.yml\t{}\t{}\n",
            zizmor.len(),
            zizmor_sha256
        ),
    )?;
    eprintln!(
        "pre-cap capture written: canonical_bytes={canonical_bytes} selected_bytes={selected_bytes} shared_files={} shared_bytes={shared_bytes} tree_files={} tree_bytes={tree_bytes}; post-assembly validator config validation-runtime/.zizmor.yml bytes={} sha256={zizmor_sha256}",
        capture.shared.len(),
        tree.files.len(),
        zizmor.len()
    );
    let tree = match rendered {
        Ok(tree) => {
            assert_eq!(
                &tree,
                capture.tree.as_ref().expect("tree was checked above")
            );
            tree
        }
        Err(error)
            if capture_overflow_matches(
                &error.to_string(),
                output.join("selected.yml").as_path(),
            )? =>
        {
            crate::validate::validate_staged(tree)?;
            eprintln!(
                "diagnostic full-tree capture: canonical_bytes={canonical_bytes} selected_bytes={selected_bytes} shared_files={} shared_bytes={shared_bytes} tree_files={} tree_bytes={tree_bytes}; validator tree still exceeds the unchanged 500000-byte workflow cap",
                capture.shared.len(),
                tree.files.len()
            );
            return Err("diagnostic capture recorded an over-cap generated tree; this is not a passing size gate".into());
        }
        Err(error) => return Err(error.into()),
    };
    crate::validate::validate_staged(&tree)?;
    assert!(tree.get(".github/workflows/ci.yml").is_some());
    assert_eq!(
        tree.get(".github/workflows/ci.yml"),
        Some(capture.selected.as_str())
    );
    for file in &capture.shared {
        assert_eq!(tree.get(&file.path), Some(file.bytes.as_str()));
    }
    Ok(())
}

/// Prove each configured task remains a job and gates the generated Required job.
fn assert_required_task_coverage(workflow: &str, task_ids: &BTreeSet<String>) {
    let required_needs = yaml_job_needs(workflow, "required");
    for task_id in task_ids {
        let job_id = format!("task-{task_id}");
        assert!(
            yaml_has_job(workflow, &job_id),
            "configured workflow task has no rendered job: {job_id}"
        );
        assert!(
            required_needs.contains(&job_id),
            "configured workflow task does not gate Required: {job_id}"
        );
    }
}

/// Check the generated YAML's job mapping without introducing a YAML parser dependency.
fn yaml_has_job(workflow: &str, job_id: &str) -> bool {
    let expected = format!("  {job_id}:");
    workflow.lines().any(|line| line == expected.as_str())
}

/// Read the renderer's block-sequence `needs` for one generated job.
fn yaml_job_needs(workflow: &str, job_id: &str) -> BTreeSet<String> {
    let header = format!("  {job_id}:");
    let lines = workflow.lines().collect::<Vec<_>>();
    let matching_headers = lines
        .iter()
        .enumerate()
        .filter_map(|(index, line)| (*line == header.as_str()).then_some(index))
        .collect::<Vec<_>>();
    assert_eq!(
        matching_headers.len(),
        1,
        "rendered job must occur exactly once: {job_id}"
    );
    let mut in_needs = false;
    let mut needs = BTreeSet::new();
    for line in lines.iter().skip(matching_headers[0] + 1) {
        if line.starts_with("  ") && !line.starts_with("    ") {
            break;
        }
        if *line == "    needs:" {
            assert!(
                !in_needs,
                "duplicate needs field for rendered job: {job_id}"
            );
            in_needs = true;
            continue;
        }
        if in_needs {
            let Some(need) = line.strip_prefix("      - ") else {
                break;
            };
            assert!(needs.insert(need.to_owned()), "duplicate need: {need}");
        }
    }
    assert!(in_needs, "rendered job has no needs field: {job_id}");
    needs
}

fn write_tree_capture(
    tree: &RenderedTree,
    output: &std::path::Path,
) -> Result<usize, Box<dyn std::error::Error>> {
    if !tree.symlinks.is_empty() {
        return Err("captured generated tree unexpectedly contains symlinks".into());
    }
    let tree_root = output.join("tree");
    std::fs::create_dir_all(&tree_root)?;
    let mut seen = BTreeSet::new();
    let mut total_bytes = 0usize;
    let mut inventory = String::from("path\tbytes\tsha256\n");
    for file in &tree.files {
        if !seen.insert(file.path.as_str()) {
            return Err(format!("duplicate rendered tree path: {}", file.path).into());
        }
        let safe = velnor_actions_workflow_renderer::guard::validate_tree_path(&file.path)?;
        let destination = tree_root.join(safe.as_str());
        let parent = destination.parent().ok_or("tree file has no parent")?;
        std::fs::create_dir_all(parent)?;
        std::fs::write(&destination, file.bytes.as_bytes())?;
        let digest = sha256_hex(file.bytes.as_bytes());
        total_bytes += file.bytes.len();
        inventory.push_str(&format!(
            "{}\t{}\t{}\n",
            file.path,
            file.bytes.len(),
            digest
        ));
        eprintln!(
            "pre-cap tree file: path={} bytes={}",
            file.path,
            file.bytes.len()
        );
    }
    std::fs::write(output.join("tree-inventory.tsv"), inventory)?;
    Ok(total_bytes)
}

fn capture_overflow_matches(
    error: &str,
    selected_path: &std::path::Path,
) -> Result<bool, std::io::Error> {
    let selected_bytes = std::fs::read(selected_path)?.len();
    Ok(error.contains(&format!(
        "workflow_too_large:.github/workflows/ci.yml:{selected_bytes}:500000"
    )))
}
