use super::fixture;
use crate::artifact_admission::inspect_release_archive;
use crate::error::HostError;
use serde_json::json;

#[test]
fn release_adapter_rejects_explicit_non_object_descriptor() -> Result<(), HostError> {
    let (archive, manifest) = fixture(None, false)?;
    let identity = super::super::validate(&archive, &manifest)?;
    let config = serde_json::to_value(identity.config).map_err(|_| HostError::Identity)?;
    let classic_inspect = json!([{
        "Id": identity.config_digest,
        "Os": "linux",
        "Architecture": "amd64",
        "Config": config,
    }]);
    let valid = serde_json::to_vec(&classic_inspect).map_err(|_| HostError::Identity)?;
    inspect_release_archive(&valid, &archive, &manifest.source_commit)?;

    let mut invalid_inspect = classic_inspect;
    invalid_inspect[0]["Descriptor"] = serde_json::Value::Null;
    let invalid = serde_json::to_vec(&invalid_inspect).map_err(|_| HostError::Identity)?;
    assert_eq!(
        inspect_release_archive(&invalid, &archive, &manifest.source_commit).err(),
        Some(HostError::Identity)
    );
    Ok(())
}
