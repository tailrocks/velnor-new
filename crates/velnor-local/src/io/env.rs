// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt::Write as _;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;
use tokio::fs;

use super::local_ci::LocalCi;
use super::proxy::Proxy;
use crate::cli::EnvironmentArgs;
use crate::jit::labels;
use crate::side::Side;

mod engine;
mod ready;

pub(super) use crate::doctor::STANDIN_IMAGE;
use crate::doctor::{CACHE_IMAGE as LOCAL_IMAGE, RUNNER_IMAGE};
pub use engine::{DockerCli, Engine};
pub(super) use engine::{program, words};
pub const READY_TIMEOUT: Duration = Duration::from_secs(60);
const RUNNER_START: &str = "run -d --platform linux/arm64 -e GITHUB_ACTIONS_RUNNER_FORCE_GHES=1 -e VELNOR_CRATES_MIRROR=off";
const OPENSSL: &str = "openssl genrsa -out /keys/token.pem 2048 && openssl rsa -in /keys/token.pem -RSAPublicKey_out -outform DER -out /keys/token.pub.der";

#[derive(Debug)]
pub struct Environment<E> {
    pub name: String,
    pub dir: PathBuf,
    labels: String,
    engine: E,
    ready_timeout: Duration,
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

fn strs(values: &[String]) -> Vec<&str> {
    values.iter().map(String::as_str).collect()
}

impl<E: Engine> Environment<E> {
    pub async fn at(dir: &Path, engine: E) -> io::Result<Self> {
        fs::create_dir_all(dir).await?;
        let dir = fs::canonicalize(dir).await?;
        let digest =
            ring::digest::digest(&ring::digest::SHA256, dir.as_os_str().as_encoded_bytes());
        let name =
            digest.as_ref()[..4]
                .iter()
                .fold("velnor-local-".to_owned(), |mut name, byte| {
                    write!(name, "{byte:02x}").ok();
                    name
                });
        Ok(Self {
            name,
            dir,
            labels: labels(&[]),
            engine,
            ready_timeout: READY_TIMEOUT,
        })
    }

    #[must_use]
    pub fn with_labels(mut self, extra: &[String]) -> Self {
        self.labels = labels(extra);
        self
    }

    #[must_use]
    pub const fn with_ready_timeout(mut self, ready_timeout: Duration) -> Self {
        self.ready_timeout = ready_timeout;
        self
    }

    pub(super) fn labels(&self) -> &str {
        &self.labels
    }

    async fn docker(&self, constant: &'static str, values: &[&str]) -> io::Result<String> {
        self.engine
            .docker(&[&words(constant)[..], values].concat())
            .await
    }

    pub(super) fn engine(&self) -> &E {
        &self.engine
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
        let local = super::local_ci::flags(args).await?;
        self.down().await?;
        let started = self.start_all(repo, args, &local).await;
        if started.is_err() {
            self.down().await.ok();
        }
        started
    }

    async fn start_all(
        &self,
        repo: &Path,
        args: &EnvironmentArgs,
        local: &LocalCi,
    ) -> io::Result<Addresses> {
        super::pair::private_folder(&self.dir.join("keys")).await?;
        super::pair::private_folder(&self.dir.join("authority")).await?;
        for directory in ["cache", "api"] {
            fs::create_dir_all(self.dir.join(directory)).await?;
        }
        let pulls = self.dir.join("api/pulls.json");
        if fs::metadata(&pulls).await.is_err() {
            fs::write(&pulls, "[]").await?;
        }
        let mount = |from: &str, to: &str| format!("{}:{to}", self.dir.join(from).display());
        let token = self.dir.join("keys/control");
        super::keys::control(&token).await?;
        let checkout = format!("{}:/repo:ro", repo.display());
        let state = mount("api", "/api");
        let der = mount("keys/token.pub.der", "/keys/token.pub.der:ro");
        let control = mount("keys/control", "/keys/control:ro");
        let data = mount("cache", "/data");
        let ledger = mount("authority", "/authority:ro");
        self.side("", args.containers(args.runners), local).await?;
        let stub = words(
            "api-stub --listen 0.0.0.0:80 --repo /repo --dir /api --cache-dir /data --standin runner-server:80 \
             --control-listen 0.0.0.0:81 --control-token /keys/control --token-key /keys/token.pub.der \
             --ledger /authority",
        );
        let mounts = [
            "-v",
            &checkout,
            "-v",
            &der,
            "-v",
            &state,
            "-v",
            &data,
            "-v",
            &control,
            "-v",
            &ledger,
            LOCAL_IMAGE,
        ];
        let branch = ["--default-branch", &args.default_branch];
        let admin: &[&str] = if args.job_cache_admin {
            &["--job-cache-admin"]
        } else {
            &[]
        };
        self.start(
            "api",
            "run -d --network-alias api-stub -p 127.0.0.1::81 -e GIT_CONFIG_COUNT=1 \
             -e GIT_CONFIG_KEY_0=safe.directory -e GIT_CONFIG_VALUE_0=*",
            &[&mounts[..], &stub, &branch, admin, &strs(&local.stub)[..]].concat(),
        )
        .await?;
        if args.fork_runners > 0 {
            self.side(
                "fork-",
                args.containers(args.fork_runners),
                &LocalCi::default(),
            )
            .await?;
        }
        self.ready(args).await
    }

    async fn side(&self, prefix: &str, runners: u16, local: &LocalCi) -> io::Result<()> {
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
            &[&strs(&local.standin)[..], &["-v", &pem, STANDIN_IMAGE]].concat(),
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
                 --token anything --name {}-{prefix}runner-{number} --labels {} \
                 && exec ./run.sh",
                self.name, self.labels
            );
            self.start(
                &format!("{prefix}runner-{number}"),
                RUNNER_START,
                &[
                    &strs(&local.runner)[..],
                    &[RUNNER_IMAGE, "sh", "-c", &register],
                ]
                .concat(),
            )
            .await?;
        }
        Ok(())
    }

    pub async fn down(&self) -> io::Result<[usize; 3]> {
        let label = format!("velnor-local={}", self.name);
        let filter = format!("label={label}");
        let mut audit = super::cleanup::Audit::open(&self.dir, label).await?;
        super::supervisor::stop(&self.dir).await?;
        let mut removed = [0; 3];
        let kinds = [
            ("ps -aq --no-trunc --filter", "rm -f", "container"),
            ("volume ls -q --filter", "volume rm -f", "volume"),
            ("network ls -q --no-trunc --filter", "network rm", "network"),
        ];
        for ((list, remove, kind), count) in kinds.into_iter().zip(&mut removed) {
            let listed = self.docker(list, &[&filter]).await?;
            let ids: Vec<&str> = listed.split_whitespace().collect();
            for id in ids {
                audit.record("intent", Some((kind, id))).await?;
                self.docker(remove, &[id]).await?;
                audit.record("removed", Some((kind, id))).await?;
                *count += 1;
            }
        }
        Ok(removed)
    }
}
