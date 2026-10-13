#[test]
fn prepare_lockless_with_deps_writes_nothing_merged() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\nserde = \"1\"\nvelnor-poison-probe-nonexistent = \"1\"\n",
    )?;
    let before = snapshot(root)?;
    let prep = prepare(root)?;
    assert_eq!(before, snapshot(root)?, "prepare must not write");
    assert!(!root.join("Cargo.lock").exists(), "no lockfile synthesized");
    assert!(!prep.discovery.workspaces.is_empty(), "demo detected");
    let argv = velnor_actions_mise::MetadataDiscovery::new(root.join("Cargo.toml"))?
        .mbx_argv(&velnor_actions_mise::ToolCatalog::pinned());
    let skips = argv.iter().any(|arg| arg == "--no-deps");
    assert!(skips, "discovery skips resolution");
    let probe = std::process::Command::new("cargo")
        .args([
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--manifest-path",
        ])
        .arg(root.join("Cargo.toml"))
        .env("CARGO_HTTP_PROXY", "http://127.0.0.1:9/")
        .env("CARGO_HTTPS_PROXY", "http://127.0.0.1:9/")
        .output()?;
    assert!(probe.status.success(), "no-deps never fetches");
    assert!(probe.stderr.is_empty(), "no index chatter");
    Ok(())
}
