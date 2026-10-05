//! MBX producer environment and runner-policy tests.

use std::process::Command;

use super::*;

fn obligation() -> CrateObligation {
    CrateObligation {
        task_id: "stack/rust/demo/clippy/default".to_owned(),
        kind: "clippy".to_owned(),
        step_name: "Clippy".to_owned(),
        gated_by: Vec::new(),
        matrix_key: "m-0123456789abcdef".to_owned(),
        task_digest: format!("b3-{}", "a".repeat(64)),
        run: vec!["true".to_owned()],
    }
}

#[test]
fn mbx_driver_passes_only_the_preflighted_store_and_group_to_rust_producers() {
    let step = obligation_step(
        &obligation(),
        &ToolCatalog::pinned(),
        &[],
        CompileDriver::Mbx,
        None,
        "ubuntu-24.04",
    )
    .expect("MBX step");
    let velnor_actions_contract::StepKind::Shell { env, .. } = &step.kind else {
        panic!("obligation must be a shell step");
    };
    assert_eq!(
        env.get(MBX_CACHE_DIR_ENV).map(String::as_str),
        Some(MBX_CACHE_DIR_OUTPUT)
    );
    assert_eq!(
        env.get(MBX_CACHE_EXPORT_GROUP_ENV).map(String::as_str),
        Some(MBX_CACHE_EXPORT_GROUP_OUTPUT)
    );
    assert_eq!(env.get(MBX_GC_AUTO_ENV).map(String::as_str), Some("0"));
    assert_eq!(
        env.get(MBX_SHARE_OUT_DIR_ENV).map(String::as_str),
        Some("0")
    );
    let emitted_mbx_keys: Vec<_> = env
        .keys()
        .filter(|key| key.starts_with("MBX_"))
        .map(String::as_str)
        .collect();
    assert_eq!(
        emitted_mbx_keys,
        [
            MBX_CACHE_DIR_ENV,
            MBX_CACHE_EXPORT_GROUP_ENV,
            MBX_GC_AUTO_ENV,
            MBX_SHARE_OUT_DIR_ENV,
        ],
        "no unapproved MBX variable is emitted"
    );
    let extra = BTreeMap::from([(MBX_CACHE_DIR_ENV.to_owned(), "${{ github.sha }}".to_owned())]);
    assert!(
        task_step_env(&ToolCatalog::pinned(), &extra, true)
            .expect_err("unvalidated cache paths are rejected")
            .to_string()
            .contains("unvalidated_mbx_cache_env")
    );
    for (key, value) in [
        (MBX_CACHE_EXPORT_GROUP_ENV, "untrusted-group"),
        (MBX_GC_AUTO_ENV, "1"),
        (MBX_SHARE_OUT_DIR_ENV, "1"),
    ] {
        let extra = BTreeMap::from([(key.to_owned(), value.to_owned())]);
        assert!(
            task_step_env(&ToolCatalog::pinned(), &extra, true)
                .expect_err("MBX policy values are fixed")
                .to_string()
                .contains(&format!("unvalidated_mbx_cache_env:{key}"))
        );
    }
}

#[test]
fn scale_set_mbx_producer_keeps_gc_default_and_disables_shared_out_dir() {
    let scale_set = "scale-set:velnor+ubuntu-26.04-scale-set";
    let step = obligation_step(
        &obligation(),
        &ToolCatalog::pinned(),
        &[],
        CompileDriver::Mbx,
        None,
        scale_set,
    )
    .expect("Scale Set MBX step");
    let velnor_actions_contract::StepKind::Shell { env, .. } = &step.kind else {
        panic!("obligation must be a shell step");
    };
    assert_eq!(
        env.get(MBX_CACHE_DIR_ENV).map(String::as_str),
        Some(MBX_CACHE_DIR_OUTPUT)
    );
    assert_eq!(
        env.get(MBX_CACHE_EXPORT_GROUP_ENV).map(String::as_str),
        Some(MBX_CACHE_EXPORT_GROUP_OUTPUT)
    );
    assert_eq!(
        env.get(MBX_SHARE_OUT_DIR_ENV).map(String::as_str),
        Some("0")
    );
    assert!(
        !env.contains_key(MBX_GC_AUTO_ENV),
        "Scale Set jobs retain MBX's normal GC policy"
    );
}

#[test]
fn mbx_producer_env_reaches_an_inheriting_child_process() {
    let producer_script = "test \"$MBX_CACHE_DIR\" = /tmp/velnor/mbx/store && test \"$MBX_CACHE_EXPORT_GROUP\" = repo-42-run-7-attempt-2-clippy && test \"$MBX_GC_AUTO\" = 0 && test \"$MBX_SHARE_OUT_DIR\" = 0";
    let mut task = obligation();
    task.run = vec![
        "/bin/sh".to_owned(),
        "-c".to_owned(),
        producer_script.to_owned(),
    ];
    let step = obligation_step(
        &task,
        &ToolCatalog::pinned(),
        &[],
        CompileDriver::Mbx,
        None,
        "ubuntu-24.04",
    )
    .expect("MBX step");
    let velnor_actions_contract::StepKind::Shell { env, run } = &step.kind else {
        panic!("obligation must be a shell step");
    };
    let mut run = run.clone();
    let helper = helper_path_for_version();
    for arg in &mut run {
        *arg = arg.replace(&helper, "/bin/true");
    }
    let mut command = Command::new(&run[0]);
    command.args(&run[1..]);
    for (key, value) in env {
        let value = match key.as_str() {
            MBX_CACHE_DIR_ENV => "/tmp/velnor/mbx/store",
            MBX_CACHE_EXPORT_GROUP_ENV => "repo-42-run-7-attempt-2-clippy",
            _ => value.as_str(),
        };
        command.env(key, value);
    }
    let output = command.output().expect("run isolated producer fixture");
    assert!(
        output.status.success(),
        "actual inheriting child receives the validated producer env: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
