use super::*;

#[test]
fn no_fixed_toolchain_without_a_trusted_dir_is_refused_before_any_fetch() -> TestResult {
    let fixture = Fixture::new("fetch-fixed-alone", 0)?;
    let api = serve(204)?;
    let root = fixture.repo.root();
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .current_dir(root)
        .args([
            "fetch",
            "--no-fixed-toolchain",
            "--manifest-path",
            "Cargo.toml",
        ])
        .arg("--home")
        .arg(root.join("home"))
        .env_clear()
        .env(
            "PATH",
            format!("{}:/usr/bin:/bin", root.join("trusted/bin").display()),
        )
        .env("VELNOR_API_URL", &api.url)
        .env("VELNOR_PRIVATE_TOKEN", TOKEN)
        .output()?;
    assert!(!output.status.success(), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("required arguments were not provided") && stderr.contains("--trusted-dir"),
        "{stderr}"
    );
    assert_eq!(fixture.log("cargo.args"), "");
    assert!(api.requests.lock().map_err(|e| e.to_string())?.is_empty());
    Ok(())
}

#[test]
fn no_fixed_toolchain_with_a_trusted_dir_still_fetches() -> TestResult {
    let fixture = Fixture::new("fetch-fixed-with-dir", 0)?;
    let api = serve(204)?;
    let output = fixture.run(&api, Some(TOKEN), None);
    assert!(output.status.success(), "{output:?}");
    assert_ne!(fixture.log("cargo.args"), "");
    Ok(())
}
