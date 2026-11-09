// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[expect(
    dead_code,
    reason = "The shared peer supports additional image-session scenarios."
)]
#[path = "image_session_public/peer.rs"]
mod peer;

#[path = "registry_acquisition_public/recovery.rs"]
mod recovery;

use peer::*;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::error::Error;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;
use velnor_runner_host::{
    Journal, RegistryImageSpecification, RegistryProjection, acquire_registry_image,
};
use velnor_runner_image_manifest::{ImageEntry, ImageKind};

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

fn digest(bytes: &[u8]) -> String {
    let mut output = String::from("sha256:");
    for byte in Sha256::digest(bytes) {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 15) as usize] as char);
    }
    output
}

fn fixture() -> TestResult<(ImageEntry, Vec<u8>, Vec<u8>, String)> {
    let source = "a".repeat(40);
    let config = serde_json::to_vec(
        &json!({"os":"linux", "architecture":"amd64", "rootfs":{"type":"layers", "diff_ids":[format!("sha256:{}", "b".repeat(64))]}, "config":{"Labels":{"org.opencontainers.image.revision":source},"User":"65532:65532","WorkingDir":"/","Entrypoint":["/velnor/resource-probe"],"Env":["PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"]}}),
    )?;
    let manifest = serde_json::to_vec(
        &json!({"schemaVersion":2,"mediaType":"application/vnd.oci.image.manifest.v1+json","config":{"mediaType":"application/vnd.oci.image.config.v1+json","digest":digest(&config),"size":config.len()},"layers":[{"mediaType":"application/vnd.oci.image.layer.v1.tar+gzip","digest":format!("sha256:{}", "c".repeat(64)),"size":1}]}),
    )?;
    let image = serde_json::from_value(
        json!({"repository":ImageKind::ResourceProbe.repository(),"tag":format!("sha-{source}"),"registry_digest":digest(&manifest),"platform_manifest_digest":digest(&manifest),"config_digest":digest(&config),"build_source":{"metadata_sha256":"d".repeat(64),"build_type":"test","source_repository":"test","source_commit":source,"context_path":"test","dockerfile_path":"test","builder_platform":"linux/amd64","named_contexts":{}}}),
    )?;
    Ok((image, manifest, config, source))
}

fn pull_plan(
    succeeds: bool,
    image: &ImageEntry,
    config: &[u8],
    gate: &Arc<(Mutex<bool>, Condvar)>,
) -> TestResult<Plan> {
    let mut plan = Plan::default();
    plan.inspect.push_back(Response::bytes(404, vec![]));
    if succeeds {
        let configuration: serde_json::Value = serde_json::from_slice(config)?;
        plan.inspect.push_back(Response::json_value(200, &json!({"Id":image.config_digest,"RepoDigests":[format!("{}@{}",image.repository,image.registry_digest)],"Os":"linux","Architecture":"amd64","RootFS":{"Type":"layers","Layers":[format!("sha256:{}", "b".repeat(64))]},"Config":configuration["config"]})));
        plan.pull
        .push_back(Response::bytes(200, format!("{{\"status\":\"Digest: {}\"}}\n{{\"status\":\"Status: Image is up to date for {}@{}\"}}\n", image.registry_digest, image.repository, image.registry_digest).into_bytes()).gated(gate.clone()));
    } else {
        plan.pull
            .push_back(Response::bytes(500, vec![]).gated(gate.clone()));
    }
    Ok(plan)
}

#[expect(
    clippy::too_many_lines,
    reason = "The scenario keeps its gated pull and durable settlement together."
)]
#[tokio::test]
async fn delayed_pull_completes_and_failure_settles_uncertain() -> TestResult {
    for succeeds in [true, false] {
        let temp = tempfile::tempdir()?;
        let socket = temp.path().join("docker.sock");
        let journal_path = temp.path().join("journal.db");
        let journal = Journal::open(&journal_path).await?;
        journal.bind_engine(ENGINE_ID).await?;
        let (image, manifest, config, source) = fixture()?;
        let projection = RegistryProjection::from_oci(
            &image,
            ImageKind::ResourceProbe,
            &source,
            &manifest,
            &config,
        )?;
        let selection = selection()?;
        let spec = RegistryImageSpecification::new(
            &selection,
            &image,
            ImageKind::ResourceProbe,
            &source,
            &"e".repeat(64),
            projection,
        )?;
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let plan = pull_plan(succeeds, &image, &config, &gate)?;
        let peer = Peer::bind(&socket, plan)?;
        let endpoint = socket.to_str().ok_or("socket path")?;
        let task = async { acquire_registry_image(&journal, endpoint, spec).await };
        tokio::pin!(task);
        loop {
            tokio::select! {
                result = &mut task => return Err(format!("pull ended before gate: {result:?}, requests: {:?}", peer.requests()?).into()),
                () = tokio::time::sleep(Duration::from_millis(10)) => {},
            }
            if peer.requests()?.iter().any(|r| r.method == "POST") {
                break;
            }
        }
        tokio::time::pause();
        tokio::time::advance(Duration::from_secs(181)).await;
        let early = tokio::select! {
            biased;
            result = &mut task => Some(result),
            () = tokio::task::yield_now() => None,
        };
        tokio::time::resume();
        *gate.0.lock().map_err(|_| "gate")? = true;
        gate.1.notify_all();
        let result = match early {
            Some(result) => result,
            None => task.await,
        };
        let conn = turso::Builder::new_local(journal_path.to_str().ok_or("journal path")?)
            .build()
            .await?
            .connect()?;
        let mut rows = conn
            .query(
                "SELECT state, runtime_id, receipt, registry_digest, config_digest FROM image_import_registry",
                (),
            )
            .await?;
        let row = rows.next().await?.ok_or("missing import")?;
        let state: String = row.get(0)?;
        assert_eq!(state, if succeeds { "completed" } else { "uncertain" });
        if succeeds {
            assert_eq!(result?.runtime_id(), image.config_digest);
            let runtime: String = row.get(1)?;
            let receipt: String = row.get(2)?;
            assert_eq!(runtime, image.config_digest);
            let receipt: serde_json::Value = serde_json::from_str(&receipt)?;
            assert_eq!(receipt["store"], "classic");
            assert_eq!(receipt["runtime_id"], image.config_digest);
            assert_eq!(receipt["manifest_size"], manifest.len());
            assert_eq!(row.get::<String>(3)?, image.registry_digest);
            assert_eq!(row.get::<String>(4)?, image.config_digest);
        } else {
            assert!(result.is_err());
        }
    }
    Ok(())
}

#[test]
fn malformed_oci_projection_is_rejected() -> TestResult {
    let (image, manifest, mut config, source) = fixture()?;
    config.push(b' ');
    assert!(
        RegistryProjection::from_oci(
            &image,
            ImageKind::ResourceProbe,
            &source,
            &manifest,
            &config
        )
        .is_err()
    );
    Ok(())
}
