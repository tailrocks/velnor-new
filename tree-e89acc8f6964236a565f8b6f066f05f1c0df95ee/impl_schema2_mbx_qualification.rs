//! Security assertions for the protected-main MBX cache round trip.

use velnor_actions_orchestrator::{prepare, render_staged_tree};

use crate::impl_common::{TestResult, make_repo};

const CACHE_GENERATION: &str = "velnor-qualification-mbx-1.21.1-share-out-dir-disabled-v1-action-d0825fbaf3cc36ca2609aa38e71046265a1f1e37-run-${{ github.run_id }}-${{ github.run_attempt }}-${{ github.sha }}";
const CACHE_KEY: &str = "velnor-qualification-mbx-1.21.1-share-out-dir-disabled-v1-action-d0825fbaf3cc36ca2609aa38e71046265a1f1e37-${{ runner.os }}-${{ runner.arch }}-rust-1.98.1-run-${{ github.run_id }}-${{ github.run_attempt }}-${{ github.sha }}";

fn assert_cache_identity(job: &str) {
    assert!(job.contains(CACHE_GENERATION), "{job}");
    assert!(job.contains(&format!("cache-key: {CACHE_KEY}")), "{job}");
    assert!(job.contains("isolate-objects-cache: \"true\""), "{job}");
    assert!(job.contains("version: 1.21.1"), "{job}");
    assert!(!job.contains("cache-key-suffix"), "{job}");
    assert!(job.contains("RUSTUP_TOOLCHAIN: 1.98.1"), "{job}");
    assert!(
        job.contains("CARGO_HOME: ${{ github.workspace }}/.velnor-mbx-cache-qualification/cargo"),
        "{job}"
    );
}

fn assert_writer(writer: &str) {
    assert!(
        writer.contains("inputs.mode == 'mbx-cache-roundtrip' && github.ref == 'refs/heads/main' && github.ref_protected == true"),
        "{writer}"
    );
    assert!(writer.contains("actions: write"), "{writer}");
    assert!(writer.contains("ACTIONS_CACHE_MODE: write"), "{writer}");
    assert!(writer.contains("MBX_GC_AUTO: \"0\""), "{writer}");
    assert!(writer.contains("MBX_SHARE_OUT_DIR: \"0\""), "{writer}");
    assert!(
        writer.contains("save-on-workflow-dispatch: \"true\""),
        "{writer}"
    );
    assert!(
        writer.contains("test \\\"$CACHE_HIT\\\" = 'false'"),
        "{writer}"
    );
    assert!(writer.contains("rustc --print sysroot"), "{writer}");
    assert_cache_identity(writer);
}

fn assert_reader(reader: &str) {
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
    assert!(reader.contains("MBX_GC_AUTO: \"0\""), "{reader}");
    assert!(reader.contains("MBX_SHARE_OUT_DIR: \"0\""), "{reader}");
    assert!(
        reader.contains("steps.mbx_cache.outputs.cache-hit"),
        "{reader}"
    );
    assert!(
        reader.contains("test \\\"$CACHE_HIT\\\" = 'true'"),
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
    assert_cache_identity(reader);
}

#[test]
fn protected_main_mbx_roundtrip_is_run_bound_and_read_only_on_restore() -> TestResult {
    let repo = make_repo(&crate::impl_schema2_routing::workflow_config())?;
    let tree = render_staged_tree(&prepare(repo.path())?)?;
    let qualification =
        crate::impl_schema2_routing::required_file(&tree, ".github/workflows/qualification.yml")?;
    let writer = crate::impl_schema2_routing::job_body(qualification, "mbx-cache-write-hosted")?;
    assert_writer(writer);
    let reader = crate::impl_schema2_routing::job_body(qualification, "mbx-cache-read-hosted")?;
    assert_reader(reader);
    Ok(())
}
