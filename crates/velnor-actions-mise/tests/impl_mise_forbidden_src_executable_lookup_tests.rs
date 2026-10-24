use super::check_source_fixture;

#[test]
fn exact_lookup_and_its_test_fixture_pass_the_scanner() -> Result<(), String> {
    for (name, source) in [
        (
            "requests_executable_lookup.rs",
            include_str!("../src/requests_executable_lookup.rs"),
        ),
        (
            "requests_executable_lookup_tests.rs",
            include_str!("../src/requests_executable_lookup_tests.rs"),
        ),
    ] {
        let violations = violations(name, source)?;
        assert!(violations.is_empty(), "{name}: {violations:?}");
    }
    Ok(())
}

#[test]
fn executable_lookup_rejects_mutating_descriptor_flags() -> Result<(), String> {
    let original = include_str!("../src/requests_executable_lookup.rs");
    assert!(
        violations("requests_executable_lookup.rs", original)?.is_empty(),
        "The descriptor walk must meet the baseline contract."
    );
    let flags = "OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC";
    for replacement in [
        "OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::WRONLY",
        "OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::CREATE",
        "OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::TRUNC",
        "OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC",
    ] {
        let changed = original.replacen(flags, replacement, 1);
        assert_ne!(changed, original, "The test must change the source flags.");
        let found = violations("requests_executable_lookup.rs", &changed)?;
        assert!(
            !found.is_empty(),
            "The scanner must reject these flags: {replacement}."
        );
    }
    let ordinary_open = format!(
        "{original}\nfn lookup(path: &std::path::Path) {{ let _ = std::fs::File::open(path); }}"
    );
    let found = violations("requests_executable_lookup.rs", &ordinary_open)?;
    assert!(
        !found.is_empty(),
        "The scanner must reject an ordinary path open."
    );
    Ok(())
}

#[test]
fn lookup_authority_is_rejected_outside_the_named_module() -> Result<(), String> {
    let source = "use rustix::fs::{Mode, OFlags, openat}; fn lookup(directory: &std::fs::File, name: &std::ffi::OsStr) { let _ = openat(directory, name, OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC, Mode::empty()); }";
    let found = violations("requests.rs", source)?;
    assert!(
        !found.is_empty(),
        "The scanner must reject this descriptor lookup outside the approved module."
    );
    Ok(())
}

#[test]
fn direct_rustup_and_production_symlink_calls_remain_forbidden() -> Result<(), String> {
    let source = include_str!("../src/requests_executable_lookup.rs");
    for suffix in [
        "fn bypass() { let _ = std::process::Command::new(\"rustup\"); }",
        "fn bypass() { let _ = std::os::unix::fs::symlink(\"a\", \"b\"); }",
    ] {
        let altered = format!("{source}\n{suffix}");
        let found = violations("requests_executable_lookup.rs", &altered)?;
        assert!(
            !found.is_empty(),
            "The scanner must reject this unsafe addition: {suffix}."
        );
    }
    let direct = violations("requests.rs", "fn bypass() { let _ = \"rustup\"; }")?;
    assert!(
        !direct.is_empty(),
        "The scanner must reject a bare Rustup reference."
    );
    Ok(())
}

#[test]
fn fixture_mutations_are_limited_to_executable_lookup_tests() -> Result<(), String> {
    for mutation in [
        "std::fs::create_dir_all(path)",
        "std::fs::write(path, bytes)",
        "std::fs::set_permissions(path, permissions)",
        "std::os::unix::fs::symlink(source, target)",
    ] {
        let source = format!("fn fixture() {{ let _ = {mutation}; }}");
        let allowed = violations("requests_executable_lookup_tests.rs", &source)?;
        assert!(
            allowed.is_empty(),
            "The scanner must allow this fixture call in the test module. Violations: {allowed:?}."
        );
        let denied = violations("requests_tests.rs", &source)?;
        assert!(
            !denied.is_empty(),
            "The scanner must reject this fixture call outside the test module: {mutation}."
        );
    }
    Ok(())
}

fn violations(name: &str, source: &str) -> Result<Vec<String>, String> {
    let mut violations = Vec::new();
    check_source_fixture(name, source, &mut violations)?;
    Ok(violations)
}
