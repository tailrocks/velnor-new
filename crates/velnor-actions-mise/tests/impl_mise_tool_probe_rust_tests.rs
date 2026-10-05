//! Rust compiler and Cargo Nextest probe fixtures.

use super::*;

#[test]
fn rustc_verbose_requires_exact_declared_release_commit_and_platform() -> TestResult {
    let fixture = Fixture::new()?;
    let platform = platform()?;
    let expected = format!(
        "rustc 1.97.1 (qualified)\nbinary: rustc\ncommit-hash: {}\nhost: {}\nrelease: 1.97.1",
        "a".repeat(40),
        platform.target()
    );
    let tool = tool(
        "rustc",
        "1.97.1",
        QualifiedToolProbe::RustcVerbose {
            expected: expected.clone(),
        },
        platform,
    );
    let observed = fixture.observation(
        "rustc",
        &format!("test \"$1\" = -vV; test \"$RUSTUP_TOOLCHAIN\" = \"$HOME/compiler\"; printf '%s\\n' '{expected}'"),
    )?;
    let proof = verify_qualified_executable(
        &tool,
        platform,
        &tool.platforms[0].executables[0],
        &observed,
        &fixture.homes,
        test_deadline()?,
    )?;
    validate_executable_proofs(&tool, platform, &[proof])?;
    let mut homes = fixture.homes.clone();
    homes.compiler_toolchain = Some(fixture.root.join("future-rust-prefix"));
    assert!(
        verify_qualified_executable(
            &tool,
            platform,
            &tool.platforms[0].executables[0],
            &observed,
            &homes,
            test_deadline()?
        )
        .is_err()
    );
    homes.compiler_toolchain = None;
    assert!(
        verify_qualified_executable(
            &tool,
            platform,
            &tool.platforms[0].executables[0],
            &observed,
            &homes,
            test_deadline()?
        )
        .is_err()
    );
    let bad = expected.replace("release: 1.97.1", "release: 1.98.1");
    let drift = fixture.observation("rustc", &format!("printf '%s\\n' '{bad}'"))?;
    assert!(
        verify_qualified_executable(
            &tool,
            platform,
            &tool.platforms[0].executables[0],
            &drift,
            &fixture.homes,
            test_deadline()?
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn nextest_version_requires_its_owned_compiler_prefix() -> TestResult {
    let fixture = Fixture::new()?;
    let platform = platform()?;
    let tool = tool(
        "cargo-nextest",
        "0.9.140",
        QualifiedToolProbe::CargoNextestVersion {
            expected: "cargo-nextest 0.9.140".into(),
        },
        platform,
    );
    let observed = fixture.observation(
        "cargo-nextest",
        "test \"$1\" = --version; printf 'cargo-nextest 0.9.140\\n'",
    )?;
    verify_qualified_executable(
        &tool,
        platform,
        &tool.platforms[0].executables[0],
        &observed,
        &fixture.homes,
        test_deadline()?,
    )?;
    let mut homes = fixture.homes.clone();
    homes.compiler_toolchain = None;
    assert!(
        verify_qualified_executable(
            &tool,
            platform,
            &tool.platforms[0].executables[0],
            &observed,
            &homes,
            test_deadline()?
        )
        .is_err()
    );
    assert!(validate_executable_proofs(&tool, platform, &[]).is_err());
    Ok(())
}
