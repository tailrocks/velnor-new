//! Exact-consumer diagnostic harness. This test is deliberately ignored and
//! runs only with the private `test-render-capture` feature enabled.

use std::collections::BTreeSet;
use std::path::PathBuf;

use sha2::{Digest, Sha256};
use velnor_actions_workflow_renderer::render::test_render_capture::full_tree_capture_guard;
use velnor_actions_workflow_renderer::tree::RenderedTree;

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

    let preparation = crate::prepare(&root)?;
    let execution = preparation
        .config
        .execution
        .as_ref()
        .ok_or("missing execution config")?;
    assert_eq!(
        execution.mode,
        Some(velnor_actions_contract::ExecutionMode::Both)
    );
    assert!(
        execution
            .workflows
            .contains(&velnor_actions_contract::RoutingWorkflow::Qualification)
    );

    let _capture_guard = full_tree_capture_guard();
    let rendered = crate::render_staged_tree_with(&preparation, None);
    let capture = velnor_actions_workflow_renderer::render::test_render_capture::take();
    let capture = capture.ok_or("render boundary did not record a workflow")?;
    let canonical_bytes = capture.canonical.len();
    let selected_bytes = capture.selected.len();
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
    let zizmor_sha256 = Sha256::digest(&zizmor);
    std::fs::write(
        output.join("validation-runtime-inventory.tsv"),
        format!(
            "path\tbytes\tsha256\n.zizmor.yml\t{}\t{:x}\n",
            zizmor.len(),
            zizmor_sha256
        ),
    )?;
    eprintln!(
        "pre-cap capture written: canonical_bytes={canonical_bytes} selected_bytes={selected_bytes} shared_files={} shared_bytes={shared_bytes} tree_files={} tree_bytes={tree_bytes}; post-assembly validator config validation-runtime/.zizmor.yml bytes={} sha256={:x}",
        capture.shared.len(),
        tree.files.len(),
        zizmor.len(),
        zizmor_sha256
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
        let digest = Sha256::digest(file.bytes.as_bytes());
        total_bytes += file.bytes.len();
        inventory.push_str(&format!(
            "{}\t{}\t{:x}\n",
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
