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

mod environment;
mod token;

const MIRROR: &str = "# velnor-ci crates-mirror owns this file\n[source.crates-io]\nreplace-with = \"velnor-crates-mirror\"\n[source.velnor-crates-mirror]\nregistry = \"sparse+https://mirror.invalid/\"\n[http]\ncainfo = \"/etc/ssl/mirror.pem\"\n";
