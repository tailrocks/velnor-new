use super::check_file;

#[path = "impl_mise_forbidden_src_executable_lookup_tests.rs"]
mod executable_lookup;

#[test]
fn bounded_file_filter_exceptions_are_exact_and_local() -> Result<(), String> {
    let allowed_write = "fn copy_from_child(output: &mut impl std::io::Write, buffer: &[u8], count: usize) { output\n.write_all(&buffer[..count])\n.map_err(std::io::Error::other)?; }";
    let altered_write = "fn copy_from_child() { other\n.write_all(&buffer[..count])?; }";
    let extra_write =
        "fn copy_from_child() { output.write_all(&buffer[..count])?; output.write_all(b\"x\")?; }";
    let allowed_metadata = "fn check() { let m = std::fs::symlink_metadata(program)?; }";
    let extra_metadata =
        "fn check() { std::fs::symlink_metadata(program)?; std::fs::symlink_metadata(other)?; }";
    let create_link = "fn bad() { std::os::unix::fs::symlink(source, target)?; }";
    let path_open_alias = "use std::fs::File as Stage; fn bad(path: &std::path::Path) { let mut output = Stage::options().create(true).write(true).truncate(true).open(path)?; output.write_all(&buffer[..count])?; }";
    let options_alias = "use std::fs::OpenOptions as StageOptions; fn bad(path: &std::path::Path) { let mut output = StageOptions::new().write(true).open(path)?; output.write_all(&buffer[..count])?; }";
    let rustix_open_alias = "use rustix::fs::open as stage_open; fn bad(path: &std::path::Path) { let output = stage_open(path, flags, mode)?; output.write_all(&buffer[..count])?; }";
    let allowed_stream = "use std::io::Write; fn sink(output: &mut std::fs::File, bytes: &[u8]) -> std::io::Result<()> { output\n.write_all(bytes)\n.map_err(std::io::Error::other)?;\nOk(()) }";
    let altered_stream = "use std::io::Write; fn sink(other: &mut std::fs::File, bytes: &[u8]) -> std::io::Result<()> { other\n.write_all(bytes) }";

    let mut violations = Vec::new();
    check_source_fixture(
        "bounded_file_filter_process.rs",
        allowed_write,
        &mut violations,
    )?;
    assert!(violations.is_empty(), "{violations:?}");

    violations.clear();
    check_source_fixture(
        "bounded_file_filter_process.rs",
        altered_write,
        &mut violations,
    )?;
    assert_ne!(violations, Vec::<String>::new());

    violations.clear();
    check_source_fixture(
        "bounded_file_filter_process.rs",
        extra_write,
        &mut violations,
    )?;
    assert_ne!(violations, Vec::<String>::new());

    for path_open in [path_open_alias, options_alias, rustix_open_alias] {
        violations.clear();
        check_source_fixture("bounded_file_filter_process.rs", path_open, &mut violations)?;
        assert!(!violations.is_empty(), "path opener passed: {path_open}");
    }

    violations.clear();
    check_source_fixture("command_stream.rs", allowed_stream, &mut violations)?;
    assert!(violations.is_empty(), "{violations:?}");

    for source in [
        altered_stream,
        "use std::io::Write; fn sink(output: &mut std::fs::File, bytes: &[u8]) -> std::io::Result<()> { output.write_all(bytes)?; output.write_all(b\"extra\") }",
    ] {
        violations.clear();
        check_source_fixture("command_stream.rs", source, &mut violations)?;
        assert!(!violations.is_empty(), "unapproved stream write passed");
    }

    violations.clear();
    check_source_fixture("bounded_file_filter.rs", allowed_metadata, &mut violations)?;
    assert!(violations.is_empty(), "{violations:?}");

    violations.clear();
    check_source_fixture("bounded_file_filter.rs", extra_metadata, &mut violations)?;
    assert_ne!(violations, Vec::<String>::new());

    violations.clear();
    check_source_fixture("bounded_file_filter.rs", create_link, &mut violations)?;
    assert_ne!(violations, Vec::<String>::new());

    violations.clear();
    check_source_fixture(
        "bounded_file_filter_tests.rs",
        allowed_write,
        &mut violations,
    )?;
    assert_ne!(violations, Vec::<String>::new());

    for mutation in [
        "std::fs::write(path, bytes)",
        "File::create(path)",
        "std::fs::create_dir(path)",
        "std::fs::rename(old, new)",
        "std::fs::remove_file(path)",
        "std::fs::set_permissions(path, permissions)",
        "std::os::unix::fs::hard_link(source, target)",
    ] {
        violations.clear();
        check_source_fixture("bounded_file_filter.rs", mutation, &mut violations)?;
        assert!(!violations.is_empty(), "mutation passed: {mutation}");
    }
    Ok(())
}

pub(super) fn check_source_fixture(
    name: &str,
    source: &str,
    violations: &mut Vec<String>,
) -> Result<(), String> {
    let directory = std::env::temp_dir().join(format!(
        "velnor-mise-source-policy-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_nanos()
    ));
    std::fs::create_dir(&directory).map_err(|error| error.to_string())?;
    let path = directory.join(name);
    let check = (|| {
        std::fs::write(&path, source).map_err(|error| error.to_string())?;
        check_file(&path, violations)
    })();
    let cleanup = std::fs::remove_dir_all(directory).map_err(|error| error.to_string());
    check?;
    cleanup
}

#[test]
fn actual_checker_rejects_compiling_path_open_fixture() -> Result<(), String> {
    let fixture = r"
use std::io::Write;
fn arbitrary_path_output(path: &std::path::Path, buffer: &[u8], count: usize) -> std::io::Result<()> {
    let mut output = std::fs::File::options().create(true).write(true).truncate(true).open(path)?;
    output
        .write_all(&buffer[..count])
        .map_err(std::io::Error::other)?;
    Ok(())
}
";
    let mut violations = Vec::new();
    check_source_fixture("bounded_file_filter_process.rs", fixture, &mut violations)?;
    assert!(
        !violations.is_empty(),
        "actual checker accepted path-created output"
    );
    Ok(())
}

#[test]
fn actual_checker_rejects_cross_module_rustix_output_openers() -> Result<(), String> {
    let combined = r"
mod cache {
pub fn destination(path: &std::path::Path) -> std::io::Result<std::fs::File> {
 let fd = rustix::fs::open(path, rustix::fs::OFlags::WRONLY | rustix::fs::OFlags::CREATE | rustix::fs::OFlags::TRUNC, rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR)?;
 Ok(std::fs::File::from(fd))
}
}
use std::io::Write;
fn arbitrary_path_output(path: &std::path::Path, buffer: &[u8], count: usize) -> std::io::Result<()> {
 let mut output = crate::cache::destination(path)?;
 output
 .write_all(&buffer[..count])
 .map_err(std::io::Error::other)?;
 Ok(())
}
";
    let mut violations = Vec::new();
    check_source_fixture("cache.rs", combined, &mut violations)?;
    assert!(!violations.is_empty(), "cross-module opener passed");

    violations.clear();
    check_source_fixture(
        "cache.rs",
        include_str!("fixtures/mise-policy-rustix-aliases.rs"),
        &mut violations,
    )?;
    assert!(!violations.is_empty(), "aliased Rustix openers passed");

    for aliased_opener in [
        "use rustix::fs::open as stage_open; fn bad(path: &std::path::Path) { let fd = stage_open(path, flags, mode)?; }",
        "use rustix::fs::{open as stage_open}; fn bad(path: &std::path::Path) { let fd = stage_open(path, flags, mode)?; }",
        "use rustix::fs as stage_fs; fn bad(path: &std::path::Path) { let fd = stage_fs::open(path, flags, mode)?; }",
        "use rustix as rx; fn bad(path: &std::path::Path) { let fd = rx::fs::open(path, flags, mode)?; }",
        "fn bad(path: &std::path::Path) { let fd = rustix::fs::openat(dir, path, flags, mode)?; }",
        "fn bad(path: &std::ffi::CStr) { unsafe { libc::open(path.as_ptr(), flags) }; }",
        "use std::fs as file_ops; fn bad(path: &std::path::Path) { file_ops::write(path, b\"x\")?; }",
        "use std::{fs as file_ops}; fn bad(path: &std::path::Path) { file_ops::write(path, b\"x\")?; }",
    ] {
        violations.clear();
        check_source_fixture("cache.rs", aliased_opener, &mut violations)?;
        assert!(
            !violations.is_empty(),
            "aliased opener passed: {aliased_opener}"
        );
    }

    let read_only = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/check_file_read.rs"),
    )
    .map_err(|error| error.to_string())?;
    violations.clear();
    check_source_fixture("check_file_read.rs", &read_only, &mut violations)?;
    assert!(
        violations.is_empty(),
        "read-only opener failed: {violations:?}"
    );

    for altered in [
        read_only.replace("OFlags::RDONLY", "OFlags::WRONLY"),
        read_only.replace(
            "OFlags::RDONLY",
            "OFlags::RDONLY | rustix::fs::OFlags::CREATE",
        ),
        read_only.replace("OFlags::NOFOLLOW", "OFlags::CLOEXEC"),
    ] {
        violations.clear();
        check_source_fixture("check_file_read.rs", &altered, &mut violations)?;
        assert!(!violations.is_empty(), "altered read flags passed");
    }
    Ok(())
}

#[test]
fn actual_checker_rejects_structured_import_capabilities() -> Result<(), String> {
    let grouped = include_str!("fixtures/mise-policy-grouped-rustix-aliases.rs");
    let mut violations = Vec::new();
    check_source_fixture("cache.rs", grouped, &mut violations)?;
    assert!(!violations.is_empty(), "grouped Rustix alias passed");

    for source in [
        "use {rustix::fs as stage_ops, std::fs::File}; fn bad(path: &std::path::Path) { let fd = stage_ops::open(path, flags, mode)?; }",
        "use {rustix::{self as stage_root, fs as stage_ops}}; fn bad(path: &std::path::Path) { let fd = stage_ops::open(path, flags, mode)?; }",
        "use std::fs::{self as file_ops}; fn bad(path: &std::path::Path) { file_ops::write(path, b\"x\")?; }",
        "use std as standard; fn bad(path: &std::path::Path) { standard::fs::write(path, b\"x\")?; }",
        "use std::fs::File as StageFile; fn bad(path: &std::path::Path) { StageFile::create(path)?; }",
        "type StageFile = std::fs::File; fn bad(path: &std::path::Path) { StageFile::create(path)?; }",
        "use rustix::fs::open as stage_open; fn bad(path: &std::path::Path) { stage_open(path, flags, mode)?; }",
        "use rustix::fs::*; fn bad(path: &std::path::Path) { open(path, flags, mode)?; }",
        "use rustix as stage_root; fn bad(path: &std::path::Path) { stage_root::fs::open(path, flags, mode)?; }",
        "use rustix::{self as stage_root}; fn bad(path: &std::path::Path) { stage_root::fs::open(path, flags, mode)?; }",
        "use std::fs::File as StageFile; type Output = StageFile; fn bad(path: &std::path::Path) { Output::create(path)?; }",
        "use std::fs::File as StageFile; fn bad(path: &std::path::Path) { let create = StageFile::create; create(path)?; }",
        "use std::fs::*; fn bad(path: &std::path::Path) { File::create(path)?; }",
        "macro_rules! hidden_open { ($path:expr) => { rustix::fs::open($path, flags, mode) } }",
    ] {
        violations.clear();
        check_source_fixture("cache.rs", source, &mut violations)?;
        assert!(
            !violations.is_empty(),
            "structured path-opening capability passed: {source}"
        );
    }

    for source in [
        "use std::fs::File as ReadFile; fn read(path: &std::path::Path) { ReadFile::open(path)?; }",
        "use std::fs as file_ops; fn read(path: &std::path::Path) { file_ops::File::open(path)?; }",
    ] {
        violations.clear();
        check_source_fixture("cache.rs", source, &mut violations)?;
        assert!(
            violations.is_empty(),
            "read-only standard open failed: {violations:?}"
        );
    }

    violations.clear();
    check_source_fixture(
        "cache.rs",
        include_str!("fixtures/mise-policy-file-aliases.rs"),
        &mut violations,
    )?;
    assert!(!violations.is_empty(), "type and function aliases passed");

    violations.clear();
    check_source_fixture(
        "bounded_file_filter_process.rs",
        "fn copy_from_child(output: &mut impl std::io::Write, buffer: &[u8], count: usize) {\n    output\n        .write_all(&buffer[..count])\n        .map_err(std::io::Error::other)?;\n}",
        &mut violations,
    )?;
    assert!(
        violations.is_empty(),
        "supplied-writer output failed: {violations:?}"
    );
    Ok(())
}

#[test]
fn actual_checker_rejects_scoped_alias_and_qualified_type_bypasses() -> Result<(), String> {
    for (label, source) in [
        (
            "nested alias shadowing",
            include_str!("fixtures/mise-policy-scope-shadow.rs"),
        ),
        (
            "qualified type call",
            include_str!("fixtures/mise-policy-qualified-type.rs"),
        ),
    ] {
        let mut violations = Vec::new();
        check_source_fixture("cache.rs", source, &mut violations)?;
        assert!(!violations.is_empty(), "{label} passed the policy check");
    }

    let read_only = r"
use std::fs::File as Handle;
mod inner { use std::fs::File as Handle; }
fn read(path: &std::path::Path) -> std::io::Result<Handle> { <Handle>::open(path) }
";
    let mut violations = Vec::new();
    check_source_fixture("cache.rs", read_only, &mut violations)?;
    assert!(
        violations.is_empty(),
        "scoped aliases or qualified read-only open failed: {violations:?}"
    );
    Ok(())
}
