//! The real prepare/render pipeline includes the registered cancellation workflow.

use velnor_actions_orchestrator::{prepare, render_staged_tree};

use crate::impl_common::{TestResult, make_repo};
use crate::impl_schema2_routing::{job_body, required_file, workflow_config};

#[test]
fn prepared_qualification_workflow_contains_all_trusted_cancellation_roles() -> TestResult {
    let repo = make_repo(&workflow_config())?;
    let prep = prepare(repo.path())?;
    let tree = render_staged_tree(&prep)?;
    let workflow = required_file(&tree, ".github/workflows/qualification.yml")?;

    assert!(workflow.contains("workflow_dispatch:"), "{workflow}");
    assert!(workflow.contains("run-name:"), "{workflow}");
    assert!(workflow.contains("inputs.probe_id"), "{workflow}");
    for (phase, token) in [("pre-save", "pre-save"), ("during-save", "during-save")] {
        for (role, mode_role, permission, always) in [
            ("victim", "victim", "actions: write", false),
            ("controller", "controller", "actions: write", false),
            ("observer", "controller", "actions: read", true),
        ] {
            let id = format!("mbx-cancel-{phase}-{role}");
            let block = job_body(workflow, &id)?;
            let mode = format!("mbx-cancel-{token}-{mode_role}");
            let gate = format!(
                "inputs.mode == '{mode}' && github.event_name == 'workflow_dispatch' && github.ref == 'refs/heads/main' && github.ref_protected == true"
            );
            assert!(block.contains(&gate), "{id}: {block}");
            if always {
                assert!(
                    block.contains(&format!("always() && ({gate})")),
                    "{id}: {block}"
                );
            }
            for expected in [
                permission,
                "contents: none",
                "pull-requests: none",
                "id-token: none",
            ] {
                assert!(block.contains(expected), "{id} missing {expected}: {block}");
            }
            assert!(!block.contains("ACTIONS_ID_TOKEN_REQUEST_TOKEN"), "{block}");
            assert!(!block.contains("secrets."), "{block}");
        }

        let controller = job_body(workflow, &format!("mbx-cancel-{phase}-controller"))?;
        let observer = job_body(workflow, &format!("mbx-cancel-{phase}-observer"))?;
        assert!(controller.contains(".run_attempt == 1"), "{controller}");
        assert!(controller.contains(&format!("name: mbx-cancel-controller-receipt-{phase}")));
        assert!(observer.contains(&format!("name: mbx-cancel-observer-{phase}")));
        assert!(observer.contains("child_run_id"), "{observer}");
        assert!(observer.contains("child_attempt"), "{observer}");
        assert!(observer.contains(".run_attempt == 1"), "{observer}");
        for step in [
            "name: Prepare MBX bundle key",
            "name: Restore MBX single bundle",
            "name: Import MBX single bundle",
        ] {
            assert!(
                observer.contains(step),
                "missing R13 observer step {step}: {observer}"
            );
        }
    }
    Ok(())
}
