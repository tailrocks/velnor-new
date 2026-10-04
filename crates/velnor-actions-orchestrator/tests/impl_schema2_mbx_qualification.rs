//! Security assertions for the generated single-bundle MBX round trip.

use velnor_actions_orchestrator::{prepare, render_staged_tree};

use crate::impl_common::{TestResult, make_repo};

#[path = "impl_schema2_mbx_qualification_helpers.rs"]
mod helpers;

use self::helpers::{
    assert_compression_profile, assert_external_cache_actions, assert_mbx_action_pin,
    assert_mbx_bootstrap, assert_mbx_preflight, assert_no_implicit_cache, assert_ordered,
    assert_qualification_gate, assert_shared_identity, step_body,
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
    assert_compression_profile(writer);
    assert_mbx_action_pin(writer);
    assert_mbx_bootstrap(writer);
    assert_mbx_preflight(writer);
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
            "name: Install pinned Rust and MBX toolchains",
            "name: Verify cache compression support",
            "name: Verify MBX and Rust toolchains",
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
    assert_mbx_bootstrap(reader);
    assert_mbx_preflight(reader);
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
            "name: Install pinned Rust and MBX toolchains",
            "name: Verify cache compression support",
            "name: Verify MBX and Rust toolchains",
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
