use std::os::unix::fs::symlink;

use super::*;

#[test]
fn rejects_symlink_workspace_before_reserving_any_path() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let linked_workspace = fixture.root.0.join("workspace alias");
    symlink(&fixture.workspace, &linked_workspace)?;
    let [preflight, _action] = mbx_tool_steps(
        "jdx/mr-boxington-action@aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        TEST_MBX_VERSION,
        TEST_RUST_TOOLCHAIN,
    )?;
    let StepKind::Shell { run, .. } = preflight.kind else {
        return Err(std::io::Error::other("preflight was not a shell step").into());
    };
    let script = run
        .get(2)
        .ok_or_else(|| std::io::Error::other("preflight script missing"))?;
    let (success, paths, _) =
        fixture.run_at(script, super::cases::exact_probe_case(), &linked_workspace)?;
    assert!(!success, "symlinked workspace must fail closed");
    assert!(paths.is_empty(), "invalid workspace must not alter PATH");
    assert!(fs::read_to_string(&fixture.github_output)?.is_empty());
    assert!(fs::read_to_string(&fixture.github_env)?.is_empty());
    assert!(!fixture.root.0.join("velnor").exists());
    assert!(!linked_workspace.join(".velnor-mbx-cache.lock").exists());
    assert!(!fixture.workspace.join(".velnor-mbx-cache").exists());
    Ok(())
}

#[test]
fn existing_workspace_lease_blocks_before_transport_mutation()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let lease = fixture.workspace.join(".velnor-mbx-cache.lock");
    fs::create_dir(&lease)?;
    let [preflight, _action] = mbx_tool_steps(
        "jdx/mr-boxington-action@aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        TEST_MBX_VERSION,
        TEST_RUST_TOOLCHAIN,
    )?;
    let StepKind::Shell { run, .. } = preflight.kind else {
        return Err(std::io::Error::other("preflight was not a shell step").into());
    };
    let script = run
        .get(2)
        .ok_or_else(|| std::io::Error::other("preflight script missing"))?;

    let (success, paths, _) =
        fixture.run_at(script, super::cases::exact_probe_case(), &fixture.workspace)?;

    assert!(!success, "an active workspace lease must block another job");
    assert!(paths.is_empty(), "a rejected job must not update PATH");
    assert!(fs::read_to_string(&fixture.github_output)?.is_empty());
    assert!(fs::read_to_string(&fixture.github_env)?.is_empty());
    assert!(
        lease.is_dir(),
        "the existing lease remains owned by its holder"
    );
    assert!(!fixture.workspace.join(".velnor-mbx-cache").exists());
    assert!(
        !fixture
            .root
            .0
            .join("velnor/mbx-job-741852963-12001-1-mbx_lifecycle")
            .exists(),
        "rejected preflight releases its private per-job root"
    );
    Ok(())
}
