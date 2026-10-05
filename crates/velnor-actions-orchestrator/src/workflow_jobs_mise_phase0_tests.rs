use super::*;
use velnor_actions_contract::StepKind;

#[test]
fn jobs_cover_only_platforms_with_official_phase0_assets() {
    let jobs = jobs(&ToolCatalog::pinned()).expect("phase0 jobs");
    assert_eq!(jobs.len(), JOB_IDS.len());
    assert_eq!(jobs[0].0, velnor_actions_contract::MISE_PHASE0_LINUX_JOB_ID);
    assert_eq!(jobs[0].1.runs_on, "ubuntu-26.04");
    assert_eq!(
        jobs[1].0,
        velnor_actions_contract::MISE_PHASE0_MACOS_ARM64_JOB_ID
    );
    assert_eq!(jobs[1].1.runs_on, "macos-15");
    assert_eq!(
        jobs[2].0,
        velnor_actions_contract::MISE_PHASE0_MACOS_INTEL_JOB_ID
    );
    assert_eq!(jobs[2].1.runs_on, "macos-15-intel");
    for (_, job) in jobs {
        assert_eq!(job.steps[0].name, "Checkout");
        assert_eq!(job.steps[1].name, PREPARE_PINNED_TOOLS_STEP);
        assert_eq!(
            job.steps[2].name,
            "Run official-binary Mise Phase0 qualification"
        );
        let StepKind::Shell { run, .. } = &job.steps[2].kind else {
            panic!("Phase0 runner must execute the checked-in qualification task");
        };
        assert!(
            run.windows(2)
                .any(|args| args[0] == "bash" && args[1] == ".mise/tasks/mise-phase0")
        );
    }
}

#[test]
fn mbx_is_installed_only_on_platforms_with_official_binaries() {
    let jobs = jobs(&ToolCatalog::pinned()).expect("phase0 jobs");
    for (id, job) in jobs {
        let StepKind::Shell { run, .. } = &job.steps[1].kind else {
            panic!("tool preparation must be a shell step");
        };
        let mbx_is_supported = id == velnor_actions_contract::MISE_PHASE0_LINUX_JOB_ID
            || id == velnor_actions_contract::MISE_PHASE0_MACOS_ARM64_JOB_ID;
        assert_eq!(
            run.iter().any(|arg| arg == "mr-boxington@1.21.1"),
            mbx_is_supported,
            "unexpected MBX installation policy for {id}"
        );
        assert!(
            run.iter()
                .any(|arg| arg == "aqua:nextest-rs/nextest/cargo-nextest@0.9.146")
        );
    }
}
