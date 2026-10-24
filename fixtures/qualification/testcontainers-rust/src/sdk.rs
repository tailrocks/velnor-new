use std::{env, ffi::OsStr, fs, path::Path, time::Duration};

use anyhow::{Context, Result, anyhow, ensure};
use testcontainers::{
    CopyTargetOptions, GenericImage, ImageExt,
    core::{CopyDataSource, IntoContainerPort, WaitFor},
    runners::AsyncRunner,
};
use tokio::time::timeout;
use uuid::Uuid;

use crate::{certificates, client};

const IMAGE_NAME: &str = "ghcr.io/tailrocks/velnor-job@sha256";
const IMAGE_DIGEST: &str = "b36946574402eb0c79fb6d7a9f7a77195d2ef943c92bc6b0fbc466ff3ea408f9";
const SERVER_PORT: u16 = 8443;
const STARTUP_TIMEOUT: Duration = Duration::from_secs(30);
const REQUEST_GROUP_TIMEOUT: Duration = Duration::from_secs(20);
const CLEANUP_TIMEOUT: Duration = Duration::from_secs(10);
const READY_MARKER: &str = "velnor-tls-listening";
const SERVER_SOURCE: &[u8] =
    include_bytes!("../../../../qualification/testcontainers-tls/server.mjs");
const SERVER_TARGET: &str = "/tmp/velnor-tls-server.mjs";
const CERT_TARGET: &str = "/tmp/velnor-tls-server.pem";
const KEY_TARGET: &str = "/tmp/velnor-tls-server-key.pem";

pub async fn run_qualification(certificate_directory: &Path) -> Result<()> {
    validate_docker_environment()?;
    let bundle = certificates::validate_bundle(certificate_directory)?;
    let ca_pem = fs::read(&bundle.ca).context("could not read private test CA")?;
    let nonce = make_nonce();
    let container = start_server(&bundle, &nonce).await?;

    let operation = timeout(
        REQUEST_GROUP_TIMEOUT,
        run_container_checks(&container, &ca_pem, &nonce),
    )
    .await
    .map_err(|_| anyhow!("container HTTPS checks exceeded their overall deadline"))
    .and_then(|result| result);
    let cleanup = timeout(CLEANUP_TIMEOUT, container.rm()).await;
    match (operation, cleanup) {
        (Ok(()), Ok(Ok(()))) => {
            println!("testcontainers-tls-ok");
            println!("testcontainers-tls-rejected-untrusted-ca");
            println!("testcontainers-tls-rejected-wrong-dns");
            println!("testcontainers-tls-rejected-ip");
            Ok(())
        }
        (Err(operation_error), Ok(Ok(()))) => Err(operation_error),
        (Ok(()), Ok(Err(cleanup_error))) => {
            Err(cleanup_error).context("owned TLS container removal failed")
        }
        (Err(operation_error), Ok(Err(cleanup_error))) => Err(anyhow!(
            "TLS checks failed ({operation_error:#}); owned container removal also failed ({cleanup_error})"
        )),
        (operation_result, Err(_)) => Err(anyhow!(
            "owned TLS container removal exceeded its deadline after operation result: {}",
            operation_result.map_or_else(
                |error| format!("failed ({error:#})"),
                |()| "passed".to_owned()
            )
        )),
    }
}

async fn start_server(
    bundle: &certificates::BundlePaths,
    nonce: &str,
) -> Result<testcontainers::ContainerAsync<GenericImage>> {
    GenericImage::new(IMAGE_NAME, IMAGE_DIGEST)
        .with_exposed_port(SERVER_PORT.tcp())
        .with_copy_to(
            CopyTargetOptions::new(SERVER_TARGET).with_mode(0o644),
            server_copy_source(),
        )
        .with_copy_to(
            CopyTargetOptions::new(CERT_TARGET).with_mode(0o644),
            CopyDataSource::File(bundle.server_certificate.clone()),
        )
        .with_copy_to(
            CopyTargetOptions::new(KEY_TARGET).with_mode(0o600),
            CopyDataSource::File(bundle.server_key.clone()),
        )
        .with_env_var("VELNOR_TLS_NONCE", nonce)
        .with_cmd(["node", SERVER_TARGET])
        .with_ready_conditions(vec![WaitFor::message_on_stdout(READY_MARKER)])
        .with_startup_timeout(STARTUP_TIMEOUT)
        .start()
        .await
        .context("could not start TLS server in the pinned Velnor job image")
}

async fn run_container_checks(
    container: &testcontainers::ContainerAsync<GenericImage>,
    ca_pem: &[u8],
    nonce: &str,
) -> Result<()> {
    let host = container
        .get_host()
        .await
        .context("could not get mapped service host")?;
    ensure!(
        host.to_string() == "localhost",
        "Testcontainers mapped host must be localhost"
    );
    let port = container
        .get_host_port_ipv4(SERVER_PORT.tcp())
        .await
        .context("could not get mapped HTTPS service port")?;
    let tls_client = client::client(ca_pem, None)?;
    client::check_tls_endpoints(&tls_client, ca_pem, &host.to_string(), port, nonce).await
}

fn make_nonce() -> String {
    format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

fn server_copy_source() -> CopyDataSource {
    CopyDataSource::Data(SERVER_SOURCE.to_vec())
}

fn validate_docker_environment() -> Result<()> {
    ensure!(
        env::var_os("TESTCONTAINERS_HOST_OVERRIDE").is_none(),
        "TESTCONTAINERS_HOST_OVERRIDE must be unset for localhost qualification"
    );
    ensure!(
        env::var_os("DOCKER_CONTEXT").is_none(),
        "DOCKER_CONTEXT must be unset for localhost qualification"
    );
    ensure!(
        env::var_os("DOCKER_HOST").as_deref() == Some(OsStr::new("tcp://localhost:2375")),
        "DOCKER_HOST must select the paired localhost Docker service"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use testcontainers::{ContainerRequest, GenericImage, core::CopyDataSource};

    use super::{IMAGE_DIGEST, IMAGE_NAME, SERVER_SOURCE};

    #[test]
    fn embedded_server_source_is_copied_as_data_for_relocated_binaries() -> std::io::Result<()> {
        let embedded = super::server_copy_source();
        let source = match embedded {
            CopyDataSource::Data(source) => source,
            CopyDataSource::File(_) => Vec::new(),
        };
        let shared_source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../qualification/testcontainers-tls/server.mjs");
        assert_eq!(source.as_slice(), SERVER_SOURCE);
        assert_eq!(source, std::fs::read(shared_source)?);
        let has_server_contract = std::str::from_utf8(&source).is_ok_and(|source| {
            source.contains("server.listen(TLS_PORT, \"0.0.0.0\"")
                && source.contains("console.log(\"velnor-tls-listening\")")
                && source.contains("request.url === \"/nonce\"")
        });
        assert!(
            has_server_contract,
            "embedded server payload must retain the TLS fixture contract"
        );
        Ok(())
    }

    #[test]
    fn image_reference_is_the_existing_digest_pinned_job_image() {
        let image = ContainerRequest::from(GenericImage::new(IMAGE_NAME, IMAGE_DIGEST));
        assert_eq!(image.descriptor(), format!("{IMAGE_NAME}:{IMAGE_DIGEST}"));
        assert_eq!(
            image.descriptor(),
            "ghcr.io/tailrocks/velnor-job@sha256:b36946574402eb0c79fb6d7a9f7a77195d2ef943c92bc6b0fbc466ff3ea408f9"
        );
    }
}
