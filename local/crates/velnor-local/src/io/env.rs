// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt::Write as _;
use std::fs::Permissions;
use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use http_body_util::{BodyExt, Full};
use hyper::Request;
use ring::rand::{SecureRandom, SystemRandom};
use serde_json::Value;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use super::proxy::Proxy;
use crate::cli::EnvironmentArgs;
use crate::side::Side;

pub(super) const STANDIN_IMAGE: &str = "velnor-local/runner-server";
const LOCAL_IMAGE: &str = "velnor-local/cache-server";
const RUNNER_IMAGE: &str = "velnor-local/runner";
const RUNNER_LABELS: &str = "ubuntu-latest,ubuntu-22.04,ubuntu-24.04,ubuntu-26.04,ubuntu-22.04-arm,ubuntu-24.04-arm,ubuntu-26.04-arm";
const READY_TIMEOUT: Duration = Duration::from_secs(60);
const OPENSSL: &str = "openssl genrsa -out /keys/token.pem 2048 && openssl rsa -in /keys/token.pem -RSAPublicKey_out -outform DER -out /keys/token.pub.der";

pub trait Engine: Sync {
    fn docker(&self, args: &[&str]) -> impl Future<Output = io::Result<String>> + Send;
    fn logged(&self, args: &[&str], log: &Path) -> impl Future<Output = io::Result<bool>> + Send;
}

#[derive(Debug)]
pub struct DockerCli;

impl Engine for DockerCli {
    async fn docker(&self, args: &[&str]) -> io::Result<String> {
        let stdout = program("docker", args, None).await?;
        Ok(String::from_utf8_lossy(&stdout).trim().to_owned())
    }

    async fn logged(&self, args: &[&str], log: &Path) -> io::Result<bool> {
        let log = std::fs::File::create(log)?;
        let status = Command::new("docker")
            .args(args)
            .kill_on_drop(true)
            .stdin(Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log)
            .status()
            .await?;
        Ok(status.success())
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
    pub(super) fork: Option<Proxy>,
    pub(super) stub: Proxy,
    pub stub_url: String,
    pub(super) token: String,
}

impl Addresses {
    pub(super) fn standin(&self, side: Side) -> io::Result<&Proxy> {
        side.pick(&self.cache, self.fork.as_ref())
            .map_err(io::Error::other)
    }
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

    pub fn network(&self, side: Side) -> String {
        match side {
            Side::Untrusted => format!("{}-fork", self.name),
            Side::Trusted => self.name.clone(),
        }
    }

    pub async fn client(&self, payload: &Value, values: &[&str], log: &Path) -> io::Result<bool> {
        let label = format!("velnor-local={}", self.name);
        let network = self.network(Side::of(payload));
        let head = ["run", "--rm", "--label", &label, "--network", &network];
        self.engine.logged(&[&head[..], values].concat(), log).await
    }

    async fn start(&self, role: &str, constant: &'static str, values: &[&str]) -> io::Result<()> {
        let container = format!("{}-{role}", self.name);
        let label = format!("velnor-local={}", self.name);
        let network = self.network(Side::of_role(role));
        let head = [
            "--name",
            &container,
            "--label",
            &label,
            "--network",
            &network,
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
        let token = self.dir.join("keys/control");
        if fs::metadata(&token).await.is_err() {
            let mut bytes = [0_u8; 32];
            SystemRandom::new()
                .fill(&mut bytes)
                .map_err(|_| io::Error::other("no random bytes for the control token"))?;
            let hex = bytes.iter().fold(String::new(), |mut hex, byte| {
                let _ = write!(hex, "{byte:02x}");
                hex
            });
            fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&token)
                .await?
                .write_all(hex.as_bytes())
                .await?;
        }
        fs::set_permissions(&token, Permissions::from_mode(0o600)).await?;
        let checkout = format!("{}:/repo:ro", repo.display());
        let state = mount("api", "/api");
        let control = mount("keys/control", "/keys/control:ro");
        let data = mount("cache", "/data");
        self.side("", args.runners).await?;
        let stub = words(
            "api-stub --listen 0.0.0.0:80 --repo /repo --dir /api --cache-dir /data --standin runner-server:80 \
             --control-listen 0.0.0.0:81 --control-token /keys/control",
        );
        let mounts = [
            "-v",
            &checkout,
            "-v",
            &state,
            "-v",
            &data,
            "-v",
            &control,
            LOCAL_IMAGE,
        ];
        let branch = ["--default-branch", &args.default_branch];
        self.start(
            "api",
            "run -d --network-alias api-stub -p 127.0.0.1::81 -e GIT_CONFIG_COUNT=1 \
             -e GIT_CONFIG_KEY_0=safe.directory -e GIT_CONFIG_VALUE_0=*",
            &[&mounts[..], &stub, &branch].concat(),
        )
        .await?;
        if args.fork_runners > 0 {
            self.side("fork-", args.fork_runners).await?;
        }
        self.ready(args).await
    }

    async fn side(&self, prefix: &str, runners: u16) -> io::Result<()> {
        let label = format!("velnor-local={}", self.name);
        let network = self.network(Side::of_role(prefix));
        self.docker("network create", &["--label", &label, &network])
            .await?;
        let mount = |from: &str, to: &str| format!("{}:{to}", self.dir.join(from).display());
        let keys = match Side::of_role(prefix) {
            Side::Trusted => "keys",
            Side::Untrusted => "keys/fork",
        };
        fs::create_dir_all(self.dir.join(keys)).await?;
        if fs::metadata(self.dir.join(keys).join("token.pub.der"))
            .await
            .is_err()
        {
            let generated = mount(keys, "/keys");
            let values = ["-v", &generated, STANDIN_IMAGE, "-c", OPENSSL];
            self.docker("run --rm --entrypoint sh", &values).await?;
        }
        let pem = mount(&format!("{keys}/token.pem"), "/keys/token.pem:ro");
        self.start(
            &format!("{prefix}standin"),
            "run -d --network-alias runner-server -e Runner.Server__GitServerUrl=http://github.localhost \
             -e Runner.Server__GitApiServerUrl=http://api-stub \
             -e Runner.Server__RSASignatureKeyPath=/keys/token.pem",
            &["-v", &pem, STANDIN_IMAGE],
        )
        .await?;
        let der = mount(&format!("{keys}/token.pub.der"), "/keys/token.pub.der:ro");
        let data = mount("cache", "/data");
        let control = mount("keys/control", "/keys/control:ro");
        let mut server = words(
            "cache-server --listen 0.0.0.0:80 --upstream runner-server:80 --dir /data --token-key /keys/token.pub.der \
             --control-listen 0.0.0.0:81 --control-token /keys/control",
        );
        if Side::of_role(prefix) == Side::Untrusted {
            server.push("--restore-only");
        }
        self.start(
            &format!("{prefix}cache"),
            "run -d --network-alias github.localhost -p 127.0.0.1::81",
            &[
                &["-v", &der, "-v", &data, "-v", &control, LOCAL_IMAGE],
                &server[..],
            ]
            .concat(),
        )
        .await?;
        for number in 1..=runners {
            let register = format!(
                "./config.sh --unattended --url http://github.localhost/runner/server \
                 --token anything --name {}-{prefix}runner-{number} --labels {RUNNER_LABELS} \
                 && exec ./run.sh",
                self.name
            );
            self.start(
                &format!("{prefix}runner-{number}"),
                "run -d --platform linux/arm64 -e GITHUB_ACTIONS_RUNNER_FORCE_GHES=1",
                &[RUNNER_IMAGE, "sh", "-c", &register],
            )
            .await?;
        }
        Ok(())
    }

    async fn published(&self, role: &str) -> io::Result<String> {
        let container = format!("{}-{role}", self.name);
        let ports = self.docker("port", &[&container, "81/tcp"]).await?;
        let address = ports
            .lines()
            .find_map(|line| line.trim().parse::<SocketAddr>().ok());
        address
            .map(|address| address.to_string())
            .ok_or_else(|| io::Error::other(format!("{role} publishes no port: {ports}")))
    }

    async fn ready(&self, args: &EnvironmentArgs) -> io::Result<Addresses> {
        let token = fs::read_to_string(self.dir.join("keys/control")).await?;
        let stub = self.published("api").await?;
        let fork = if args.fork_runners > 0 {
            Some(Proxy::authorized(
                &self.published("fork-cache").await?,
                &token,
            ))
        } else {
            None
        };
        let addresses = Addresses {
            cache: Proxy::authorized(&self.published("cache").await?, &token),
            fork,
            stub: Proxy::authorized(&stub, &token),
            stub_url: format!("http://{stub}"),
            token,
        };
        let deadline = tokio::time::Instant::now() + READY_TIMEOUT;
        let sides = [("", &addresses.cache, args.runners)].into_iter().chain(
            addresses
                .fork
                .as_ref()
                .map(|fork| ("fork-", fork, args.fork_runners)),
        );
        for (prefix, proxy, runners) in sides {
            for number in 1..=runners {
                let path = format!(
                    "/_apis/v1/Message/isagentonline?name={}-{prefix}runner-{number}",
                    self.name
                );
                while request(proxy, "GET", &path, "").await.is_none() {
                    if tokio::time::Instant::now() > deadline {
                        return Err(io::Error::other(format!(
                            "{prefix}runner {number} did not connect within 60 s"
                        )));
                    }
                    tokio::time::sleep(Duration::from_millis(200)).await;
                }
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
