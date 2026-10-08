use std::os::unix::fs::PermissionsExt;
use tempfile::TempDir;

use crate::worker::cleanup::DiagnosticsStore;

pub(in crate::worker::cleanup::tests) fn diagnostics_store(
    directory: &TempDir,
) -> Result<DiagnosticsStore, String> {
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))
        .map_err(|error| error.to_string())?;
    DiagnosticsStore::new(directory.path()).map_err(|error| error.to_string())
}

pub(super) fn empty_diagnostic_tar() -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut builder = tar::Builder::new(&mut bytes);
    let contents = b"runner stopped\n";
    let mut header = tar::Header::new_gnu();
    header.set_entry_type(tar::EntryType::Regular);
    header.set_size(u64::try_from(contents.len()).expect("test log length fits u64"));
    header.set_mode(0o600);
    header.set_cksum();
    builder
        .append_data(&mut header, "Runner_1.log", contents.as_slice())
        .expect("append log");
    builder.finish().expect("finish tar");
    drop(builder);
    bytes
}
