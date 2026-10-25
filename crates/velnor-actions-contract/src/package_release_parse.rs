use super::PackageReleaseReceipt;
use crate::ContractError;

pub fn parse_package_release_receipt(bytes: &[u8]) -> Result<PackageReleaseReceipt, ContractError> {
    let value = crate::strict_json::parse_strict_json_bytes(
        bytes,
        crate::strict_json::MAX_UNTRUSTED_DOCUMENT_BYTES,
    )?;
    let receipt: PackageReleaseReceipt = serde_json::from_value(value)
        .map_err(|error| ContractError::identity("package_release", error.to_string()))?;
    receipt.validate()?;
    if receipt.canonical_bytes()?.as_slice() != bytes {
        return Err(ContractError::identity(
            "package_release",
            "noncanonical_json_bytes",
        ));
    }
    Ok(receipt)
}
