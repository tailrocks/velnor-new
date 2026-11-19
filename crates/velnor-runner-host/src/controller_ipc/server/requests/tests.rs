// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Instant;

use super::*;
use crate::config::HostConfig;
use crate::journal::ReadinessCapacityUsage;
use crate::readiness::Readiness;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

const ENGINE: &str = "engine-test";
const ROOT: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const NONCE: &str = "0123456789abcdef0123456789abcdef";
const CONFIG: &str = r#"schema = 2
[[connections]]
connection_id = "conn-test-1"
repository_id = 42
repository = "owner/repo"
credential_ref = "keychain:velnor.test/conn-test-1"
[connections.scale_set]
name = "ubuntu-26.04-scale-set"
labels = ["velnor", "ubuntu-26.04-scale-set"]
[connections.source_policy]
mode = "branch_restricted"
branches = ["main"]
tag_prefixes = []
events = ["push"]
pull_request_rules = []
[host]
max_jobs = 1
[host.resources]
runner_cpu_millicores = 2000
runner_memory_bytes = 4294967296
dind_cpu_millicores = 1000
dind_memory_bytes = 2147483648
operational_memory_reserve_bytes = 1073741824
[docker]
context = "orbstack"
platform = "linux/amd64"
endpoint = "unix:///docker.sock"
"#;

struct Fixture {
    dir: tempfile::TempDir,
    journal: Journal,
    state: ControlState,
}

impl Fixture {
    async fn published() -> TestResult<Self> {
        let dir = tempfile::tempdir()?;
        std::fs::write(dir.path().join("host.toml"), CONFIG)?;
        let journal = Journal::open(&dir.path().join("launch.db")).await?;
        journal.bind_engine(ENGINE).await?;
        let config = HostConfig::parse(CONFIG)?;
        let state = ControlState::new(&config);
        state.set_admission_ready(true);
        let binding = config.connections().first().ok_or("connection")?;
        state.record_session_poll_observation(binding, 7, 1, "session-1", None)?;
        let (_, config_sha256) =
            crate::readiness_scan::controller_config(dir.path()).ok_or("config")?;
        let now = Instant::now();
        state.publish_readiness(crate::controller_ipc::ControllerReadinessProof {
            config_sha256,
            engine_id: ENGINE.to_owned(),
            docker_root_digest: ROOT.to_owned(),
            journal_instance_id: journal.instance_id().await?,
            journal_revision: journal.revision().await?,
            sample_generation: "generation".to_owned(),
            sample_observed_at: now,
            collection_started_at: now,
            collection_finished_at: now,
            capacity: ReadinessCapacityUsage {
                jobs: 0,
                cpu_millicores: 0,
                memory_bytes: 0,
                max_jobs: 1,
                max_cpu_millicores: 1000,
                max_memory_bytes: 1 << 30,
                operational_cpu_millicores: 500,
                operational_memory_bytes: 1 << 29,
                operational_lease_quiescent: true,
            },
            sample_cpu_millicores: 2000,
            sample_memory_total_bytes: 4 << 30,
            sample_memory_available_bytes: 1 << 30,
            inventory_sha256: ROOT.to_owned(),
            poll_receipts: state.complete_session_poll_receipts()?.ok_or("polls")?,
        })?;
        Ok(Self {
            dir,
            journal,
            state,
        })
    }

    async fn ask(&self, request: Request) -> TestResult<Option<Readiness>> {
        let response = handle(
            &self.journal,
            &self.state,
            self.dir.path(),
            request,
            ENGINE,
            ROOT,
        )
        .await?;
        Ok(response.readiness)
    }

    async fn controller_readiness(&self) -> TestResult<Option<Readiness>> {
        let (_, config_sha256) =
            crate::readiness_scan::controller_config(self.dir.path()).ok_or("config")?;
        self.ask(Request::ControllerReadiness {
            config_sha256,
            engine_id: ENGINE.to_owned(),
            docker_root_digest: ROOT.to_owned(),
        })
        .await
    }

    async fn doctor_probe_readiness(&self) -> TestResult<Option<Readiness>> {
        self.ask(Request::DoctorProbeStatus {
            nonce: NONCE.to_owned(),
        })
        .await
    }

    fn drain(&self) -> TestResult {
        std::fs::write(self.dir.path().join("drain"), "")?;
        Ok(())
    }

    fn undrain(&self) -> TestResult {
        std::fs::remove_file(self.dir.path().join("drain"))?;
        Ok(())
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_matching_proof_without_a_drain_marker_is_ready() -> TestResult {
    let fixture = Fixture::published().await?;
    assert_eq!(
        fixture.controller_readiness().await?,
        Some(Readiness::Ready)
    );
    assert_eq!(
        fixture.doctor_probe_readiness().await?,
        Some(Readiness::Ready)
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_drain_marker_makes_a_matching_proof_draining_until_it_is_removed() -> TestResult {
    let fixture = Fixture::published().await?;
    fixture.drain()?;
    assert_eq!(
        fixture.controller_readiness().await?,
        Some(Readiness::Draining)
    );
    assert_eq!(
        fixture.doctor_probe_readiness().await?,
        Some(Readiness::Draining)
    );
    fixture.undrain()?;
    assert_eq!(
        fixture.controller_readiness().await?,
        Some(Readiness::Ready)
    );
    Ok(())
}
