use super::*;

/// Workspace inputs with fixed digests for key tests.
fn workspace() -> WorkspaceInputs {
    WorkspaceInputs {
        repository_id: digest_b3(b"repo"),
        stack_id: "rust".to_owned(),
        project_root: "crates/demo".to_owned(),
        inventory_digest: digest_b3(b"inventory"),
    }
}

#[test]
fn keys_are_semantic_and_bounded() {
    let workspace = workspace_id(&workspace()).expect("workspace");
    let lane = lane_id(&LaneInputs {
        workspace_id: workspace,
        component_id: "pkg".to_owned(),
        task_kind: "clippy".to_owned(),
        configuration: "default".to_owned(),
        writer_lane: "lane-0".to_owned(),
    })
    .expect("lane");
    assert!(lane.starts_with("b3-"));
    let compat = digest_b3(b"compat");
    let snapshot = digest_b3(b"snapshot");
    let key = cache_key("task", "trusted", &compat, &snapshot).expect("key");
    assert!(key.len() <= MAX_CACHE_KEY_BYTES);
    assert!(key.starts_with("velnor-v1-task-trusted-"));
    assert!(
        restore_prefix("task", "trusted", &compat)
            .expect("prefix")
            .ends_with('-')
    );
    assert!(cache_key("mbx", "pr", "nope", &snapshot).is_err());
    assert!(
        lane_id(&LaneInputs {
            workspace_id: digest_b3(b"w"),
            component_id: "/abs/path".to_owned(),
            task_kind: "clippy".to_owned(),
            configuration: "default".to_owned(),
            writer_lane: "lane-0".to_owned(),
        })
        .is_err()
    );
    assert!(
        platform_id(&PlatformInputs {
            os: "linux".to_owned(),
            arch: "x86_64".to_owned(),
            runs_on: "ubuntu-26.04".to_owned(),
            image_os: "ubuntu26".to_owned(),
            image_version: "20260928.1.0".to_owned(),
            target: "host".to_owned(),
        })
        .is_ok()
    );
    assert!(
        cache_format_id(&FormatInputs {
            adapter: "mbx".to_owned(),
            format: String::new(),
            generation: "7".to_owned(),
        })
        .is_err()
    );
    assert!(validate_miss_reason("no_entry").is_ok());
    assert!(validate_miss_reason("sometimes").is_err());
}
