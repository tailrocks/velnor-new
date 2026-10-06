use super::*;

/// Scratch dir unique to this process plus the test name.
fn scratch(test: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("velnor-preseed-{test}-{}", std::process::id()));
    drop(std::fs::remove_dir_all(&dir));
    std::fs::create_dir_all(&dir).expect("scratch");
    dir
}

/// Valid writer inputs over a scratch tree with a fixture binary.
fn valid_inputs(
    root: &std::path::Path,
) -> (String, String, String, String, String, std::path::PathBuf) {
    let binary = root.join("velnor-actions");
    std::fs::write(&binary, "helper-bytes\n").expect("fixture");
    let temp = root.join("temp");
    let out = temp.join("velnor").join("preseed-output");
    (
        binary.display().to_string(),
        out.display().to_string(),
        "x86_64-unknown-linux-gnu".to_owned(),
        "rust@1.98.1".to_owned(),
        "a".repeat(40),
        temp,
    )
}

/// Writer emits the exact §4.4 shape with a real digest.
///
/// The digest below is the system `shasum -a 256` of the fixture
/// bytes, not this module's own output: an independent oracle.
mod preseed_manifest_tests;
