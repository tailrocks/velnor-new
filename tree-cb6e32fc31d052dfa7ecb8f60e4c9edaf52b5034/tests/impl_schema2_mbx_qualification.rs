//! Security assertions for the generated single-bundle MBX round trip.

use velnor_actions_orchestrator::{prepare, render_staged_tree};

use crate::impl_common::{TestResult, make_repo};

#[test]
fn protected_main_writer_and_fresh_reader_use_production_bundle_graph() -> TestResult {
    let repo = make_repo(&crate::impl_schema2_routing::workflow_config())?;
    let tree = render_staged_tree(&prepare(repo.path())?)?;
    let qualification =
        crate::impl_schema2_routing::required_file(&tree, ".github/workflows/qualification.yml")?;
    let writer = crate::impl_schema2_routing::job_body(qualification, "mbx-cache-write-hosted")?;
    let reader = crate::impl_schema2_routing::job_body(qualification, "mbx-cache-read-hosted")?;

    assert_writer(writer);
    assert_reader(reader);
    assert_shared_profile(writer, reader);
    Ok(())
}

fn assert_writer(writer: &str) {
    assert_qualification_gate(writer);
    assert!(writer.contains("actions: write"), "{writer}");
    assert!(writer.contains("backend: local"), "{writer}");
    assert!(writer.contains("ACTIONS_CACHE_MODE: read"), "{writer}");
    assert!(writer.contains("MBX_GC_AUTO: \"1\""), "{writer}");
    assert!(writer.contains("MBX_CACHE_DIR:"), "{writer}");
    assert!(
        writer.contains("MBX_CACHE_EXPORT_GROUP: velnor-qualification-mbx-"),
        "{writer}"
    );
    assert!(writer.contains("RUSTUP_TOOLCHAIN: 1.98.1"), "{writer}");
    assert!(writer.contains("rustc --print sysroot"), "{writer}");
    assert_mbx_action_pin(writer);
    assert!(
        writer.contains("actions/cache/restore@55cc8345863c7cc4c66a329aec7e433d2d1c52a9"),
        "{writer}"
    );
    assert!(
        writer.contains("actions/cache/save@55cc8345863c7cc4c66a329aec7e433d2d1c52a9"),
        "{writer}"
    );
    assert!(
        writer.contains("success() && inputs.mode == 'mbx-cache-roundtrip' && github.event_name == 'workflow_dispatch' && github.ref == 'refs/heads/main' && github.ref_protected == true"),
        "{writer}"
    );
    assert_eq!(writer.matches("if: success() &&").count(), 2, "{writer}");
    assert!(!writer.contains("continue-on-error"), "{writer}");
    assert!(
        writer.contains("mbx bundle import failed; continuing cold"),
        "{writer}"
    );
    assert_no_implicit_cache(writer);
    assert_ordered(
        writer,
        &[
            "name: Restore MBX objects",
            "name: Prepare MBX bundle key",
            "name: Restore MBX single bundle",
            "name: Import MBX single bundle",
            "name: Verify pinned MBX version",
            "name: Compile MBX cache probe",
            "name: Export MBX single bundle",
            "name: Save MBX single bundle",
        ],
    );
    assert_eq!(
        writer.matches("uses: actions/cache/restore@").count(),
        1,
        "{writer}"
    );
    assert_eq!(
        writer.matches("uses: actions/cache/save@").count(),
        1,
        "{writer}"
    );
}

fn assert_reader(reader: &str) {
    assert_qualification_gate(reader);
    assert!(
        reader.contains("needs:\n      - mbx-cache-write-hosted"),
        "{reader}"
    );
    assert!(reader.contains("actions: read"), "{reader}");
    assert!(reader.contains("backend: local"), "{reader}");
    assert!(reader.contains("ACTIONS_CACHE_MODE: read"), "{reader}");
    assert_mbx_action_pin(reader);
    assert!(reader.contains("mbx cache stats --json"), "{reader}");
    assert!(reader.contains(".objects > 0"), "{reader}");
    assert!(
        reader.contains("mbx bundle import failed; continuing cold"),
        "{reader}"
    );
    assert!(
        reader.contains(".savings.cached_compilations > 0"),
        "{reader}"
    );
    assert!(
        reader.contains("path: ${{ runner.temp }}/mbx-single-bundle"),
        "{reader}"
    );
    assert!(
        reader.contains("steps.mbx-bundle.outputs.cache-matched-key"),
        "{reader}"
    );
    assert_no_implicit_cache(reader);
    assert!(
        !reader.contains("name: Export MBX single bundle"),
        "{reader}"
    );
    assert!(!reader.contains("name: Save MBX single bundle"), "{reader}");
    assert_eq!(
        reader.matches("uses: actions/cache/restore@").count(),
        1,
        "{reader}"
    );
    assert_eq!(
        reader.matches("uses: actions/cache/save@").count(),
        0,
        "{reader}"
    );
    assert_ordered(
        reader,
        &[
            "name: Restore MBX objects",
            "name: Prepare MBX bundle key",
            "name: Restore MBX single bundle",
            "name: Import MBX single bundle",
            "name: Require imported MBX objects",
            "name: Compile MBX cache probe",
            "name: Require reused compilation",
        ],
    );
}

fn assert_shared_profile(writer: &str, reader: &str) {
    let writer_version = mbx_action_version(writer);
    assert!(writer_version.is_some(), "MBX version missing: {writer}");
    assert_eq!(writer_version, mbx_action_version(reader));
    for key in [
        "MBX_CACHE_DIR",
        "MBX_CACHE_EXPORT_GROUP",
        "path",
        "key",
        "restore-keys",
    ] {
        assert_same_value(writer, reader, key);
    }
}

fn assert_qualification_gate(job: &str) {
    assert!(
        job.contains("if: inputs.mode == 'mbx-cache-roundtrip' && github.event_name == 'workflow_dispatch' && github.ref == 'refs/heads/main' && github.ref_protected == true"),
        "{job}"
    );
}

fn assert_no_implicit_cache(job: &str) {
    assert!(!job.contains("github-cache-mode:"), "{job}");
    assert!(!job.contains("save-on-workflow-dispatch:"), "{job}");
}

fn assert_mbx_action_pin(job: &str) {
    let Some(start) = job.find("name: Restore MBX objects") else {
        assert!(job.contains("name: Restore MBX objects"), "{job}");
        return;
    };
    let action = &job[start..];
    let Some(uses) = action
        .lines()
        .find_map(|line| line.trim().strip_prefix("uses: "))
    else {
        assert!(action.contains("uses: jdx/mr-boxington-action@"), "{job}");
        return;
    };
    let Some(sha) = uses.strip_prefix("jdx/mr-boxington-action@") else {
        assert!(uses.starts_with("jdx/mr-boxington-action@"), "{uses}");
        return;
    };
    assert_eq!(sha.len(), 40, "{uses}");
    assert!(
        sha.bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()),
        "{uses}"
    );
}

fn mbx_action_version(job: &str) -> Option<&str> {
    let start = job.find("name: Restore MBX objects")?;
    let end = job[start..].find("name: Prepare MBX bundle key")?;
    job[start..start + end]
        .lines()
        .find_map(|line| line.trim().strip_prefix("version: "))
}

fn assert_same_value(writer: &str, reader: &str, key: &str) {
    let prefix = format!("{key}: ");
    let writer_value = writer
        .lines()
        .find_map(|line| line.trim().strip_prefix(&prefix));
    let reader_value = reader
        .lines()
        .find_map(|line| line.trim().strip_prefix(&prefix));
    assert!(
        writer_value.is_some(),
        "{key} missing from writer: {writer}"
    );
    assert_eq!(writer_value, reader_value, "{key} differs between jobs");
}

fn assert_ordered(job: &str, names: &[&str]) {
    let mut previous = 0;
    for name in names {
        let position = job[previous..].find(name);
        assert!(
            position.is_some(),
            "missing or out of order `{name}` in {job}"
        );
        previous += position.unwrap_or_default() + name.len();
    }
}
