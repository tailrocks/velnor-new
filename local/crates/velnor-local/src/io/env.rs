// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt::Write as _;
use std::future::Future;
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
const OPENSSL: &str = "openssl genrsa -out /keys/token.pem 2048 && openssl rsa -in /keys/token.pem -RSAPublicKey_out -outform DER -out /keys/token.pub.der";

pub trait Engine: Sync {
    fn docker(&self, args: &[&str]) -> impl Future<Output = io::Result<String>> + Send;
}

#[derive(Debug)]
pub struct DockerCli;

impl Engine for DockerCli {
    async fn docker(&self, args: &[&str]) -> io::Result<String> {
        let stdout = program("docker", args, None).await?;
        Ok(String::from_utf8_lossy(&stdout).trim().to_owned())
    }
}

#[derive(Debug)]
pub struct Environment<E> {
    pub name: String,
    pub dir: PathBuf,
    engine: E,
}

#[derive(Debug)]
pub struct Addresses {
    pub(super) cache: Proxy,
    pub(super) stub: Proxy,
    pub stub_url: String,
}

pub(super) fn words(text: &'static str) -> Vec<&'static str> {
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

impl<E: Engine> Environment<E> {
    pub async fn at(dir: &Path, engine: E) -> io::Result<Self> {
        fs::create_dir_all(dir).await?;
        let dir = fs::canonicalize(dir).await?;
        let digest =
            ring::digest::digest(&ring::digest::SHA256, dir.as_os_str().as_encoded_bytes());
        let name = digest.as_ref()[..4]
            .iter()
            .fold("velnor-".to_owned(), |mut name, byte| {
                let _ = write!(name, "{byte:02x}");
                name
            });
        Ok(Self { name, dir, engine })
    }

    async fn docker(&self, constant: &'static str, values: &[&str]) -> io::Result<String> {
        self.engine
            .docker(&[&words(constant)[..], values].concat())
            .await
    }

    async fn start(&self, role: &str, constant: &'static str, values: &[&str]) -> io::Result<()> {
        let container = format!("{}-{role}", self.name);
        let label = format!("velnor-local={}", self.name);
        let head = [
            "--name",
            &container,
            "--label",
            &label,
            "--network",
            &self.name,
        ];
        self.docker(constant, &[&head[..], values].concat()).await?;
        Ok(())
    }

    pub async fn up(&self, repo: &Path, args: &EnvironmentArgs) -> io::Result<Addresses> {
        self.down().await?;
        let started = self.start_all(repo, args).await;
        if started.is_err() {
            let _ = self.down().await;
        }
        started
    }

    async fn start_all(&self, repo: &Path, args: &EnvironmentArgs) -> io::Result<Addresses> {
        for directory in ["keys", "cache", "api"] {
            fs::create_dir_all(self.dir.join(directory)).await?;
        }
        let pulls = self.dir.join("api/pulls.json");
        if fs::metadata(&pulls).await.is_err() {
            fs::write(&pulls, "[]").await?;
        }
        let mount = |from: &str, to: &str| format!("{}:{to}", self.dir.join(from).display());
        if fs::metadata(self.dir.join("keys/token.pub.der"))
            .await
            .is_err()
        {
            let keys = mount("keys", "/keys");
            let values = ["-v", &keys, STANDIN_IMAGE, "-c", OPENSSL];
            self.docker("run --rm --entrypoint sh", &values).await?;
        }
        let label = format!("velnor-local={}", self.name);
        self.docker("network create", &["--label", &label, &self.name])
            .await?;
        let pem = mount("keys/token.pem", "/keys/token.pem:ro");
        self.start(
            "standin",
            "run -d --network-alias runner-server -e Runner.Server__GitServerUrl=http://github.localhost \
             -e Runner.Server__GitApiServerUrl=http://api-stub \
             -e Runner.Server__RSASignatureKeyPath=/keys/token.pem",
            &["-v", &pem, STANDIN_IMAGE],
        )
        .await?;
        let der = mount("keys/token.pub.der", "/keys/token.pub.der:ro");
        let data = mount("cache", "/data");
        let server = words(
            "cache-server --listen 0.0.0.0:80 --upstream runner-server:80 --dir /data --token-key /keys/token.pub.der",
        );
        self.start(
            "cache",
            "run -d --network-alias github.localhost -p 127.0.0.1::80",
            &[&["-v", &der, "-v", &data, LOCAL_IMAGE], &server[..]].concat(),
        )
        .await?;
        let checkout = format!("{}:/repo:ro", repo.display());
        let state = mount("api", "/api");
        let stub = words(
            "api-stub --listen 0.0.0.0:80 --repo /repo --dir /api --cache-dir /data --standin runner-server:80",
        );
        let mounts = ["-v", &checkout, "-v", &state, "-v", &data, LOCAL_IMAGE];
        let branch = ["--default-branch", &args.default_branch];
        self.start(
            "api",
            "run -d --network-alias api-stub -p 127.0.0.1::80 -e GIT_CONFIG_COUNT=1 \
             -e GIT_CONFIG_KEY_0=safe.directory -e GIT_CONFIG_VALUE_0=*",
            &[&mounts[..], &stub, &branch].concat(),
        )
        .await?;
        for number in 1..=args.runners {
            let register = format!(
                "./config.sh --unattended --url http://github.localhost/runner/server \
                 --token anything --name {}-runner-{number} --labels {RUNNER_LABELS} \
                 && exec ./run.sh",
                self.name
            );
            self.start(
                &format!("runner-{number}"),
                "run -d --platform linux/arm64 -e GITHUB_ACTIONS_RUNNER_FORCE_GHES=1",
                &[RUNNER_IMAGE, "sh", "-c", &register],
            )
            .await?;
        }
        self.ready(args.runners).await
    }

    async fn published(&self, role: &str) -> io::Result<String> {
        let container = format!("{}-{role}", self.name);
        let ports = self.docker("port", &[&container, "80/tcp"]).await?;
        let address = ports
            .lines()
            .find_map(|line| line.trim().parse::<SocketAddr>().ok());
        address
            .map(|address| address.to_string())
            .ok_or_else(|| io::Error::other(format!("{role} publishes no port: {ports}")))
    }

    async fn ready(&self, runners: u16) -> io::Result<Addresses> {
        let stub = self.published("api").await?;
        let addresses = Addresses {
            cache: Proxy::new(&self.published("cache").await?),
            stub: Proxy::new(&stub),
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

    pub async fn down(&self) -> io::Result<()> {
        let filter = format!("label=velnor-local={}", self.name);
        let containers = self.docker("ps -aq --filter", &[&filter]).await?;
        if !containers.is_empty() {
            let ids: Vec<&str> = containers.split_whitespace().collect();
            self.docker("rm -f -v", &ids).await?;
        }
        let networks = self.docker("network ls -q --filter", &[&filter]).await?;
        if !networks.is_empty() {
            let ids: Vec<&str> = networks.split_whitespace().collect();
            self.docker("network rm", &ids).await?;
        }
        Ok(())
    }
}
