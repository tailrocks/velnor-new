use crate::config::KeychainReference;
use crate::error::HostError;

use super::{ReleaseReadConfig, validate_release_read};

fn parse(value: &str) -> Result<ReleaseReadConfig, HostError> {
    toml::from_str(value).map_err(|_| HostError::Config)
}

#[test]
fn release_reader_reference_round_trips_without_secret_data() -> Result<(), HostError> {
    let config = parse("credential_ref = 'keychain:com.tailrocks.velnor.host/release-read'")?;
    assert_eq!(
        config.credential_ref().to_string(),
        "keychain:com.tailrocks.velnor.host/release-read"
    );
    let encoded = toml::to_string(&config).map_err(|_| HostError::Config)?;
    assert!(encoded.contains("credential_ref"));
    assert!(!encoded.contains("token"));
    Ok(())
}

#[test]
fn release_reader_reference_cannot_reuse_a_repository_credential() -> Result<(), HostError> {
    let release_read = parse("credential_ref = 'keychain:service/release-reader'")?;
    let same = KeychainReference::parse("keychain:service/release-reader")?;
    let other = KeychainReference::parse("keychain:service/repository")?;
    assert!(validate_release_read(Some(&release_read), [&same].into_iter()).is_err());
    assert!(validate_release_read(Some(&release_read), [&other].into_iter()).is_ok());
    assert!(validate_release_read(None, std::iter::empty()).is_ok());
    Ok(())
}

#[test]
fn release_reader_config_rejects_secret_fields() {
    assert!(
        parse("credential_ref = 'keychain:service/release-reader'\ntoken = 'secret-value'")
            .is_err()
    );
}
