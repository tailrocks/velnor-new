//! Hosted MBX resource evidence and corrupt-import fallback stay bound to the writer.

use std::collections::BTreeSet;

use velnor_actions_contract::RoutingWorkflow;
use velnor_actions_workflow_renderer::schema2::QUALIFICATION_WORKFLOW;
use velnor_actions_workflow_renderer::{
    MbxQualificationPins, RenderError, Schema2WorkflowRequest, render_schema2_workflows,
};

use super::impl_renderer_fixtures::mise;

fn qualification() -> Result<String, RenderError> {
    let request = Schema2WorkflowRequest {
        version: "0.1.1".to_owned(),
        hosted_label: "ubuntu-26.04".to_owned(),
        scale_set: Schema2WorkflowRequest::canonical_scale_set()?,
        workflows: BTreeSet::from([RoutingWorkflow::Qualification]),
        mbx_qualification: Some(MbxQualificationPins {
            mise_setup: mise(),
            mbx_action_uses: format!("jdx/mr-boxington-action@{}", "a".repeat(40)),
            mbx_version: "1.22.0".to_owned(),
            rust_version: "1.98.1".to_owned(),
        }),
    };
    let files = render_schema2_workflows(&request)?;
    files
        .into_iter()
        .find(|file| file.path == QUALIFICATION_WORKFLOW)
        .map(|file| file.bytes)
        .ok_or_else(|| RenderError::InvalidWorkflow("missing_qualification_workflow".to_owned()))
}

fn job_block<'a>(text: &'a str, id: &str, next_id: Option<&str>) -> Result<&'a str, RenderError> {
    let Some(start) = text.find(&format!("  {id}:")) else {
        return Err(RenderError::InvalidWorkflow(format!(
            "missing_qualification_job:{id}"
        )));
    };
    let end = next_id.and_then(|next| text[start..].find(&format!("  {next}:")));
    Ok(&text[start..start + end.unwrap_or(text.len() - start)])
}

fn assert_provisional_receipt_and_role_rules() {
    let stop_script = include_str!("../src/schema2_mbx_resource_stop.sh");
    let receipt_script = include_str!("../src/schema2_mbx_resource_receipt.sh");
    let sampler_script = include_str!("../src/schema2_mbx_resource_sampler.sh");
    let path_script = include_str!("../src/schema2_mbx_resource_path.sh");
    assert!(receipt_script.contains("receipt_status:\"provisional\""));
    assert!(receipt_script.contains("restore_miss_candidate:"));
    assert!(stop_script.contains(".restore_miss_candidate == true"));
    assert!(stop_script.contains(".restore_primary_key == .primary_key"));
    assert!(stop_script.contains(".cache_hit == \"\""));
    assert!(!stop_script.contains(".cache_hit == \"false\""));
    assert!(path_script.contains("writer|seed|new-key-writer) printf 'cold\\n'"));
    assert!(path_script.contains("reader|reader-a|reader-b|corrupt-reader) printf 'hit\\n'"));
    assert!(path_script.contains("*) return 1 ;;"));
    assert!(path_script.contains("< /proc/uptime"));
    assert!(path_script.contains("timeout --signal=KILL"));
    assert!(path_script.contains("RESOURCE_SESSION_SCAN_LIMIT=4096"));
    assert!(path_script.contains("RESOURCE_DEADLINE_COMMAND_EXPIRED=1"));
    assert!(path_script.contains("resource_deadline_capture_failed \"$?\""));
    assert!(path_script.contains("resource_deadline_capture_failed \"$?\"; return 1; }"));
    assert!(!path_script.contains("$(owned_session_member_count"));
    assert!(stop_script.contains("shutdown_budget_seconds"));
    assert!(stop_script.contains("shutdown_elapsed_centiseconds"));
    assert!(stop_script.contains("shutdown_deadline_status"));
    assert!(stop_script.contains("[ \"${RESOURCE_SHUTDOWN_STATUS:-}\" = within_budget ]"));
    assert!(sampler_script.contains("$(resource_role_class)"));
    assert!(stop_script.contains("case \"$(resource_role_class)\" in"));
    assert!(!sampler_script.contains("reader|reader-a|reader-b|corrupt-reader"));
    assert!(!stop_script.contains("writer|seed|new-key-writer"));
}

#[test]
fn hosted_resource_receipts_bind_all_qualification_roles() -> Result<(), RenderError> {
    let text = qualification()?;
    let writer = job_block(
        &text,
        "mbx-cache-write-hosted",
        Some("mbx-cache-read-hosted"),
    )?;
    let reader = job_block(
        &text,
        "mbx-cache-read-hosted",
        Some("mbx-cache-corrupt-import-hosted"),
    )?;
    let corrupt = job_block(&text, "mbx-cache-corrupt-import-hosted", None)?;

    for block in [writer, reader, corrupt] {
        assert!(block.contains("github.ref_protected == true"));
        assert!(block.contains("MBX_QUALIFICATION_PHASE_FILE:"));
        assert!(block.contains("MBX_QUALIFICATION_IMPORT_RECEIPT:"));
        assert!(block.contains("name: Start bounded MBX resource sampler"));
        assert!(block.contains("name: Stop sampler and capture final MBX state"));
        assert!(block.contains("name: Record MBX cache identity and save outcome"));
        assert!(block.contains("name: Upload MBX cache evidence"));
        assert!(block.contains("retention-days: \"30\""));
        assert!(block.contains("if: always()"));
        assert!(!block.contains("restore-keys:"));
        assert!(block.contains("mbx-cache-evidence-mbx-cache-"));
        assert!(block.contains("-r${{ github.run_id }}-a${{ github.run_attempt }}"));
        let stop_start = block
            .find("name: Stop sampler and capture final MBX state")
            .expect("stop step");
        let stop_end = block[stop_start..]
            .find("name: Upload MBX cache evidence")
            .map(|offset| stop_start + offset)
            .expect("upload step follows stop");
        let stop = &block[stop_start..stop_end];
        for (key, expression) in [
            (
                "MBX_QUALIFICATION_CACHE_PRIMARY:",
                "${{ steps.mbx-bundle-key.outputs.primary }}",
            ),
            (
                "MBX_QUALIFICATION_CACHE_PREFIX:",
                "${{ steps.mbx-bundle-key.outputs.prefix }}",
            ),
            (
                "MBX_QUALIFICATION_CACHE_GENERATION:",
                "${{ steps.mbx-bundle-key.outputs.generation }}",
            ),
            (
                "MBX_QUALIFICATION_RUSTC_IDENTITY:",
                "${{ steps.mbx-bundle-key.outputs.rustc_identity }}",
            ),
            (
                "MBX_QUALIFICATION_RESTORE_PRIMARY_KEY:",
                "${{ steps.mbx-bundle.outputs.cache-primary-key }}",
            ),
            (
                "MBX_QUALIFICATION_RESTORE_CONCLUSION:",
                "${{ steps.mbx-bundle.conclusion }}",
            ),
        ] {
            assert!(stop.contains(key), "stop env missing {key}");
            assert!(
                stop.contains(expression),
                "stop env binding missing {expression}"
            );
        }
    }
    assert!(writer.contains("actions: write"));
    assert!(reader.contains("actions: read"));
    assert!(corrupt.contains("actions: read"));
    assert!(writer.contains("MBX_QUALIFICATION_ROLE: writer"));
    assert!(reader.contains("MBX_QUALIFICATION_ROLE: reader"));
    assert!(corrupt.contains("MBX_QUALIFICATION_ROLE: corrupt-reader"));
    assert!(writer.contains("export-complete"));
    assert!(writer.contains("gc-complete"));
    assert_provisional_receipt_and_role_rules();
    Ok(())
}

#[test]
fn corrupt_reader_mutates_after_exact_restore_then_proves_cold_fallback() -> Result<(), RenderError>
{
    let text = qualification()?;
    let corrupt = job_block(&text, "mbx-cache-corrupt-import-hosted", None)?;
    let restore = corrupt
        .find("name: Restore MBX single bundle")
        .expect("restore");
    let mutate = corrupt
        .find("name: Corrupt restored MBX bundle payload")
        .expect("mutation");
    let import = corrupt
        .find("name: Import MBX single bundle")
        .expect("import");
    let verify = corrupt
        .find("name: Verify corrupt import selected a cold MBX store")
        .expect("fallback verification");
    let compile = corrupt
        .find("name: Compile MBX cache probe")
        .expect("cold compile");
    assert!(restore < mutate && mutate < import && import < verify && verify < compile);
    assert!(corrupt.contains("exit_status="));
    assert!(corrupt.contains("fallback-cache-root.txt"));
    assert!(corrupt.contains(".objects == 0"));
    assert!(corrupt.contains(".savings.cached_compilations == 0"));
    assert!(corrupt.contains("mbx-cache-evidence-mbx-cache-corrupt-import-hosted"));
    Ok(())
}
