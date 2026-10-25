use super::{
    Scratch, manifest, validate_qualification_manifest, validate_qualification_output_bindings,
};
use std::fs;

#[test]
fn manifest_and_workflow_file_size_limits_are_enforced_on_opened_bytes() {
    let scratch = Scratch::new();
    let manifest_path = scratch.path("manifest.json");
    fs::write(&manifest_path, vec![b' '; 8 * 1024 * 1024 + 1]).expect("write oversized manifest");
    assert!(
        validate_qualification_manifest(&manifest_path)
            .expect_err("oversized manifest is rejected")
            .contains("regular file")
    );

    fs::write(&manifest_path, manifest()).expect("write valid manifest");
    let workflows = scratch.path("workflows");
    fs::create_dir(&workflows).expect("create workflow root");
    let path = workflows.join("large.yml");
    fs::write(&path, vec![b'x'; 8 * 1024 * 1024 + 1]).expect("write oversized workflow");
    let error = validate_qualification_output_bindings(&manifest_path, &workflows)
        .expect_err("oversized workflow file is rejected");
    assert!(error.contains("regular file"), "{error}");
}

#[test]
fn workflow_tree_byte_depth_and_file_count_limits_are_enforced() {
    let scratch = Scratch::new();
    let manifest_path = scratch.path("manifest.json");
    fs::write(&manifest_path, manifest()).expect("write manifest");

    let depth_root = scratch.path("depth");
    fs::create_dir(&depth_root).expect("create root");
    let mut current = depth_root.clone();
    for index in 0..130 {
        current.push(format!("d{index}"));
        fs::create_dir(&current).expect("create nested directory");
    }
    assert!(
        validate_qualification_output_bindings(&manifest_path, &depth_root)
            .expect_err("excessive directory depth is rejected")
            .contains("directory levels")
    );

    let count_root = scratch.path("count");
    fs::create_dir(&count_root).expect("create count root");
    for index in 0..10_001 {
        fs::write(count_root.join(format!("{index}.yml")), b"").expect("create workflow file");
    }
    assert!(
        validate_qualification_output_bindings(&manifest_path, &count_root)
            .expect_err("excessive regular-file count is rejected")
            .contains("10000 regular files")
    );

    let bytes_root = scratch.path("bytes");
    fs::create_dir(&bytes_root).expect("create bytes root");
    let chunk = vec![b'x'; 7 * 1024 * 1024];
    for index in 0..10 {
        fs::write(bytes_root.join(format!("{index}.yml")), &chunk).expect("write workflow chunk");
    }
    let error = validate_qualification_output_bindings(&manifest_path, &bytes_root)
        .expect_err("excessive actual tree bytes are rejected");
    assert!(error.contains("67108864 bytes"), "{error}");
}

#[test]
fn empty_tree_and_tree_without_binding_are_rejected() {
    let scratch = Scratch::new();
    let manifest_path = scratch.path("manifest.json");
    fs::write(&manifest_path, manifest()).expect("write manifest");
    let workflows = scratch.path("workflows");
    fs::create_dir(&workflows).expect("create workflow root");
    assert!(
        validate_qualification_output_bindings(&manifest_path, &workflows)
            .expect_err("empty tree has no bindings")
            .contains("no ConsumerV1 download bindings")
    );
    fs::write(workflows.join("ci.yml"), "name: CI\n").expect("write unbound workflow");
    assert!(
        validate_qualification_output_bindings(&manifest_path, &workflows)
            .expect_err("workflow without bindings is rejected")
            .contains("no ConsumerV1 download bindings")
    );
}

#[cfg(unix)]
#[test]
fn workflow_tree_entry_limit_bounds_symlink_only_breadth() {
    use std::os::unix::fs::symlink;

    let scratch = Scratch::new();
    let manifest_path = scratch.path("manifest.json");
    fs::write(&manifest_path, manifest()).expect("write manifest");
    let workflows = scratch.path("links");
    fs::create_dir(&workflows).expect("create workflow root");
    for index in 0..20_001 {
        symlink("missing-target", workflows.join(format!("link-{index}")))
            .expect("create leaf symlink");
    }
    assert!(
        validate_qualification_output_bindings(&manifest_path, &workflows)
            .expect_err("symlink-only tree exceeds total entry bound")
            .contains("20000 total entries")
    );
}
