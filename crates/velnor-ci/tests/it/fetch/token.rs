use super::*;

#[test]
fn the_token_reaches_only_the_real_cargo_and_git_and_is_revoked_after_the_fetch() -> TestResult {
    let fixture = Fixture::new("fetch-token", 0)?;
    let api = serve(204)?;
    let output = fixture.run(&api, Some(TOKEN), None);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(fixture.log("hostile.log"), "");
    assert_eq!(
        fixture.log("cargo.args").trim(),
        "fetch --locked --manifest-path Cargo.toml --config net.offline=false"
    );
    let cargo_env = fixture.log("cargo.env");
    let git_env = fixture.log("git.env");
    assert!(cargo_env.contains(TOKEN) && git_env.contains(TOKEN));
    for environment in [&cargo_env, &git_env] {
        assert!(!environment.contains("hostile"), "{environment}");
        assert!(!environment.contains("CARGO_NET_OFFLINE"), "{environment}");
        assert!(environment.contains("GIT_CONFIG_NOSYSTEM=1"));
        assert!(
            environment
                .lines()
                .any(|line| line == "GIT_ALLOW_PROTOCOL=https:http:file"),
            "{environment}"
        );
    }
    assert!(!cargo_env.contains("PRIVATE_TOKEN"), "{cargo_env}");
    let requests = api.requests.lock().map_err(|e| e.to_string())?.clone();
    assert_eq!(
        requests,
        [format!(
            "delete /installation/token http/1.1 | authorization: bearer {}",
            TOKEN.to_lowercase()
        )]
    );
    Ok(())
}

#[test]
fn a_malformed_api_url_still_runs_the_fetch_and_reports_the_missing_revocation() -> TestResult {
    let fixture = Fixture::new("fetch-bad-api", 0)?;
    let api = serve(204)?;
    let output = fixture.run_with(
        &api,
        Some(TOKEN),
        None,
        &["Cargo.toml"],
        &[("VELNOR_API_URL", "ftp://not-the-api.invalid")],
    );
    assert!(!output.status.success());
    assert!(!fixture.log("cargo.args").is_empty(), "the fetch never ran");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("revoked") && stderr.contains("VELNOR_API_URL"),
        "{stderr}"
    );
    assert!(api.requests.lock().map_err(|e| e.to_string())?.is_empty());
    Ok(())
}

#[test]
fn the_runner_proxy_reaches_cargo_and_git_but_a_checkout_cargo_setting_does_not() -> TestResult {
    let fixture = Fixture::new("fetch-proxy", 0)?;
    let api = serve(204)?;
    let proxy = [
        ("HTTPS_PROXY", "http://proxy.runner.invalid:3128"),
        ("https_proxy", "http://proxy.runner.invalid:3128"),
        ("ALL_PROXY", "socks5://proxy.runner.invalid:1080"),
        ("NO_PROXY", "127.0.0.1,.cluster.local"),
    ];
    let output = fixture.run_with(&api, Some(TOKEN), None, &["Cargo.toml"], &proxy);
    assert!(output.status.success(), "{output:?}");
    for environment in [fixture.log("cargo.env"), fixture.log("git.env")] {
        for (name, value) in proxy {
            assert!(environment.contains(&format!("{name}={value}")), "{name}");
        }
        assert!(!environment.contains("hostile"), "{environment}");
    }
    Ok(())
}

#[test]
fn each_manifest_is_fetched_from_its_own_directory_so_its_cargo_config_applies() -> TestResult {
    let fixture = Fixture::new("fetch-nested", 0)?;
    fixture.repo.write_unstaged("tools/Cargo.toml", "");
    let api = serve(204)?;
    let output = fixture.run_with(&api, Some(TOKEN), None, &["tools/Cargo.toml"], &[]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        fixture.log("cargo.args").trim(),
        "fetch --locked --manifest-path Cargo.toml --config net.offline=false"
    );
    assert!(fixture.log("cargo.pwd").trim().ends_with("/tools"));
    Ok(())
}
