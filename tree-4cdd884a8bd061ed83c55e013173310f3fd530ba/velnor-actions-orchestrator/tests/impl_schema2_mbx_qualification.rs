//! Security assertions for the protected-main MBX cache round trip.

use velnor_actions_orchestrator::{prepare, render_staged_tree};

use crate::impl_common::{TestResult, make_repo};

#[test]
fn protected_main_mbx_roundtrip_is_run_bound_and_read_only_on_restore() -> TestResult {
    let repo = make_repo(&crate::impl_schema2_routing::workflow_config())?;
    let tree = render_staged_tree(&prepare(repo.path())?)?;
    let qualification =
        crate::impl_schema2_routing::required_file(&tree, ".github/workflows/qualification.yml")?;

    let writer = crate::impl_schema2_routing::job_body(qualification, "mbx-cache-write-hosted")?;
    assert!(
        writer.contains("inputs.mode == 'mbx-cache-roundtrip' && github.ref == 'refs/heads/main' && github.ref_protected == true"),
        "{writer}"
    );
    assert!(writer.contains("actions: write"), "{writer}");
    assert!(writer.contains("ACTIONS_CACHE_MODE: write"), "{writer}");
    assert!(writer.contains("MBX_GC_AUTO: \"1\""), "{writer}");
    assert!(
        writer.contains("save-on-workflow-dispatch: \"true\""),
        "{writer}"
    );
    assert!(
        writer.contains("velnor-qualification-mbx-1.21.1-action-1687e54eb349cadf61fa38b5813a77875489e8e6-run-${{ github.run_id }}-${{ github.run_attempt }}-${{ github.sha }}"),
        "{writer}"
    );
    assert!(writer.contains("version: 1.21.1"), "{writer}");
    assert!(writer.contains("RUSTUP_TOOLCHAIN: 1.98.1"), "{writer}");
    assert!(
        writer
            .contains("CARGO_HOME: ${{ github.workspace }}/.velnor-mbx-cache-qualification/cargo"),
        "{writer}"
    );
    assert!(writer.contains("rustc --print sysroot"), "{writer}");

    let reader = crate::impl_schema2_routing::job_body(qualification, "mbx-cache-read-hosted")?;
    assert!(
        reader.contains("inputs.mode == 'mbx-cache-roundtrip' && github.ref == 'refs/heads/main' && github.ref_protected == true"),
        "{reader}"
    );
    assert!(
        reader.contains("needs:\n      - mbx-cache-write-hosted"),
        "{reader}"
    );
    assert!(reader.contains("actions: read"), "{reader}");
    assert!(reader.contains("ACTIONS_CACHE_MODE: read"), "{reader}");
    assert!(reader.contains("MBX_GC_AUTO: \"1\""), "{reader}");
    assert!(
        reader.contains("steps.mbx_cache.outputs.cache-hit"),
        "{reader}"
    );
    assert!(
        reader.contains("velnor-qualification-mbx-1.21.1-action-1687e54eb349cadf61fa38b5813a77875489e8e6-run-${{ github.run_id }}-${{ github.run_attempt }}-${{ github.sha }}"),
        "{reader}"
    );
    assert!(
        reader.contains("test \\\"$CACHE_HIT\\\" = 'false'"),
        "{reader}"
    );
    assert!(
        reader.contains("save-on-workflow-dispatch: \"false\""),
        "{reader}"
    );
    assert!(reader.contains("mbx cache stats --json"), "{reader}");
    assert!(
        reader.contains(".savings.cached_compilations > 0"),
        "{reader}"
    );
    assert!(reader.contains("version: 1.21.1"), "{reader}");
    assert!(reader.contains("RUSTUP_TOOLCHAIN: 1.98.1"), "{reader}");
    assert!(
        reader
            .contains("CARGO_HOME: ${{ github.workspace }}/.velnor-mbx-cache-qualification/cargo"),
        "{reader}"
    );
    Ok(())
}
