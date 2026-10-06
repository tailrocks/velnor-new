//! P12 dependency-identity cases: every form, scope, and failure mode.
//!
//! Each case builds a self-contained fixture tree and runs the real
//! `scripts/check-freshness.sh --root` against it.

use std::error::Error;

use super::p12_harness as harness;

#[test]
fn complete_supported_inventory_passes() -> Result<(), Box<dyn Error>> {
    let fixture = harness::passing("p12-pass")?;
    let run = harness::run_script(&fixture.dir, &[])?;
    harness::assert_clean(&run);
    for needle in [
        "aaa:dependencies:serde",
        "aaa:dependencies:js",
        "aaa:build-dependencies:toml",
        "aaa:dev-dependencies:tempfile",
        "aaa:target.cfg(unix).dependencies:globset",
        "path-only, no registry identity",
        "9 locked names retained",
        "10 locked packages reachable",
    ] {
        assert!(
            run.stdout.contains(needle),
            "missing {needle}:\n{}",
            run.stdout
        );
    }
    harness::cleanup(&fixture);
    Ok(())
}

#[test]
fn excluded_nested_workspace_uses_its_own_lock() -> Result<(), Box<dyn Error>> {
    let fixture = harness::passing("p12-nested-workspace")?;
    harness::add_nested_workspace(&fixture)?;

    let run = harness::run_script(&fixture.dir, &[])?;
    harness::assert_clean(&run);
    for needle in [
        "worker:dependencies:runneronly",
        "crates/runner/(lock-membership)",
        "crates/runner/(lock-graph)",
    ] {
        assert!(
            run.stdout.contains(needle),
            "missing {needle}:\n{}",
            run.stdout
        );
    }
    harness::cleanup(&fixture);
    Ok(())
}

#[test]
fn nested_workspace_dependency_skew_fails() -> Result<(), Box<dyn Error>> {
    let fixture = harness::passing("p12-nested-skew")?;
    harness::add_nested_workspace(&fixture)?;
    harness::mutate(
        &fixture.dir,
        "crates/runner/crates/worker/Cargo.toml",
        "runneronly = \"=1.2.3\"",
        "runneronly = \"=1.2.4\"",
    )?;

    let run = harness::run_script(&fixture.dir, &[])?;
    harness::assert_fail(&run, "worker:dependencies:runneronly");
    assert!(run.stdout.contains("no locked identity"), "{}", run.stdout);
    harness::cleanup(&fixture);
    Ok(())
}

#[test]
fn inexact_requirement_fails() -> Result<(), Box<dyn Error>> {
    let fixture = harness::passing("p12-inexact")?;
    harness::mutate(
        &fixture.dir,
        "crates/aaa/Cargo.toml",
        "serde_json = \"=1.0.100\"",
        "serde_json = \"1.0.100\"",
    )?;
    let run = harness::run_script(&fixture.dir, &[])?;
    harness::assert_fail(&run, "aaa:dependencies:serde_json");
    assert!(
        run.stdout.contains("exact `=x.y.z` (VER-2.26)"),
        "{}",
        run.stdout
    );
    harness::cleanup(&fixture);
    Ok(())
}

#[test]
fn declared_lock_skew_fails() -> Result<(), Box<dyn Error>> {
    let fixture = harness::passing("p12-skew")?;
    harness::mutate(
        &fixture.dir,
        "crates/aaa/Cargo.toml",
        "blake3 = { version = \"=1.5.0\" }",
        "blake3 = { version = \"=1.6.0\" }",
    )?;
    let run = harness::run_script(&fixture.dir, &[])?;
    harness::assert_fail(&run, "has no locked identity");
    harness::cleanup(&fixture);
    Ok(())
}

#[test]
fn git_dependency_fails() -> Result<(), Box<dyn Error>> {
    let fixture = harness::passing("p12-git")?;
    harness::mutate(
        &fixture.dir,
        "crates/aaa/Cargo.toml",
        "[dev-dependencies]",
        "evil = { git = \"https://example.invalid/evil\", rev = \"abc123\" }\n\n[dev-dependencies]",
    )?;
    let run = harness::run_script(&fixture.dir, &[])?;
    harness::assert_fail(&run, "git dependency forbidden");
    harness::cleanup(&fixture);
    Ok(())
}

#[test]
fn unresolvable_workspace_inheritance_fails() -> Result<(), Box<dyn Error>> {
    let fixture = harness::passing("p12-inherit")?;
    harness::mutate(
        &fixture.dir,
        "Cargo.toml",
        "serde = \"=1.0.200\"",
        "serde_moved = \"=1.0.200\"",
    )?;
    let run = harness::run_script(&fixture.dir, &[])?;
    harness::assert_fail(&run, "workspace inheritance unresolvable");
    harness::cleanup(&fixture);
    Ok(())
}

#[test]
fn ambiguous_multi_source_identity_fails() -> Result<(), Box<dyn Error>> {
    let fixture = harness::passing("p12-ambiguous")?;
    let path = fixture.dir.join("Cargo.lock");
    let mut body = std::fs::read_to_string(&path)?;
    body.push_str(
        "[[package]]\nname = \"serde_json\"\nversion = \"1.0.100\"\n\
         source = \"git+https://example.invalid/serde_json#abc123\"\n",
    );
    std::fs::write(path, body)?;
    let run = harness::run_script(&fixture.dir, &[])?;
    harness::assert_fail(&run, "ambiguous identity");
    harness::cleanup(&fixture);
    Ok(())
}

#[test]
fn stranded_lock_package_fails() -> Result<(), Box<dyn Error>> {
    let fixture = harness::passing("p12-stranded")?;
    let path = fixture.dir.join("Cargo.lock");
    let mut body = std::fs::read_to_string(&path)?;
    body.push_str(
        "[[package]]\nname = \"orphan\"\nversion = \"9.9.9\"\n\
         source = \"registry+https://github.com/rust-lang/crates.io-index\"\n",
    );
    std::fs::write(path, body)?;
    let run = harness::run_script(&fixture.dir, &[])?;
    harness::assert_fail(&run, "unreachable locked package orphan");
    harness::cleanup(&fixture);
    Ok(())
}

#[test]
fn workspace_package_rename_inherits() -> Result<(), Box<dyn Error>> {
    let fixture = harness::passing("p12-ws-rename")?;
    harness::mutate(
        &fixture.dir,
        "crates/aaa/Cargo.toml",
        "js = { package = \"serde_json\", version = \"=1.0.100\" }",
        "js = { package = \"serde_json\", version = \"=1.0.100\" }\njs2 = { workspace = true }",
    )?;
    harness::mutate(
        &fixture.dir,
        "Cargo.toml",
        "toml = { version = \"=0.9.0\" }",
        "toml = { version = \"=0.9.0\" }\njs2 = { package = \"serde_json\", version = \"=1.0.100\" }",
    )?;
    let run = harness::run_script(&fixture.dir, &[])?;
    harness::assert_clean(&run);
    assert!(
        run.stdout.contains("aaa:dependencies:js2"),
        "renamed inherit not resolved:\n{}",
        run.stdout
    );
    harness::cleanup(&fixture);
    Ok(())
}

#[test]
fn target_scope_inexact_fails() -> Result<(), Box<dyn Error>> {
    let fixture = harness::passing("p12-target-inexact")?;
    harness::mutate(
        &fixture.dir,
        "crates/aaa/Cargo.toml",
        "globset = \"=0.4.10\"",
        "globset = \"0.4.10\"",
    )?;
    let run = harness::run_script(&fixture.dir, &[])?;
    harness::assert_fail(&run, "aaa:target.cfg(unix).dependencies:globset");
    assert!(
        run.stdout.contains("exact `=x.y.z` (VER-2.26)"),
        "{}",
        run.stdout
    );
    harness::cleanup(&fixture);
    Ok(())
}

#[test]
fn dangling_lock_edge_fails() -> Result<(), Box<dyn Error>> {
    let fixture = harness::passing("p12-dangling")?;
    harness::mutate(
        &fixture.dir,
        "Cargo.lock",
        " \"tempfile\",\n",
        " \"tempfile\",\n \"nope 1.2.3\",\n",
    )?;
    let run = harness::run_script(&fixture.dir, &[])?;
    harness::assert_fail(&run, "dangling edge");
    harness::cleanup(&fixture);
    Ok(())
}
