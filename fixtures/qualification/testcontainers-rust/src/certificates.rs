use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};
use rcgen::{
    BasicConstraints, CertificateParams, CertifiedIssuer, DistinguishedName, DnType,
    ExtendedKeyUsagePurpose, IsCa, KeyPair, KeyUsagePurpose,
};
use time::{Duration, OffsetDateTime};

pub const CA_NAME: &str = "ca.pem";
pub const SERVER_CERT_NAME: &str = "server.pem";
pub const SERVER_KEY_NAME: &str = "server-key.pem";
const FILES: [&str; 3] = [CA_NAME, SERVER_KEY_NAME, SERVER_CERT_NAME];

#[derive(Debug)]
pub struct BundlePaths {
    pub ca: PathBuf,
    pub server_certificate: PathBuf,
    pub server_key: PathBuf,
}

pub fn generate_bundle(directory: &Path) -> Result<()> {
    validate_directory(directory, true)?;
    let (ca_pem, server_pem, key_pem) = generate_pem()?;
    let contents = [
        (CA_NAME, ca_pem.as_bytes(), 0o644),
        (SERVER_CERT_NAME, server_pem.as_bytes(), 0o644),
        (SERVER_KEY_NAME, key_pem.as_bytes(), 0o600),
    ];
    write_new_files(directory, &contents)
}

pub fn validate_bundle(directory: &Path) -> Result<BundlePaths> {
    validate_directory(directory, false)?;
    let names = fs::read_dir(directory)
        .with_context(|| format!("cannot read certificate directory {}", directory.display()))?
        .map(|entry| entry.map(|item| item.file_name()))
        .collect::<std::io::Result<Vec<_>>>()?;
    let mut names = names;
    names.sort();
    ensure!(
        names.len() == FILES.len()
            && names
                .iter()
                .zip(FILES)
                .all(|(name, expected)| name == expected),
        "certificate directory must contain exactly ca.pem, server.pem, and server-key.pem"
    );
    for name in FILES {
        validate_regular_file(&directory.join(name), name == SERVER_KEY_NAME)?;
    }
    Ok(BundlePaths {
        ca: directory.join(CA_NAME),
        server_certificate: directory.join(SERVER_CERT_NAME),
        server_key: directory.join(SERVER_KEY_NAME),
    })
}

fn generate_pem() -> Result<(String, String, String)> {
    let (not_before, not_after) = validity_window();
    let mut ca_params = CertificateParams::default();
    ca_params.not_before = not_before;
    ca_params.not_after = not_after;
    ca_params.distinguished_name = DistinguishedName::new();
    ca_params
        .distinguished_name
        .push(DnType::CommonName, "Velnor TLS fixture CA");
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    ca_params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    let ca_key = KeyPair::generate().context("could not generate ephemeral CA key")?;
    let ca = CertifiedIssuer::self_signed(ca_params, ca_key)
        .context("could not generate ephemeral CA certificate")?;

    let mut leaf_params = CertificateParams::new(vec!["localhost".to_owned()])?;
    leaf_params.not_before = not_before;
    leaf_params.not_after = not_after;
    leaf_params.distinguished_name = DistinguishedName::new();
    leaf_params
        .distinguished_name
        .push(DnType::CommonName, "localhost");
    leaf_params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    leaf_params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    let leaf_key = KeyPair::generate().context("could not generate ephemeral server key")?;
    let leaf = leaf_params
        .signed_by(&leaf_key, &ca)
        .context("could not sign ephemeral localhost certificate")?;
    Ok((ca.pem(), leaf.pem(), leaf_key.serialize_pem()))
}

fn validity_window() -> (OffsetDateTime, OffsetDateTime) {
    let now = OffsetDateTime::now_utc();
    (now - Duration::minutes(1), now + Duration::hours(1))
}

fn validate_directory(directory: &Path, require_empty: bool) -> Result<()> {
    let metadata = fs::symlink_metadata(directory).with_context(|| {
        format!(
            "certificate directory {} must already exist",
            directory.display()
        )
    })?;
    ensure!(
        metadata.is_dir() && !metadata.file_type().is_symlink(),
        "certificate path must be a real directory"
    );
    ensure!(
        metadata.mode().trailing_zeros() >= 6,
        "certificate directory must not grant group or other access"
    );
    ensure!(
        metadata.mode() & 0o300 == 0o300,
        "certificate directory must be owner-writable and searchable"
    );
    if require_empty {
        ensure!(
            fs::read_dir(directory)?.next().is_none(),
            "certificate output directory must be empty"
        );
    }
    Ok(())
}

fn validate_regular_file(path: &Path, secret: bool) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink(),
        "{} must be a regular file",
        path.display()
    );
    ensure!(metadata.len() > 0, "{} must not be empty", path.display());
    if secret {
        ensure!(
            metadata.mode() & 0o777 == 0o600,
            "server private key must have mode 0600"
        );
    }
    Ok(())
}

fn write_new_files(directory: &Path, files: &[(&str, &[u8], u32)]) -> Result<()> {
    let mut created = Vec::with_capacity(files.len());
    for (name, contents, mode) in files {
        let path = directory.join(name);
        let result = write_new_file(&path, contents, *mode);
        if let Err(error) = result {
            for created_path in created {
                let _ = fs::remove_file(created_path);
            }
            return Err(error).with_context(|| format!("could not write {}", path.display()));
        }
        created.push(path);
    }
    Ok(())
}

fn write_new_file(path: &Path, contents: &[u8], mode: u32) -> Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .open(path)?;
    file.write_all(contents)?;
    file.sync_all()?;
    File::open(path.parent().unwrap_or_else(|| Path::new(".")))?.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use tempfile::tempdir;

    use super::{generate_bundle, validate_bundle};

    #[test]
    fn bundle_is_private_dns_only_and_create_new() -> anyhow::Result<()> {
        let directory = tempdir()?;
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))?;
        generate_bundle(directory.path())?;
        let paths = validate_bundle(directory.path())?;
        assert!(std::fs::read_to_string(paths.ca)?.contains("BEGIN CERTIFICATE"));
        assert!(std::fs::read_to_string(paths.server_certificate)?.contains("BEGIN CERTIFICATE"));
        assert_eq!(
            std::fs::metadata(paths.server_key)?.permissions().mode() & 0o777,
            0o600
        );
        assert!(
            generate_bundle(directory.path()).is_err(),
            "a second invocation must not overwrite PEM files"
        );
        Ok(())
    }
}
