use std::{ffi::OsString, fs, io::Read, path::Path, time::Duration};

use sha2::{Digest, Sha256};

pub use crate::catalog::{COSIGN_LINUX_AMD64_SHA256, COSIGN_VERSION};
use crate::{IsolatedCommand, MiseError, command::EnvPolicy};
use velnor_actions_contract::vcs::is_safe_github_repository;

pub const COSIGN_MAX_BYTES: usize = crate::catalog::COSIGN_LINUX_AMD64_BYTES;
pub const COSIGN_COMMAND_OUTPUT_LIMIT: usize = 64 * 1024;
pub const COSIGN_COMMAND_TIMEOUT: Duration = Duration::from_secs(180);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CosignArtifact {
    pub name: &'static str,
    pub sha256: &'static str,
    pub bytes: usize,
}

pub fn cosign_artifact() -> Result<CosignArtifact, MiseError> {
    cosign_artifact_for(std::env::consts::OS, std::env::consts::ARCH)
}

fn cosign_artifact_for(os: &str, arch: &str) -> Result<CosignArtifact, MiseError> {
    use crate::catalog::{
        COSIGN_DARWIN_AMD64_BYTES, COSIGN_DARWIN_AMD64_SHA256, COSIGN_DARWIN_ARM64_BYTES,
        COSIGN_DARWIN_ARM64_SHA256, COSIGN_LINUX_AMD64_BYTES, COSIGN_LINUX_ARM64_BYTES,
        COSIGN_LINUX_ARM64_SHA256,
    };
    let (name, sha256, bytes) = match (os, arch) {
        ("linux", "x86_64") => (
            "cosign-linux-amd64",
            COSIGN_LINUX_AMD64_SHA256,
            COSIGN_LINUX_AMD64_BYTES,
        ),
        ("linux", "aarch64") => (
            "cosign-linux-arm64",
            COSIGN_LINUX_ARM64_SHA256,
            COSIGN_LINUX_ARM64_BYTES,
        ),
        ("macos", "x86_64") => (
            "cosign-darwin-amd64",
            COSIGN_DARWIN_AMD64_SHA256,
            COSIGN_DARWIN_AMD64_BYTES,
        ),
        ("macos", "aarch64") => (
            "cosign-darwin-arm64",
            COSIGN_DARWIN_ARM64_SHA256,
            COSIGN_DARWIN_ARM64_BYTES,
        ),
        _ => return Err(invalid("cosign_platform_unsupported")),
    };
    Ok(CosignArtifact {
        name,
        sha256,
        bytes,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CosignCertificateIdentity {
    repository: String,
    workflow_path: String,
    source_ref: String,
    workflow_sha: String,
    trigger: String,
}

impl CosignCertificateIdentity {
    pub fn new(
        repository: &str,
        workflow_path: &str,
        source_ref: &str,
        workflow_sha: &str,
        trigger: &str,
    ) -> Result<Self, MiseError> {
        if !is_safe_github_repository(repository)
            || !workflow_path.starts_with(".github/workflows/")
            || !workflow_path
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._/-".contains(&byte))
            || workflow_path.contains("..")
            || !source_ref.starts_with("refs/heads/")
            || !source_ref
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._/-".contains(&byte))
            || !is_sha(workflow_sha)
            || !matches!(trigger, "workflow_run" | "workflow_dispatch")
        {
            return Err(invalid("certificate_identity"));
        }
        Ok(Self {
            repository: repository.to_owned(),
            workflow_path: workflow_path.to_owned(),
            source_ref: source_ref.to_owned(),
            workflow_sha: workflow_sha.to_owned(),
            trigger: trigger.to_owned(),
        })
    }

    fn certificate_identity(&self) -> String {
        format!(
            "https://github.com/{}/{}@{}",
            self.repository, self.workflow_path, self.source_ref
        )
    }
}

#[derive(Debug, Clone)]
pub struct PinnedCosign {
    binary: String,
}

impl PinnedCosign {
    pub fn from_verified_path(binary: &Path) -> Result<Self, MiseError> {
        cosign_artifact()?;
        if !binary.is_absolute() {
            return Err(invalid("cosign_path_not_absolute"));
        }
        verify_cosign_bytes(binary)?;
        let binary = binary
            .to_str()
            .ok_or_else(|| invalid("cosign_path_not_utf8"))?;
        Ok(Self {
            binary: binary.to_owned(),
        })
    }

    pub fn download_command() -> Result<IsolatedCommand, MiseError> {
        let artifact = cosign_artifact()?;
        let url = format!(
            "https://github.com/sigstore/cosign/releases/download/v{COSIGN_VERSION}/{}",
            artifact.name
        );
        let args = vec![
            OsString::from("--disable"),
            OsString::from("--fail"),
            OsString::from("--silent"),
            OsString::from("--show-error"),
            OsString::from("--location"),
            OsString::from("--proto"),
            OsString::from("=https"),
            OsString::from("--proto-redir"),
            OsString::from("=https"),
            OsString::from("--tlsv1.2"),
            OsString::from("--max-time"),
            OsString::from("120"),
            OsString::from("--max-filesize"),
            OsString::from(artifact.bytes.to_string()),
            OsString::from(url),
        ];
        IsolatedCommand::repo_task("/usr/bin/curl", args, &[])
    }

    pub fn sign_blob_command(
        &self,
        subject: &Path,
        bundle: &Path,
    ) -> Result<IsolatedCommand, MiseError> {
        let command = self.command(vec![
            OsString::from("sign-blob"),
            OsString::from("--yes"),
            OsString::from("--bundle"),
            path_arg(bundle)?,
            path_arg(subject)?,
        ])?;
        command.with_github_actions_oidc()
    }

    pub fn verify_blob_command(
        &self,
        subject: &Path,
        bundle: &Path,
        identity: &CosignCertificateIdentity,
    ) -> Result<IsolatedCommand, MiseError> {
        let cert_identity = identity.certificate_identity();
        self.command(vec![
            OsString::from("verify-blob"),
            OsString::from("--bundle"),
            path_arg(bundle)?,
            OsString::from("--certificate-identity"),
            OsString::from(cert_identity),
            OsString::from("--certificate-oidc-issuer"),
            OsString::from("https://token.actions.githubusercontent.com"),
            OsString::from("--certificate-github-workflow-repository"),
            OsString::from(identity.repository.as_str()),
            OsString::from("--certificate-github-workflow-ref"),
            OsString::from(identity.source_ref.as_str()),
            OsString::from("--certificate-github-workflow-sha"),
            OsString::from(identity.workflow_sha.as_str()),
            OsString::from("--certificate-github-workflow-trigger"),
            OsString::from(identity.trigger.as_str()),
            path_arg(subject)?,
        ])
    }

    fn command(&self, args: Vec<OsString>) -> Result<IsolatedCommand, MiseError> {
        IsolatedCommand::repo_task(&self.binary, args, &[])
            .map(|command| command.with_policy(EnvPolicy::ReleaseSigner))
    }
}

fn path_arg(path: &Path) -> Result<OsString, MiseError> {
    if !path.is_absolute() {
        return Err(invalid("cosign_path_not_absolute"));
    }
    Ok(path.as_os_str().to_owned())
}

fn verify_cosign_bytes(path: &Path) -> Result<(), MiseError> {
    let metadata = fs::metadata(path).map_err(|_| invalid("cosign_file_missing"))?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > COSIGN_MAX_BYTES as u64 {
        return Err(invalid("cosign_file_invalid"));
    }
    let mut file = fs::File::open(path).map_err(|_| invalid("cosign_file_read_failed"))?;
    verify_cosign_reader(&mut file)
}

fn verify_cosign_reader(reader: &mut impl Read) -> Result<(), MiseError> {
    verify_cosign_reader_for(reader, cosign_artifact()?)
}

fn verify_cosign_reader_for(
    reader: &mut impl Read,
    artifact: CosignArtifact,
) -> Result<(), MiseError> {
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024].into_boxed_slice();
    let mut size = 0_u64;
    loop {
        let count = reader
            .read(&mut buffer)
            .map_err(|_| invalid("cosign_file_read_failed"))?;
        if count == 0 {
            break;
        }
        size = size.saturating_add(count as u64);
        if size > artifact.bytes as u64 {
            return Err(invalid("cosign_file_too_large"));
        }
        hasher.update(&buffer[..count]);
    }
    let mut digest = String::with_capacity(64);
    for byte in hasher.finalize() {
        digest.push(char::from(b"0123456789abcdef"[usize::from(byte >> 4)]));
        digest.push(char::from(b"0123456789abcdef"[usize::from(byte & 0x0f)]));
    }
    if size == artifact.bytes as u64 && digest == artifact.sha256 {
        Ok(())
    } else {
        Err(invalid("cosign_sha256_mismatch"))
    }
}

fn is_sha(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn invalid(value: &str) -> MiseError {
    MiseError::InvalidStepInput {
        field: "package_release_signer".to_owned(),
        value: value.to_owned(),
    }
}
