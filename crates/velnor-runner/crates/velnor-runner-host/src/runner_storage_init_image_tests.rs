#[test]
fn runner_image_contains_source_bound_storage_initializer() {
    let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("the Velnor workspace exists");
    let manifest =
        std::fs::read_to_string(workspace.join("crates/velnor-resource-probe/Cargo.toml"))
            .expect("the resource utility manifest exists");
    let dockerfile = super::RUNNER_DOCKERFILE;
    let build = super::RESOURCE_PROBE_BUILD;

    assert!(manifest.contains("name = \"velnor-storage-init\""));
    assert!(build.contains("git rev-parse HEAD"));
    assert!(build.contains("--bin velnor-storage-init"));
    assert!(build.contains("--target x86_64-unknown-linux-musl"));
    assert!(build.contains("storage_init=$runner_context"));
    assert!(build.contains("storage_init_sha256"));
    assert!(build.contains("/velnor/storage-init"));
    assert!(build.contains("storage_init_mode\" == 555"));
    assert!(dockerfile.contains("COPY --from=storage_init storage-init /velnor/storage-init"));
    assert!(dockerfile.contains("chmod 0555 /velnor/resource-probe /velnor/storage-init"));
}
