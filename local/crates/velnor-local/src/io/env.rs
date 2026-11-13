// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt::Write as _;
use std::io;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use http_body_util::{BodyExt, Full};
use hyper::Request;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use super::proxy::Proxy;
use crate::cli::EnvironmentArgs;

pub(super) const STANDIN_IMAGE: &str = "velnor-local/runner-server";
const LOCAL_IMAGE: &str = "velnor-local/cache-server";
const RUNNER_IMAGE: &str = "velnor-local/runner";
const RUNNER_LABELS: &str = "ubuntu-latest,ubuntu-22.04,ubuntu-24.04,ubuntu-26.04,ubuntu-22.04-arm,ubuntu-24.04-arm,ubuntu-26.04-arm";
const READY_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug)]
pub(super) struct Environment {
    pub(super) name: String,
    pub(super) dir: PathBuf,
}

#[derive(Debug)]
pub(super) struct Addresses {
    pub(super) cache: Proxy,
    pub(super) stub_url: String,
}

pub(super) fn words(text: &str) -> Vec<&str> {
    text.split_whitespace().collect()
}

pub(super) async fn program(
    program: &str,
    args: &[&str],
    input: Option<&[u8]>,
) -> io::Result<Vec<u8>> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(input.map_or_else(Stdio::null, |_| Stdio::piped()))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    if let (Some(input), Some(mut stdin)) = (input, child.stdin.take()) {
        stdin.write_all(input).await?;
    }
    let output = child.wait_with_output().await?;
    if output.status.success() {
        return Ok(output.stdout);
    }
    let error = String::from_utf8_lossy(&output.stderr);
    let failed = format!("{program} {}: {}", args.join(" "), error.trim());
    Err(io::Error::other(failed))
}

pub(super) async fn docker(args: &[&str]) -> io::Result<String> {
    let stdout = program("docker", args, None).await?;
    Ok(String::from_utf8_lossy(&stdout).trim().to_owned())
}

async fn docker_script(command: &str, script: &str) -> io::Result<String> {
    docker(&[&words(command)[..], &[script]].concat()).await
}

pub(super) async fn request(
    proxy: &Proxy,
    method: &str,
    path: &str,
    body: &str,
) -> Option<Vec<u8>> {
    let body = Full::new(bytes::Bytes::from(body.to_owned()));
    let request = Request::builder()
        .method(method)
        .uri(path)
        .body(body.map_err(|never| match never {}).boxed_unsync())
        .ok()?;
    let response = proxy.forward(request).await.ok()?;
    let ok = response.status().is_success();
    let bytes = response.into_body().collect().await.ok()?.to_bytes();
    ok.then(|| bytes.to_vec())
}

fn without_space(path: PathBuf) -> io::Result<PathBuf> {
    if path.to_string_lossy().contains(char::is_whitespace) {
        return Err(io::Error::other(format!(
            "{} contains a space",
            path.display()
        )));
    }
    Ok(path)
}

impl Environment {
    pub(super) async fn at(dir: &Path) -> io::Result<Self> {
        fs::create_dir_all(dir).await?;
        let dir = without_space(fs::canonicalize(dir).await?)?;
        let digest =
            ring::digest::digest(&ring::digest::SHA256, dir.as_os_str().as_encoded_bytes());
        let name = digest.as_ref()[..4]
            .iter()
            .fold("velnor-".to_owned(), |mut name, byte| {
                let _ = write!(name, "{byte:02x}");
                name
            });
        Ok(Self { name, dir })
    }

    pub(super) async fn up(&self, repo: &Path, args: &EnvironmentArgs) -> io::Result<Addresses> {
        self.down().await?;
        for directory in ["keys", "cache", "api"] {
            fs::create_dir_all(self.dir.join(directory)).await?;
        }
        let pulls = self.dir.join("api/pulls.json");
        if fs::metadata(&pulls).await.is_err() {
            fs::write(&pulls, "[]").await?;
        }
        let repo = without_space(repo.to_owned())?;
        let (dir, name, repo) = (self.dir.display(), &self.name, repo.display());
        if fs::metadata(self.dir.join("keys/token.pub.der"))
            .await
            .is_err()
        {
            let openssl = "openssl genrsa -out /keys/token.pem 2048 && openssl rsa -in /keys/token.pem -RSAPublicKey_out -outform DER -out /keys/token.pub.der";
            let keys = format!("run --rm -v {dir}/keys:/keys --entrypoint sh {STANDIN_IMAGE} -c");
            docker_script(&keys, openssl).await?;
        }
        let run = format!("run -d --label velnor-local={name} --network {name}");
        docker(&words(&format!(
            "network create --label velnor-local={name} {name}"
        )))
        .await?;
        docker(&words(&format!(
            "{run} --name {name}-standin --network-alias runner-server \
             -e Runner.Server__GitServerUrl=http://github.localhost \
             -e Runner.Server__GitApiServerUrl=http://api-stub \
             -e Runner.Server__RSASignatureKeyPath=/keys/token.pem \
             -v {dir}/keys/token.pem:/keys/token.pem:ro {STANDIN_IMAGE}"
        )))
        .await?;
        docker(&words(&format!(
            "{run} --name {name}-cache --network-alias github.localhost -p 127.0.0.1::80 \
             -v {dir}/keys/token.pub.der:/keys/token.pub.der:ro -v {dir}/cache:/data \
             {LOCAL_IMAGE} cache-server --listen 0.0.0.0:80 --upstream runner-server:80 \
             --dir /data --token-key /keys/token.pub.der"
        )))
        .await?;
        docker(&words(&format!(
            "{run} --name {name}-api --network-alias api-stub -p 127.0.0.1::80 \
             -e GIT_CONFIG_COUNT=1 -e GIT_CONFIG_KEY_0=safe.directory -e GIT_CONFIG_VALUE_0=* \
             -v {repo}:/repo:ro -v {dir}/api:/api -v {dir}/cache:/data \
             {LOCAL_IMAGE} api-stub --listen 0.0.0.0:80 --repo /repo --dir /api \
             --cache-dir /data --default-branch {}",
            args.default_branch
        )))
        .await?;
        for number in 1..=args.runners {
            let runner = format!(
                "{run} --name {name}-runner-{number} --platform linux/arm64 \
                 -e GITHUB_ACTIONS_RUNNER_FORCE_GHES=1 {RUNNER_IMAGE} sh -c"
            );
            let register = format!(
                "./config.sh --unattended --url http://github.localhost/runner/server \
                 --token anything --name {name}-runner-{number} --labels {RUNNER_LABELS} \
                 && exec ./run.sh"
            );
            docker_script(&runner, &register).await?;
        }
        self.ready(args.runners).await
    }

    async fn published(&self, role: &str) -> io::Result<String> {
        let ports = docker(&["port", &format!("{}-{role}", self.name), "80/tcp"]).await?;
        let address = ports
            .lines()
            .find_map(|line| line.trim().parse::<SocketAddr>().ok());
        address
            .map(|address| address.to_string())
            .ok_or_else(|| io::Error::other(format!("{role} publishes no port: {ports}")))
    }

    async fn ready(&self, runners: usize) -> io::Result<Addresses> {
        let stub = self.published("api").await?;
        let addresses = Addresses {
            cache: Proxy::new(&self.published("cache").await?),
            stub_url: format!("http://{stub}"),
        };
        let deadline = tokio::time::Instant::now() + READY_TIMEOUT;
        for number in 1..=runners {
            let path = format!(
                "/_apis/v1/Message/isagentonline?name={}-runner-{number}",
                self.name
            );
            while request(&addresses.cache, "GET", &path, "").await.is_none() {
                if tokio::time::Instant::now() > deadline {
                    return Err(io::Error::other(format!(
                        "runner {number} did not connect within 60 s"
                    )));
                }
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        }
        Ok(addresses)
    }

    pub(super) async fn down(&self) -> io::Result<()> {
        let filter = format!("label=velnor-local={}", self.name);
        let containers = docker(&["ps", "-aq", "--filter", &filter]).await?;
        if !containers.is_empty() {
            docker(&[&["rm", "-f", "-v"], &words(&containers)[..]].concat()).await?;
        }
        let networks = docker(&["network", "ls", "-q", "--filter", &filter]).await?;
        if !networks.is_empty() {
            docker(&[&["network", "rm"], &words(&networks)[..]].concat()).await?;
        }
        Ok(())
    }
}
