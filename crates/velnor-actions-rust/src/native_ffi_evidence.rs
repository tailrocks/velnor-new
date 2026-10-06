//! Native Cargo producer evidence belongs to Rust, independently of Swift.
use velnor_actions_contract::{
    ContractError, FileIndex, config::RustFfiProfile, normalize_posix_path,
};

/// Exact source paths required by the locked native Rust producer.
/// # Errors
/// Rejects invalid producer identities or nonnormalized source roots.
pub fn native_ffi_required_inputs(
    ffi: &RustFfiProfile,
    source_root: &str,
) -> Result<Vec<String>, ContractError> {
    ffi.validate(".velnor/config.toml", "native.ffi")?;
    if source_root != "." && normalize_posix_path(source_root)? != source_root {
        return Err(ContractError::identity(
            "rust_native",
            "invalid_native_source_root",
        ));
    }
    let parent = ffi
        .manifest_path
        .rsplit_once('/')
        .map_or("", |(parent, _)| parent);
    let boltffi = if parent.is_empty() {
        "boltffi.toml".to_owned()
    } else {
        format!("{parent}/boltffi.toml")
    };
    Ok(
        ["Cargo.lock".to_owned(), ffi.manifest_path.clone(), boltffi]
            .into_iter()
            .map(|path| {
                if source_root == "." {
                    path
                } else {
                    format!("{source_root}/{path}")
                }
            })
            .collect(),
    )
}

/// Validate producer-owned Cargo/lock/BoltFFI evidence before proposing work.
/// # Errors
/// Rejects unsafe producer inputs and any missing fixed source requirement.
pub fn validate_native_ffi_evidence(
    ffi: &RustFfiProfile,
    source_root: &str,
    index: &FileIndex,
) -> Result<(), ContractError> {
    for path in native_ffi_required_inputs(ffi, source_root)? {
        if !index.contains(&path) {
            return Err(ContractError::identity(
                "rust_native",
                format!("native_ffi_evidence_missing:{path}"),
            ));
        }
    }
    Ok(())
}
