//! Security assertions for the protected-main MBX cache round trip.

use velnor_actions_actionlint::actions::{
    MR_BOXINGTON_ACTION_CANDIDATE_SHA, MR_BOXINGTON_ACTION_SHA,
};
use velnor_actions_orchestrator::{prepare, render_staged_tree};

use crate::impl_common::{TestResult, make_repo};

fn assert_candidate_ref(writer: &str) {
    assert!(
        writer.contains(&format!(
            "uses: jdx/mr-boxington-action@{MR_BOXINGTON_ACTION_CANDIDATE_SHA}"
        )),
        "the experiment must invoke its exact immutable candidate"
    );
    assert!(
        !writer.contains(&format!(
            "uses: jdx/mr-boxington-action@{MR_BOXINGTON_ACTION_SHA}"
        )),
        "qualification must not silently substitute the production pin"
    );
}

fn assert_candidate_owner(job: &str) {
    for fragment in [
        "version: 1.22.0",
        "toolchain: 1.98.1",
        "isolate-objects-cache: \"true\"",
        "RUSTUP_TOOLCHAIN: 1.98.1",
        "MBX_SHARE_OUT_DIR: \"0\"",
        "CARGO_HOME: ${{ github.workspace }}/.velnor-mbx-cache-qualification/cargo",
    ] {
        assert!(job.contains(fragment), "{job}");
    }
}

#[test]
fn protected_main_mbx_roundtrip_is_run_bound_and_read_only_on_restore() -> TestResult {
    let repo = make_repo(&crate::impl_schema2_routing::workflow_config())?;
    let tree = render_staged_tree(&prepare(repo.path())?)?;
    let qualification =
        crate::impl_schema2_routing::required_file(&tree, ".github/workflows/qualification.yml")?;

    let writer = crate::impl_schema2_routing::job_body(qualification, "mbx-cache-write-hosted")?;
    assert_candidate_ref(writer);
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
        writer.contains("velnor-qualification-mbx-1.22.0-share-out-dir-disabled-v1-action-d0825fbaf3cc36ca2609aa38e71046265a1f1e37-run-${{ github.run_id }}-${{ github.run_attempt }}-${{ github.sha }}"),
        "{writer}"
    );
    assert!(writer.contains("version: 1.22.0"), "{writer}");
    assert!(writer.contains("RUSTUP_TOOLCHAIN: 1.98.1"), "{writer}");
    assert!(
        writer
            .contains("CARGO_HOME: ${{ github.workspace }}/.velnor-mbx-cache-qualification/cargo"),
        "{writer}"
    );
    assert_candidate_owner(writer);
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
        reader.contains("velnor-qualification-mbx-1.22.0-share-out-dir-disabled-v1-action-d0825fbaf3cc36ca2609aa38e71046265a1f1e37-run-${{ github.run_id }}-${{ github.run_attempt }}-${{ github.sha }}"),
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
    assert!(reader.contains("version: 1.22.0"), "{reader}");
    assert!(reader.contains("RUSTUP_TOOLCHAIN: 1.98.1"), "{reader}");
    assert!(
        reader
            .contains("CARGO_HOME: ${{ github.workspace }}/.velnor-mbx-cache-qualification/cargo"),
        "{reader}"
    );
    assert_candidate_owner(reader);
    Ok(())
}
