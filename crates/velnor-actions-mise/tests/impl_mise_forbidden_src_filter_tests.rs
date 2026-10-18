use super::check_file;

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

fn check_source_fixture(
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
