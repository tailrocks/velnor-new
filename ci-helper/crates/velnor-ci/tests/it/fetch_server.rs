// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;

use super::fetch::{Fixture, TOKEN, TestResult, script, serve};

#[test]
fn a_credential_provider_a_checkout_config_declares_is_never_run_and_never_sees_the_token()
-> TestResult {
    let fixture = Fixture::new("fetch-providers", 0)?;
    let root = fixture.repo.root().to_path_buf();
    let recorder = root.join("recorder");
    let seen = root.join("log/recorder.env");
    script(
        &recorder,
        &format!(
            "env > {}\necho invoked >> {}.log",
            seen.display(),
            seen.display()
        ),
    )?;
    let rec = recorder.display();
    fixture.repo.write_unstaged(
        ".cargo/config.toml",
        &format!(
            "[registry]\nglobal-credential-providers = [\"{rec}\"]\ncredential-provider = \"{rec}\"\n[registries.other-reg]\nindex = \"sparse+https://other.invalid/\"\ncredential-provider = [\"{rec}\", \"arg\"]\n"
        ),
    );
    fixture.repo.write_unstaged("tools/Cargo.toml", "");
    fixture.repo.write_unstaged(
        "tools/.cargo/config.toml",
        &format!("[registries.alt]\nindex = \"sparse+https://alt.invalid/\"\ncredential-provider = \"{rec}\"\n"),
    );
    script(
        &root.join("trusted/bin/cargo"),
        &format!(
            "env > {log}/cargo.env\necho \"$*\" > {log}/cargo.args\n\
             [ -n \"$CARGO_REGISTRIES_ALT_CREDENTIAL_PROVIDER\" ] || {rec}\n\
             [ -n \"$CARGO_REGISTRIES_OTHER_REG_CREDENTIAL_PROVIDER\" ] || {rec}\n\
             [ -n \"$CARGO_REGISTRY_CREDENTIAL_PROVIDER\" ] || {rec}\n\
             case \"$*\" in *registry.global-credential-providers*) ;; *) {rec} ;; esac",
            log = root.join("log").display()
        ),
    )?;
    let api = serve(204)?;
    let output = fixture.run_with(&api, Some(TOKEN), None, &["tools/Cargo.toml"], &[]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(fixture.log("recorder.env.log"), "");
    assert!(!fixture.log("recorder.env").contains(TOKEN));
    let cargo_env = fixture.log("cargo.env");
    for variable in [
        "CARGO_REGISTRIES_ALT_CREDENTIAL_PROVIDER=cargo:token",
        "CARGO_REGISTRIES_OTHER_REG_CREDENTIAL_PROVIDER=cargo:token",
        "CARGO_REGISTRY_CREDENTIAL_PROVIDER=cargo:token",
    ] {
        assert!(cargo_env.contains(variable), "{variable}\n{cargo_env}");
    }
    assert_eq!(
        fixture.log("cargo.args").trim(),
        "fetch --locked --manifest-path Cargo.toml --config net.offline=false --config registry.global-credential-providers=[\"cargo:token-from-stdout /usr/bin/true\"]"
    );
    Ok(())
}

#[test]
fn a_github_enterprise_server_gets_its_own_urls_authenticated_and_github_com_is_left_alone()
-> TestResult {
    let fixture = Fixture::new("fetch-ghes", 0)?;
    let urls = [
        "https://github.corp/acme/private.git",
        "ssh://git@github.corp/acme/private.git",
        "ssh://github.corp/acme/private.git",
        "git@github.corp:acme/private.git",
        "https://github.com/acme/other.git",
    ];
    fs::write(fixture.repo.root().join("urls"), urls.join(" "))?;
    let api = serve(204)?;
    let output = fixture.run_with(
        &api,
        Some(TOKEN),
        None,
        &["Cargo.toml"],
        &[("VELNOR_SERVER_URL", "https://github.corp")],
    );
    assert!(output.status.success(), "{output:?}");
    let authenticated = format!("https://x-access-token:{TOKEN}@github.corp/acme/private.git");
    assert_eq!(
        fixture.log("cargo.urls").lines().collect::<Vec<_>>(),
        [
            authenticated.as_str(),
            authenticated.as_str(),
            authenticated.as_str(),
            authenticated.as_str(),
            "https://github.com/acme/other.git"
        ]
    );
    Ok(())
}

#[test]
fn a_server_url_that_is_not_a_plain_https_host_runs_nothing_and_still_revokes() -> TestResult {
    for (index, bad) in [
        "http://github.corp",
        "https://user@github.corp",
        "https://github.corp/path",
    ]
    .into_iter()
    .enumerate()
    {
        let fixture = Fixture::new(&format!("fetch-server-{index}"), 0)?;
        let api = serve(204)?;
        let output = fixture.run_with(
            &api,
            Some(TOKEN),
            None,
            &["Cargo.toml"],
            &[("VELNOR_SERVER_URL", bad)],
        );
        assert!(!output.status.success(), "{bad}: {output:?}");
        assert_eq!(fixture.log("cargo.args"), "", "{bad}");
        assert_eq!(api.requests.lock().map_err(|e| e.to_string())?.len(), 1);
    }
    Ok(())
}

#[test]
fn an_include_chain_deeper_than_the_scanner_reads_runs_nothing_and_still_revokes() -> TestResult {
    let fixture = Fixture::new("fetch-deep-include", 0)?;
    for level in 0..12 {
        let (name, next) = if level == 0 {
            (".cargo/config.toml".to_owned(), "c1.toml".to_owned())
        } else {
            (
                format!(".cargo/c{level}.toml"),
                format!("c{}.toml", level + 1),
            )
        };
        let body = if level == 11 {
            "[registries.deep]\nindex = \"sparse+https://deep.invalid/\"\ncredential-provider = \"/bin/false\"\n".to_owned()
        } else {
            format!("include = [\"{next}\"]\n")
        };
        fixture.repo.write_unstaged(&name, &body);
    }
    let api = serve(204)?;
    let output = fixture.run_with(&api, Some(TOKEN), None, &["Cargo.toml"], &[]);
    assert!(!output.status.success(), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("include"),
        "{output:?}"
    );
    assert_eq!(fixture.log("cargo.args"), "");
    assert_eq!(api.requests.lock().map_err(|e| e.to_string())?.len(), 1);
    Ok(())
}

#[test]
fn a_hook_in_a_restored_git_database_never_runs_during_the_authenticated_fetch() -> TestResult {
    let fixture = Fixture::new("fetch-hooks", 0)?;
    let root = fixture.repo.root().to_path_buf();
    let database = root.join("db");
    let git = |args: &[&str]| {
        std::process::Command::new("/usr/bin/git")
            .current_dir(&database)
            .env_clear()
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .args(["-c", "user.name=a", "-c", "user.email=a@b.c"])
            .args(args)
            .output()
    };
    fs::create_dir_all(&database)?;
    git(&["init", "-q"])?;
    git(&["commit", "-q", "--allow-empty", "-m", "x"])?;
    let ran = root.join("log/hook.ran");
    script(
        &database.join(".git/hooks/reference-transaction"),
        &format!("echo \"$GIT_CONFIG_KEY_0\" >> {}", ran.display()),
    )?;
    script(
        &root.join("trusted/bin/cargo"),
        &format!(
            "cd {} && /usr/bin/git update-ref refs/heads/moved HEAD",
            database.display()
        ),
    )?;
    let api = serve(204)?;
    let output = fixture.run_with(&api, Some(TOKEN), None, &["Cargo.toml"], &[]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(fixture.log("hook.ran"), "");
    Ok(())
}

fn through_a_rustup_proxy(
    fixture: &Fixture,
    active: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let bin = fixture.repo.root().join("trusted/bin");
    fs::remove_file(bin.join("cargo"))?;
    let log = fixture.repo.root().join("log");
    script(
        &bin.join("rustup"),
        &format!(
            "case \"$0\" in *rustup) echo '{active} (default)' ;; *) env > {log}/cargo.env; echo \"$*\" > {log}/cargo.args ;; esac",
            log = log.display()
        ),
    )?;
    std::os::unix::fs::symlink("rustup", bin.join("cargo"))?;
    Ok(())
}

#[test]
fn a_rustup_proxy_cargo_is_pinned_to_the_runner_toolchain_not_the_checkouts() -> TestResult {
    let fixture = Fixture::new("fetch-rustup", 0)?;
    through_a_rustup_proxy(&fixture, "stable-test")?;
    fixture.repo.write_unstaged(
        "rust-toolchain.toml",
        "[toolchain]\npath = \"/tmp/hostile-toolchain\"\n",
    );
    let api = serve(204)?;
    let output = fixture.run_with(&api, Some(TOKEN), None, &["Cargo.toml"], &[]);
    assert!(output.status.success(), "{output:?}");
    assert!(
        fixture
            .log("cargo.env")
            .lines()
            .any(|line| line == "RUSTUP_TOOLCHAIN=stable-test"),
        "{}",
        fixture.log("cargo.env")
    );
    Ok(())
}

#[test]
fn a_rustup_proxy_whose_active_toolchain_is_a_path_runs_nothing_and_still_revokes() -> TestResult {
    let fixture = Fixture::new("fetch-rustup-path", 0)?;
    through_a_rustup_proxy(&fixture, "/tmp/hostile-toolchain")?;
    let api = serve(204)?;
    let output = fixture.run_with(&api, Some(TOKEN), None, &["Cargo.toml"], &[]);
    assert!(!output.status.success(), "{output:?}");
    assert_eq!(fixture.log("cargo.args"), "");
    assert_eq!(api.requests.lock().map_err(|e| e.to_string())?.len(), 1);
    Ok(())
}
