use super::*;

#[test]
fn without_a_token_the_fetch_is_offline_and_a_miss_says_why() -> TestResult {
    let fixture = Fixture::new("fetch-offline", 101)?;
    let api = serve(204)?;
    let output = fixture.run(&api, None, None);
    assert!(!output.status.success());
    assert_eq!(
        fixture.log("cargo.args").trim(),
        "fetch --locked --manifest-path Cargo.toml --offline"
    );
    assert!(!fixture.log("cargo.env").contains("GIT_CONFIG_KEY_0=url."));
    assert!(!fixture.log("cargo.env").contains("GIT_ALLOW_PROTOCOL"));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("fork"), "{stderr}");
    assert!(api.requests.lock().map_err(|e| e.to_string())?.is_empty());
    Ok(())
}

#[test]
fn a_cargo_home_outside_the_tool_directories_runs_nothing_and_still_revokes() -> TestResult {
    let fixture = Fixture::new("fetch-home", 0)?;
    let api = serve(204)?;
    let hostile = fixture.repo.root().join("checkout-cargo-home");
    let output = fixture.run(&api, Some(TOKEN), Some(&hostile.display().to_string()));
    assert!(!output.status.success());
    assert_eq!(fixture.log("cargo.args"), "");
    assert_eq!(fixture.log("hostile.log"), "");
    assert_eq!(api.requests.lock().map_err(|e| e.to_string())?.len(), 1);
    Ok(())
}

#[test]
fn a_token_that_cannot_be_revoked_fails_the_step() -> TestResult {
    let fixture = Fixture::new("fetch-revoke", 0)?;
    let api = serve(401)?;
    let output = fixture.run(&api, Some(TOKEN), None);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("revoked"));
    Ok(())
}

#[test]
fn every_github_url_form_a_lock_file_records_is_authenticated_and_nothing_else() -> TestResult {
    let fixture = Fixture::new("fetch-urls", 0)?;
    let mut urls = URLS.to_vec();
    urls.insert(1, "http://github.com/acme/private.git");
    fs::write(fixture.repo.root().join("urls"), urls.join(" "))?;
    let api = serve(204)?;
    let output = fixture.run(&api, Some(TOKEN), None);
    assert!(output.status.success(), "{output:?}");
    let authenticated = format!("https://x-access-token:{TOKEN}@github.com/acme/private.git");
    let urls = fixture.log("cargo.urls");
    assert_eq!(
        urls.lines().collect::<Vec<_>>(),
        [
            authenticated.as_str(),
            authenticated.as_str(),
            authenticated.as_str(),
            authenticated.as_str(),
            authenticated.as_str(),
            "https://example.invalid/acme/public.git"
        ]
    );
    Ok(())
}

#[test]
fn the_runner_ca_bundle_reaches_cargo_and_git() -> TestResult {
    let fixture = Fixture::new("fetch-ca-bundle", 0)?;
    let api = serve(204)?;
    let bundle = [("SSL_CERT_FILE", "/etc/runner/private-ca.pem")];
    let output = fixture.run_with(&api, Some(TOKEN), None, &["Cargo.toml"], &bundle);
    assert!(output.status.success(), "{output:?}");
    for environment in [fixture.log("cargo.env"), fixture.log("git.env")] {
        assert!(
            environment.contains("SSL_CERT_FILE=/etc/runner/private-ca.pem"),
            "{environment}"
        );
    }
    Ok(())
}

#[test]
fn the_crates_mirror_the_runner_configured_is_used_and_any_other_config_is_refused() -> TestResult {
    let fixture = Fixture::new("fetch-mirror", 0)?;
    let config = fixture.repo.root().join("velnor-cargo/config.toml");
    fs::create_dir_all(config.parent().ok_or("parent")?)?;
    fs::write(&config, MIRROR)?;
    let api = serve(204)?;
    let path = config.display().to_string();
    let output = fixture.run_with(
        &api,
        Some(TOKEN),
        None,
        &["Cargo.toml"],
        &[("VELNOR_CARGO_CONFIG", &path)],
    );
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        fixture.log("cargo.args").trim(),
        format!(
            "fetch --config {path} --locked --manifest-path Cargo.toml --config net.offline=false"
        )
    );
    assert!(!fixture.log("cargo.env").contains("VELNOR_CARGO_CONFIG"));
    let refused = Fixture::new("fetch-mirror-refused", 0)?;
    let other = refused.repo.root().join("config.toml");
    fs::write(&other, format!("{MIRROR}[credential-alias]\nx = \"y\"\n"))?;
    let output = refused.run_with(
        &api,
        Some(TOKEN),
        None,
        &["Cargo.toml"],
        &[("VELNOR_CARGO_CONFIG", &other.display().to_string())],
    );
    assert!(!output.status.success());
    assert_eq!(refused.log("cargo.args"), "");
    Ok(())
}
