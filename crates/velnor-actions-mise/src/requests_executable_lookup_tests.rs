use super::*;
use crate::command::ProcessOutput;
use crate::test_temp_dir::TestTempDir;
use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt;

fn output(stdout: Vec<u8>, success: bool, code: Option<i32>) -> ProcessOutput {
    ProcessOutput {
        stdout,
        stderr: b"lookup failed".to_vec(),
        code,
        signal: None,
        success,
    }
}

#[test]
fn mise_lookup_uses_fixed_exact_selector_command() {
    let request = MiseExecutableLookup::mise("jactionlint@2.3.0", "jactionlint")
        .expect("valid catalog tool selector");
    assert_eq!(request.command().program(), "mise");
    assert_eq!(
        request.command().argv(),
        [
            "mise",
            "--no-config",
            "--no-env",
            "--no-hooks",
            "which",
            "jactionlint",
            "--tool",
            "jactionlint@2.3.0",
        ]
        .map(OsString::from)
    );
    assert!(request.command().disables_auto_install());
}

#[test]
fn rust_lookup_hashes_the_selected_toolchain_binary_not_the_shim() {
    let request = MiseExecutableLookup::rust_toolchain("rust@1.99.1", "rustc")
        .expect("valid exact Rust toolchain");
    assert_eq!(
        request.command().argv(),
        [
            "mise",
            "--no-config",
            "--no-env",
            "--no-hooks",
            "exec",
            "rust@1.99.1",
            "--",
            "rustup",
            "which",
            "--toolchain",
            "1.99.1",
            "rustc",
        ]
        .map(OsString::from)
    );
    assert!(MiseExecutableLookup::rust_toolchain("rust@stable", "rustc").is_err());
    assert!(MiseExecutableLookup::rust_toolchain("rust@1.99.1", "cargo-install").is_err());
}

#[test]
fn mise_lookup_preserves_the_catalog_aqua_selector_for_nextest() {
    let request = MiseExecutableLookup::mise(
        "aqua:nextest-rs/nextest/cargo-nextest@0.9.148",
        "cargo-nextest",
    )
    .expect("catalog Aqua selector");
    assert_eq!(
        request.command().argv(),
        [
            "mise",
            "--no-config",
            "--no-env",
            "--no-hooks",
            "which",
            "cargo-nextest",
            "--tool",
            "aqua:nextest-rs/nextest/cargo-nextest@0.9.148",
        ]
        .map(OsString::from)
    );
}

#[test]
fn lookup_rejects_unknown_tools_loose_pins_and_wrong_binary() {
    assert!(MiseExecutableLookup::mise("unknown@1.2.3", "unknown").is_err());
    assert!(MiseExecutableLookup::mise("jactionlint@latest", "jactionlint").is_err());
    assert!(MiseExecutableLookup::mise("jactionlint@1.2", "jactionlint").is_err());
    assert!(MiseExecutableLookup::mise("jactionlint@1.2.3", "cargo").is_err());
    assert!(MiseExecutableLookup::mise("actionlint@1.7.7", "actionlint").is_err());
    assert!(MiseExecutableLookup::mise("zizmor@1.31.1", "zizmor").is_err());
}

#[test]
fn lookup_accepts_only_fixed_runtime_executables_for_project_pins() {
    assert!(MiseExecutableLookup::mise("java@21.0.2", "java").is_ok());
    assert!(MiseExecutableLookup::mise("node@24.21.0", "node").is_ok());
    assert!(MiseExecutableLookup::mise("node@24.21.0", "npm").is_err());
    assert!(MiseExecutableLookup::mise("java@latest", "java").is_err());
    assert!(MiseExecutableLookup::mise("node@../24.21.0", "node").is_err());
    assert!(MiseExecutableLookup::mise("node@22.14", "node").is_err());
    assert!(MiseExecutableLookup::mise("bun@01.2.3", "bun").is_err());
}

#[test]
fn path_parser_rejects_failed_oversized_and_ambiguous_output() {
    assert!(parse_path_output(output(b"/usr/bin/tool\n".to_vec(), false, Some(1))).is_err());
    assert!(parse_path_output(output(vec![b'a'; LOOKUP_OUTPUT_LIMIT + 1], true, Some(0))).is_err());
    for bytes in [
        b"relative/tool\n".as_slice(),
        b"/usr/bin/../tool\n".as_slice(),
        b"/usr/bin/tool\nextra\n".as_slice(),
        b"/usr/bin/tool\r\n".as_slice(),
    ] {
        assert!(parse_path_output(output(bytes.to_vec(), true, Some(0))).is_err());
    }
    assert_eq!(
        parse_path_output(output(b"/usr/bin/tool\n".to_vec(), true, Some(0)))
            .expect("one absolute path"),
        PathBuf::from("/usr/bin/tool")
    );
}

#[test]
fn executable_hash_requires_owned_regular_file_without_symlink_escape() {
    let root = TestTempDir::new("velnor-mise-executable").expect("temporary root");
    let install = root.path().join("install");
    let bin = install.join("bin");
    std::fs::create_dir_all(&bin).expect("install bin");
    let executable = bin.join("actionlint");
    std::fs::write(&executable, b"trusted executable bytes").expect("write executable");
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))
        .expect("executable mode");

    let canonical_executable = executable.canonicalize().expect("canonical executable");
    let canonical_install = install.canonicalize().expect("canonical install root");
    let digest =
        hash_owned_executable(&canonical_executable, &canonical_install).expect("owned executable");
    assert_eq!(
        digest,
        "a6414cc5bc245cf962d9bafbe04a29effb5ae29322e3828999bb665bedb7e866"
    );
    assert!(hash_owned_executable(&canonical_executable, root.path()).is_ok());

    let outside = root.path().join("outside");
    std::fs::write(&outside, b"outside bytes").expect("write outside file");
    std::fs::set_permissions(&outside, std::fs::Permissions::from_mode(0o700))
        .expect("outside executable mode");
    let escape = bin.join("escape");
    std::os::unix::fs::symlink(&outside, &escape).expect("symlink");
    assert!(hash_owned_executable(&escape, &canonical_install).is_err());

    let internal_link = bin.join("internal-link");
    std::os::unix::fs::symlink(&executable, &internal_link).expect("internal symlink");
    assert!(hash_owned_executable(&internal_link, &canonical_install).is_err());
}
