#[test]
fn clippy_toml_has_five_settings() -> Result<(), Box<dyn Error>> {
    let body = read("clippy.toml")?;
    let settings = [
        "too-many-lines-threshold = 80",
        "allow-unwrap-in-tests = false",
        "allow-expect-in-tests = true",
        "allow-panic-in-tests = true",
        "check-incompatible-msrv-in-tests = true",
    ];
    for line in settings {
        assert!(body.contains(line), "clippy.toml misses {line}");
    }
    let live = body
        .lines()
        .filter(|line| !line.trim().is_empty() && !line.trim().starts_with('#'))
        .count();
    assert_eq!(live, 5, "clippy.toml must hold exactly 5 settings");
    Ok(())
}

// Behavioral enforcement (forbidden groups, alint verdicts, unsafe
// absence) and P11 strictness guarantees live in the sibling
// `impl_repo_strictness` module, which shares the helpers above.
