use std::fmt::Write as _;

use super::compiled_release_identity;

#[test]
fn compiled_identity_matches_build_time_values() -> Result<(), String> {
    let source = option_env!("VELNOR_COMPILED_SOURCE_SHA")
        .ok_or_else(|| "compiled source value was not emitted".to_owned())?;
    let helper = option_env!("VELNOR_COMPILED_ATTESTATION_HELPER_SHA256")
        .ok_or_else(|| "compiled helper value was not emitted".to_owned())?;
    if source == "unavailable" && helper == "unavailable" {
        if compiled_release_identity().is_some() {
            return Err("local build unexpectedly has release identity".to_owned());
        }
        return Ok(());
    }
    let identity = compiled_release_identity()
        .ok_or_else(|| "configured release identity is unavailable".to_owned())?;
    if lowercase_hex(identity.source_sha())? != source
        || lowercase_hex(identity.helper_sha256())? != helper
    {
        return Err("compiled bytes differ from the validated build values".to_owned());
    }
    Ok(())
}

fn lowercase_hex(bytes: &[u8]) -> Result<String, String> {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(&mut output, "{byte:02x}")
            .map_err(|_| "could not format compiled identity".to_owned())?;
    }
    Ok(output)
}
