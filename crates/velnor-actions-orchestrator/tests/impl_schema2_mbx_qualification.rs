//! Security assertions for the generated single-bundle MBX round trip.

use velnor_actions_orchestrator::{prepare, render_staged_tree};

use crate::impl_common::{TestResult, make_repo};

#[path = "impl_schema2_mbx_qualification_helpers.rs"]
mod helpers;

use self::helpers::{
    BUNDLE_PATH, CACHE_RESTORE_PIN, CACHE_SAVE_PIN, MBX_GENERATION, MBX_VERSION,
    QUALIFICATION_SCOPE, assert_compression_profile, assert_external_cache_actions,
    assert_key_contract, assert_mbx_action_pin, assert_no_implicit_cache, assert_ordered,
    assert_qualification_gate, assert_same_step_value, mbx_action_version, step_body, step_value,
};

#[test]
fn protected_main_writer_and_fresh_reader_use_production_bundle_graph() -> TestResult {
    let repo = make_repo(&crate::impl_schema2_routing::workflow_config())?;
    let tree = render_staged_tree(&prepare(repo.path())?)?;
    let qualification =
        crate::impl_schema2_routing::required_file(&tree, ".github/workflows/qualification.yml")?;
    assert!(
        qualification.contains("workflow_dispatch:"),
        "{qualification}"
    );
    assert!(qualification.contains("mode:"), "{qualification}");
    let writer = crate::impl_schema2_routing::job_body(qualification, "mbx-cache-write-hosted")?;
    let reader = crate::impl_schema2_routing::job_body(qualification, "mbx-cache-read-hosted")?;

    assert_writer(writer);
    assert_reader(reader);
    assert_shared_identity(writer, reader);
    Ok(())
}

fn assert_writer(writer: &str) {
    assert_qualification_gate(writer);
    assert!(writer.contains("runs-on: ubuntu-26.04"), "{writer}");
    assert!(writer.contains("actions: write"), "{writer}");
    assert!(writer.contains("MBX_GC_AUTO: \"1\""), "{writer}");
    assert!(writer.contains("RUSTUP_TOOLCHAIN: 1.98.1"), "{writer}");
    assert!(
        writer.contains("MBX_CACHE_SCOPE: qualification-mbx-v1/single-bundle-roundtrip"),
        "{writer}"
    );
    assert!(
        writer.contains("MBX_MATRIX_CONTEXT: ${{ toJSON(matrix) }}"),
        "{writer}"
    );
    assert!(writer.contains("rustc --print sysroot"), "{writer}");
    assert_compression_profile(writer);
    assert_mbx_action_pin(writer);
    assert_external_cache_actions(writer, true);
    assert_eq!(writer.matches("if: success() &&").count(), 2, "{writer}");
    assert!(
        writer.contains("success() && inputs.mode == 'mbx-cache-roundtrip' && github.event_name == 'workflow_dispatch' && github.ref == 'refs/heads/main' && github.ref_protected == true && steps.mbx-bundle.outputs.cache-hit != 'true'"),
        "{writer}"
    );
    assert!(
        writer.contains("steps.mbx-export.outputs.ready == 'true'"),
        "{writer}"
    );
    assert_eq!(
        writer
            .matches("inputs.mode == 'mbx-cache-roundtrip'")
            .count(),
        3,
        "{writer}"
    );
    assert!(!writer.contains("github.event_name == 'push'"), "{writer}");
    assert!(
        !writer.contains("github.event_name == 'pull_request'"),
        "{writer}"
    );
    assert!(!writer.contains("continue-on-error"), "{writer}");
    assert!(!writer.contains("if: always()"), "{writer}");
    assert_no_implicit_cache(writer);
    assert!(
        step_body(writer, "Prepare private MBX store").contains("velnor-mbx-store.XXXXXXXXXX"),
        "{writer}"
    );
    assert!(
        step_body(writer, "Import MBX single bundle").contains("velnor-mbx-fallback.XXXXXXXXXX"),
        "{writer}"
    );
    assert_ordered(
        writer,
        &[
            "name: Verify cache compression support",
            "name: Prepare private MBX store",
            "name: Setup MBX",
            "name: Prepare MBX bundle key",
            "name: Restore MBX single bundle",
            "name: Import MBX single bundle",
            "name: Verify pinned MBX version",
            "name: Compile MBX cache probe",
            "name: Export MBX single bundle",
            "name: Save MBX single bundle",
        ],
    );
}

fn assert_reader(reader: &str) {
    assert_qualification_gate(reader);
    assert_eq!(
        reader
            .matches("inputs.mode == 'mbx-cache-roundtrip'")
            .count(),
        1,
        "{reader}"
    );
    assert!(
        reader.contains("needs:\n      - mbx-cache-write-hosted"),
        "{reader}"
    );
    assert!(reader.contains("runs-on: ubuntu-26.04"), "{reader}");
    assert!(reader.contains("actions: read"), "{reader}");
    assert!(!reader.contains("actions: write"), "{reader}");
    assert!(reader.contains("MBX_GC_AUTO: \"1\""), "{reader}");
    assert!(
        reader.contains("MBX_CACHE_SCOPE: qualification-mbx-v1/single-bundle-roundtrip"),
        "{reader}"
    );
    assert_compression_profile(reader);
    assert_mbx_action_pin(reader);
    assert_external_cache_actions(reader, false);
    assert!(reader.contains("mbx cache stats --json"), "{reader}");
    assert!(reader.contains(".objects > 0"), "{reader}");
    assert!(
        reader.contains("MBX bundle import failed; abandoning its private store"),
        "{reader}"
    );
    assert!(
        reader.contains(".savings.cached_compilations > 0"),
        "{reader}"
    );
    assert!(!reader.contains("continue-on-error"), "{reader}");
    assert_no_implicit_cache(reader);
    assert!(
        step_body(reader, "Prepare private MBX store").contains("velnor-mbx-store.XXXXXXXXXX"),
        "{reader}"
    );
    assert!(
        step_body(reader, "Import MBX single bundle").contains("velnor-mbx-fallback.XXXXXXXXXX"),
        "{reader}"
    );
    assert!(
        !reader.contains("name: Export MBX single bundle"),
        "{reader}"
    );
    assert!(!reader.contains("name: Save MBX single bundle"), "{reader}");
    assert_ordered(
        reader,
        &[
            "name: Verify cache compression support",
            "name: Prepare private MBX store",
            "name: Setup MBX",
            "name: Prepare MBX bundle key",
            "name: Restore MBX single bundle",
            "name: Import MBX single bundle",
            "name: Verify pinned MBX version",
            "name: Require imported MBX objects",
            "name: Compile MBX cache probe",
            "name: Require reused compilation",
        ],
    );
}

fn assert_shared_identity(writer: &str, reader: &str) {
    assert_eq!(mbx_action_version(writer), mbx_action_version(reader));
    assert_eq!(mbx_action_version(writer), Some(MBX_VERSION));
    for key in [
        "MBX_VERSION",
        "MBX_EXPECTED_VERSION",
        "MBX_GENERATION",
        "MBX_CACHE_SCOPE",
        "MBX_MATRIX_CONTEXT",
        "RUSTUP_TOOLCHAIN",
        "MISE_RUSTUP_HOME",
        "MISE_CARGO_HOME",
    ] {
        assert_same_step_value(writer, reader, "Prepare MBX bundle key", key);
    }
    assert_eq!(
        step_value(writer, "Prepare MBX bundle key", "MBX_CACHE_SCOPE"),
        Some(QUALIFICATION_SCOPE)
    );
    assert_eq!(
        step_value(writer, "Prepare MBX bundle key", "MBX_EXPECTED_VERSION"),
        Some(MBX_VERSION)
    );
    assert_eq!(
        step_value(writer, "Prepare MBX bundle key", "MBX_GENERATION"),
        Some(MBX_GENERATION)
    );
    assert_eq!(
        step_value(writer, "Prepare MBX bundle key", "MBX_MATRIX_CONTEXT"),
        Some("${{ toJSON(matrix) }}")
    );
    let writer_restore = step_body(writer, "Restore MBX single bundle");
    let reader_restore = step_body(reader, "Restore MBX single bundle");
    assert!(
        writer_restore.contains(CACHE_RESTORE_PIN),
        "{writer_restore}"
    );
    assert!(
        reader_restore.contains(CACHE_RESTORE_PIN),
        "{reader_restore}"
    );
    assert_same_step_value(writer, reader, "Restore MBX single bundle", "path");
    assert_same_step_value(writer, reader, "Restore MBX single bundle", "key");
    assert_eq!(
        step_value(writer, "Restore MBX single bundle", "restore-keys"),
        None,
        "qualification writer must not restore a previous run's cache"
    );
    assert_eq!(
        step_value(reader, "Restore MBX single bundle", "restore-keys"),
        None,
        "qualification reader must use only the designated writer's exact key"
    );
    assert_eq!(
        step_value(writer, "Restore MBX single bundle", "path"),
        Some(BUNDLE_PATH)
    );
    let writer_save = step_body(writer, "Save MBX single bundle");
    assert!(writer_save.contains(CACHE_SAVE_PIN), "{writer_save}");
    assert_eq!(
        step_value(writer, "Save MBX single bundle", "if"),
        Some(concat!(
            "success() && inputs.mode == 'mbx-cache-roundtrip' && ",
            "github.event_name == 'workflow_dispatch' && ",
            "github.ref == 'refs/heads/main' && github.ref_protected == true && ",
            "steps.mbx-bundle.outputs.cache-hit != 'true' && ",
            "steps.mbx-export.outputs.ready == 'true'"
        ))
    );
    assert!(
        writer_save.contains("key: ${{ steps.mbx-bundle-key.outputs.primary }}"),
        "{writer_save}"
    );
    assert!(
        writer_save.contains("path: ${{ runner.temp }}/mbx-single-bundle"),
        "{writer_save}"
    );
    assert_key_contract(step_body(writer, "Prepare MBX bundle key"));
    assert_key_contract(step_body(reader, "Prepare MBX bundle key"));
}
