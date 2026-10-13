#[allow(unused)]
mod common;

use std::process::Command;
use std::time::Instant;

use testcontainers_modules::postgres::Postgres;
use testcontainers_modules::testcontainers::ContainerAsync;
use testcontainers_modules::testcontainers::ImageExt;
use testcontainers_modules::testcontainers::core::{
    ContainerRequest, logs::LogFrame,
};
use testcontainers_modules::testcontainers::runners::AsyncRunner;
use tokio_postgres::NoTls;

const POSTGRES_MANIFEST: &str =
    "sha256:d8703cd7fba306b9fec9268ecedfa8a966846c053036a60e3635791957eb2f66";
fn decorate(request: ContainerRequest<Postgres>, case: &'static str) -> ContainerRequest<Postgres> {
    let name = format!("chainargos-pg-tc-probe-{case}");
    request
        .with_tag(format!("18-alpine@{POSTGRES_MANIFEST}"))
        .with_platform("linux/amd64")
        .with_container_name(name)
        .with_labels([
            ("ci.goal", "remove-ci-bottlenecks"),
            ("ci.owner", "consumer"),
            ("ci.probe", "postgres-testcontainers-wait"),
            ("ci.case", case),
        ])
        .with_host_config_modifier(|config| {
            config.nano_cpus = Some(1_000_000_000);
            config.memory = Some(1_073_741_824);
        })
        .with_log_consumer(move |frame: &LogFrame| {
            let stream = match frame {
                LogFrame::StdOut(_) => "stdout",
                LogFrame::StdErr(_) => "stderr",
            };
            eprintln!(
                "PG_FRAME|case={case}|stream={stream}|{:?}",
                String::from_utf8_lossy(frame.bytes())
            );
        })
}

async fn inspect_and_query(
    container: ContainerAsync<Postgres>,
    case: &str,
    start_ms: u128,
) -> bool {
    let id = container.id().to_owned();
    eprintln!("PG_STARTED|case={case}|id={id}|wait_ms={start_ms}");
    let expected_image = Command::new("docker")
        .args([
            "--context",
            "orbstack",
            "image",
            "inspect",
            "--format",
            "{{.Id}}|{{.Os}}/{{.Architecture}}",
            "docker.io/library/postgres@sha256:d8703cd7fba306b9fec9268ecedfa8a966846c053036a60e3635791957eb2f66",
        ])
        .output();
    let inspected = Command::new("docker")
        .args([
            "--context",
            "orbstack",
            "inspect",
            "--format",
            "{{.Image}}|{{.Config.Image}}|{{.Config.Tty}}|{{.HostConfig.LogConfig.Type}}|{{.HostConfig.NanoCpus}}|{{.HostConfig.Memory}}|{{.State.Status}}|{{.State.OOMKilled}}|{{index .Config.Labels \"ci.goal\"}}",
            &id,
        ])
        .output();
    let inspect_ok = match (expected_image, inspected) {
        (Ok(expected), Ok(output)) => {
            let expected_details = String::from_utf8_lossy(&expected.stdout).trim().to_owned();
            let details = String::from_utf8_lossy(&output.stdout).trim().to_owned();
            eprintln!(
                "PG_IMAGE|case={case}|exit={:?}|details={expected_details}",
                expected.status.code()
            );
            eprintln!(
                "PG_INSPECT|case={case}|exit={:?}|details={details}",
                output.status.code()
            );
            let expected_fields = expected_details.split('|').collect::<Vec<_>>();
            let fields = details.split('|').collect::<Vec<_>>();
            output.status.success()
                && expected.status.success()
                && expected_fields.len() == 2
                && expected_fields[1] == "linux/amd64"
                && fields.len() == 9
                && fields[0] == expected_fields[0]
                && fields[1].contains(POSTGRES_MANIFEST)
                && fields[2..]
                    == [
                        "false",
                        "json-file",
                        "1000000000",
                        "1073741824",
                        "running",
                        "false",
                        "remove-ci-bottlenecks",
                    ]
        }
        (Err(error), _) | (_, Err(error)) => {
            eprintln!("PG_INSPECT_ERROR|case={case}|error={error}");
            false
        }
    };

    let sql_started = Instant::now();
    let sql_ok = match async {
        let host = container.get_host().await?;
        let port = container.get_host_port_ipv4(5432).await?;
        let url = format!("postgres://postgres:postgres@{host}:{port}/postgres");
        let (client, connection) = tokio_postgres::connect(&url, NoTls).await?;
        let task = tokio::spawn(async move { connection.await });
        let row = client.query_one("SELECT 1", &[]).await?;
        let value: i32 = row.get(0);
        task.abort();
        Ok::<i32, Box<dyn std::error::Error>>(value)
    }
    .await
    {
        Ok(1) => {
            eprintln!(
                "PG_SQL|case={case}|result=1|elapsed_ms={}",
                sql_started.elapsed().as_millis()
            );
            true
        }
        Ok(value) => {
            eprintln!(
                "PG_SQL|case={case}|unexpected={value}|elapsed_ms={}",
                sql_started.elapsed().as_millis()
            );
            false
        }
        Err(error) => {
            eprintln!(
                "PG_SQL_ERROR|case={case}|elapsed_ms={}|error={error}",
                sql_started.elapsed().as_millis()
            );
            false
        }
    };

    let removal_started = Instant::now();
    let removed = container.rm().await;
    eprintln!(
        "PG_REMOVE|case={case}|result={removed:?}|elapsed_ms={}",
        removal_started.elapsed().as_millis()
    );
    inspect_ok && sql_ok && removed.is_ok()
}

async fn probe(case: &'static str, request: ContainerRequest<Postgres>) -> bool {
    let started = Instant::now();
    match request.start().await {
        Ok(container) => inspect_and_query(container, case, started.elapsed().as_millis()).await,
        Err(error) => {
            eprintln!(
                "PG_START_ERROR|case={case}|elapsed_ms={}|error={error:?}",
                started.elapsed().as_millis()
            );
            false
        }
    }
}

#[tokio::test]
async fn compare_default_and_combined_stream_waits() {
    eprintln!(
        "PG_TESTCONTAINERS_COMPARISON|version=0.27.3|modules=0.15.0|digest={POSTGRES_MANIFEST}|timeout=default"
    );
    let default_request = decorate(
        Postgres::default().with_tag("18-alpine"),
        "default-sequential",
    );
    let default_ok = probe("default-sequential", default_request).await;

    let combined_request = decorate(common::postgres_request(), "combined-times-two");
    let combined_ok = probe("combined-times-two", combined_request).await;
    eprintln!("PG_COMPARISON_SUMMARY|default_ok={default_ok}|combined_ok={combined_ok}");
    assert!(combined_ok, "combined stream readiness or SQL probe failed");
}
