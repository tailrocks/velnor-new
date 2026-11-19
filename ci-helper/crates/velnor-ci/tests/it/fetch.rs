// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};
use std::thread;

use crate::io::Repo;

pub(crate) type TestResult = Result<(), Box<dyn Error>>;

pub(crate) const TOKEN: &str = "ghs_recorded_installation_token";
const URLS: [&str; 5] = [
    "https://github.com/acme/private.git",
    "ssh://git@github.com/acme/private.git",
    "ssh://github.com/acme/private.git",
    "git@github.com:acme/private.git",
    "https://example.invalid/acme/public.git",
];

pub(crate) fn script(path: &Path, body: &str) -> TestResult {
    fs::create_dir_all(path.parent().ok_or("parent")?)?;
    fs::write(path, format!("#!/bin/sh\n{body}\n"))?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
    Ok(())
}

pub(crate) struct Api {
    pub(crate) url: String,
    pub(crate) requests: Arc<Mutex<Vec<String>>>,
}

pub(crate) fn serve(status: u16) -> Result<Api, Box<dyn Error>> {
    serve_body(status, "")
}

pub(crate) fn serve_body(status: u16, body: &str) -> Result<Api, Box<dyn Error>> {
    let body = body.to_owned();
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let url = format!("http://{}", listener.local_addr()?);
    let requests = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&requests);
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut buffer = [0_u8; 4096];
            let read = stream.read(&mut buffer).unwrap_or(0);
            let text = String::from_utf8_lossy(&buffer[..read]).to_lowercase();
            let first = text.lines().next().unwrap_or_default().to_owned();
            let bearer = text
                .lines()
                .find(|line| line.starts_with("authorization:"))
                .unwrap_or_default()
                .to_owned();
            if let Ok(mut seen) = seen.lock() {
                seen.push(format!("{first} | {bearer}"));
            }
            drop(
                stream.write_all(
                    format!(
                        "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                ),
            );
        }
    });
    Ok(Api { url, requests })
}

pub(crate) struct Fixture {
    pub(crate) repo: Repo,
}

impl Fixture {
    pub(crate) fn new(name: &str, cargo_exit: u8) -> Result<Self, Box<dyn Error>> {
        let repo = Repo::empty(name);
        let root = repo.root().to_path_buf();
        let log = root.join("log");
        fs::create_dir_all(&log)?;
        let trusted = root.join("trusted/bin");
        script(
            &trusted.join("cargo"),
            &format!(
                "env > {log}/cargo.env\necho \"$*\" > {log}/cargo.args\npwd -P > {log}/cargo.pwd\nfor url in $(cat {root}/urls); do /usr/bin/git ls-remote --get-url \"$url\" >> {log}/cargo.urls; done\ngit ls-remote > /dev/null 2>&1\nexit {cargo_exit}",
                log = log.display(),
                root = root.display()
            ),
        )?;
        fs::write(root.join("urls"), URLS.join(" "))?;
        script(
            &trusted.join("git"),
            &format!("env >> {}/git.env", log.display()),
        )?;
        for tool in ["cargo", "git"] {
            script(
                &root.join("hostile/bin").join(tool),
                &format!("echo ran >> {}/hostile.log", log.display()),
            )?;
        }
        repo.write_unstaged("Cargo.toml", "");
        Ok(Self { repo })
    }

    fn run(&self, api: &Api, token: Option<&str>, home: Option<&str>) -> Output {
        self.run_with(api, token, home, &["Cargo.toml"], &[])
    }

    pub(crate) fn run_with(
        &self,
        api: &Api,
        token: Option<&str>,
        home: Option<&str>,
        manifests: &[&str],
        extra: &[(&str, &str)],
    ) -> Output {
        let root = self.repo.root();
        let path = format!(
            "{}:{}:/usr/bin:/bin",
            root.join("hostile/bin").display(),
            root.join("trusted/bin").display()
        );
        let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-ci"));
        command
            .current_dir(root)
            .arg("fetch")
            .arg("--trusted-dir")
            .arg(root.join("trusted"))
            .env_clear()
            .env("PATH", path)
            .env("VELNOR_API_URL", &api.url)
            .env("GIT_CONFIG_COUNT", "1")
            .env("GIT_CONFIG_KEY_0", "core.sshCommand")
            .env("GIT_CONFIG_VALUE_0", "hostile-config")
            .env("CARGO_HTTP_PROXY", "http://hostile-proxy.invalid")
            .env("CARGO_NET_OFFLINE", "false");
        for manifest in manifests {
            command.args(["--manifest-path", manifest]);
        }
        command.envs(extra.iter().copied());
        if let Some(token) = token {
            command.env("VELNOR_PRIVATE_TOKEN", token);
        }
        if let Some(home) = home {
            command.env("CARGO_HOME", home);
        }
        command.output().unwrap_or_else(|e| unreachable!("{e}"))
    }

    pub(crate) fn log(&self, name: &str) -> String {
        fs::read_to_string(self.repo.root().join("log").join(name)).unwrap_or_default()
    }
}

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

const MIRROR: &str = "# velnor-ci crates-mirror owns this file\n[source.crates-io]\nreplace-with = \"velnor-crates-mirror\"\n[source.velnor-crates-mirror]\nregistry = \"sparse+https://mirror.invalid/\"\n[http]\ncainfo = \"/etc/ssl/mirror.pem\"\n";

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
            "--config {path} fetch --locked --manifest-path Cargo.toml --config net.offline=false"
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
