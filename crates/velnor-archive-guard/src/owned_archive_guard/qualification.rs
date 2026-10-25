#[path = "qualification/bindings.rs"]
mod bindings;
#[path = "qualification/filesystem.rs"]
mod filesystem;
#[path = "qualification/manifest.rs"]
mod manifest;

const MAX_MANIFEST_BYTES: u64 = 8 * 1024 * 1024;
const MAX_WORKFLOW_FILE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_WORKFLOW_TREE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_WORKFLOW_FILES: usize = 10_000;
const MAX_WORKFLOW_ENTRIES: usize = 20_000;
const MAX_WORKFLOW_DEPTH: usize = 128;
const EXPECTED_TARGETS: [&str; 3] = [
    "x86_64-unknown-linux-gnu",
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
];

pub fn validate_qualification_manifest(path: &std::path::Path) -> Result<(), String> {
    manifest::read(path)
        .map(|_| ())
        .map_err(|error| format!("candidate manifest validation: {error}"))
}

pub fn validate_qualification_output_bindings(
    manifest_path: &std::path::Path,
    workflow_root: &std::path::Path,
) -> Result<(), String> {
    bindings::validate(manifest_path, workflow_root)
        .map_err(|error| format!("candidate output binding mismatch: {error}"))
}
