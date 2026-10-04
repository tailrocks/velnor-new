//! Selection pins must resolve through the exact compiled native authority chain.

use std::error::Error;
use std::process::Command;

const SETUP: &str = r#"
import pathlib, re, tempfile, sys
text = pathlib.Path(sys.argv[1]).read_text()
failures = []
def fail_row(check, subject, detail):
    failures.append((check, subject, detail))
exec('def rust_without_comments' + text.split('def rust_without_comments', 1)[1].split('def parse_iso_date', 1)[0])
WORKLOAD_CATALOG = 'crates/velnor-actions-mise/src/catalog_workloads.rs'
QUALIFICATION = 'crates/velnor-actions-mise/src/catalog_qualification.rs'
def native(tool):
    return f'crates/velnor-actions-mise/src/catalog_qualification_{tool}.rs'
def write(path, body):
    path = pathlib.Path(root) / path
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(body)
def reset(tool):
    failures.clear()
    write(WORKLOAD_CATALOG, f'pub const {tool.upper()}_VERSION: &str = super::qualification::{tool.upper()}_SELECTION_VERSION;\n')
    write(QUALIFICATION, f'#[path = "catalog_qualification_{tool}.rs"]\nmod {tool}_records;\npub const {tool.upper()}_SELECTION_VERSION: &str = {tool}_records::VERSION;\n')
    write(native(tool), 'pub(super) const VERSION: &str = "1.2.3";\n')
def replace(path, old, new):
    source = pathlib.Path(root) / path
    assert source.read_text().count(old) == 1
    source.write_text(source.read_text().replace(old, new))
def rejected(tool):
    assert profile_version(tool) is None
    assert failures
SEMVER_CATALOG = 'crates/velnor-actions-mise/src/catalog.rs'
SEMVER_NATIVE = native('semver')
def reset_semver():
    failures.clear()
    write(SEMVER_CATALOG, '#[path = "catalog_qualification.rs"]\npub mod qualification;\npub const CARGO_SEMVER_CHECKS_VERSION: &str = qualification::CARGO_SEMVER_CHECKS_SELECTION_VERSION;\n')
    write(QUALIFICATION, '#[path = "catalog_qualification_semver.rs"]\nmod semver_records;\npub const CARGO_SEMVER_CHECKS_SELECTION_VERSION: &str = semver_records::VERSION;\n')
    write(SEMVER_NATIVE, 'pub(super) const VERSION: &str = "1.2.3";\n')
scratch = tempfile.TemporaryDirectory()
root = scratch.name
"#;

fn check(case: &str) -> Result<(), Box<dyn Error>> {
    let script = super::repo_root().join("scripts/check-freshness.sh");
    let output = Command::new("python3")
        .args(["-c", &format!("{SETUP}\n{case}"), &script.to_string_lossy()])
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

#[test]
fn exact_native_selection_chain_uses_the_single_literal() -> Result<(), Box<dyn Error>> {
    check(
        r#"for tool in ['java', 'gradle']:
    reset(tool)
    assert profile_version(tool) == '1.2.3'
    assert not failures
    replace(native(tool), '1.2.3', '1.2.4')
    assert profile_version(tool) == '1.2.4'
    assert not failures"#,
    )
}

#[test]
fn workload_literal_wrong_alias_and_missing_alias_fail_closed() -> Result<(), Box<dyn Error>> {
    check(
        r#"for tool in ['java', 'gradle']:
    for expression in ['"1.2.3"', 'super::qualification::JAVA_SOURCE_COMMIT', 'other::VERSION']:
        reset(tool)
        replace(WORKLOAD_CATALOG, f'super::qualification::{tool.upper()}_SELECTION_VERSION', expression)
        rejected(tool)
    reset(tool)
    write(WORKLOAD_CATALOG, '')
    rejected(tool)
    reset(tool)
    source = pathlib.Path(root) / WORKLOAD_CATALOG
    source.write_text(source.read_text() * 2)
    rejected(tool)"#,
    )
}

#[test]
fn qualification_alias_and_module_binding_are_exact() -> Result<(), Box<dyn Error>> {
    check(
        r#"for tool in ['java', 'gradle']:
    for expression in ['"1.2.3"', 'other_records::VERSION', f'{tool}_records::REPORTED_RUNTIME_VERSION']:
        reset(tool)
        replace(QUALIFICATION, f'= {tool}_records::VERSION;', f'= {expression};')
        rejected(tool)
    for duplicate in [False, True]:
        reset(tool)
        declaration = f'pub const {tool.upper()}_SELECTION_VERSION: &str = {tool}_records::VERSION;'
        replace(QUALIFICATION, declaration, declaration * 2 if duplicate else '')
        rejected(tool)
    for binding in ['', f'#[path = "wrong.rs"]\nmod {tool}_records;', f'#[path = "catalog_qualification_{tool}.rs"]\nmod {tool}_records;\nmod {tool}_records;']:
        reset(tool)
        replace(QUALIFICATION, f'#[path = "catalog_qualification_{tool}.rs"]\nmod {tool}_records;', binding)
        rejected(tool)"#,
    )
}

#[test]
fn missing_duplicate_commented_or_indirect_native_literal_fails_closed()
-> Result<(), Box<dyn Error>> {
    check(
        r#"for tool in ['java', 'gradle']:
    declaration = 'pub(super) const VERSION: &str = "1.2.3";'
    for source in ['', declaration * 2, '// ' + declaration, '/* ' + declaration + ' */', 'pub const VERSION: &str = "1.2.3";', 'pub(super) const VERSION: &str = OTHER;']:
        reset(tool)
        write(native(tool), source)
        rejected(tool)
    reset(tool)
    (pathlib.Path(root) / native(tool)).unlink()
    rejected(tool)"#,
    )
}

#[test]
fn commented_aliases_never_supply_live_selection_or_module_bindings() -> Result<(), Box<dyn Error>>
{
    check(
        r#"for tool in ['java', 'gradle']:
    for path in [WORKLOAD_CATALOG, QUALIFICATION]:
        for prefix, suffix in [('/* ', ' */'), ('// ', '')]:
            reset(tool)
            source = pathlib.Path(root) / path
            source.write_text('\n'.join(prefix + line + suffix for line in source.read_text().splitlines()))
            rejected(tool)"#,
    )
}

#[test]
fn conditional_module_with_import_rebinding_fails_closed() -> Result<(), Box<dyn Error>> {
    check(
        r#"for tool in ['java', 'gradle']:
    for conditional in [False, True]:
        reset(tool)
        path = pathlib.Path(root) / QUALIFICATION
        source = path.read_text()
        if conditional:
            source = '#[cfg(any())]\n' + source
        path.write_text(source + f'\nuse {"gradle" if tool == "java" else "java"}_records as {tool}_records;\n')
        rejected(tool)
    reset(tool)
    path = pathlib.Path(root) / QUALIFICATION
    path.write_text('#[cfg(any())]\n' + path.read_text())
    rejected(tool)"#,
    )
}

#[test]
fn conditional_native_literal_with_reexport_fails_closed() -> Result<(), Box<dyn Error>> {
    check(
        r#"for tool in ['java', 'gradle']:
    for conditional in [False, True]:
        reset(tool)
        path = pathlib.Path(root) / native(tool)
        source = path.read_text()
        if conditional:
            source = '#[cfg(any())]\n' + source
        path.write_text(source + f'\npub(super) use super::{"gradle" if tool == "java" else "java"}_records::VERSION;\n')
        rejected(tool)
    reset(tool)
    path = pathlib.Path(root) / native(tool)
    path.write_text('#[cfg(any())]\n' + path.read_text())
    rejected(tool)"#,
    )
}

#[test]
fn complete_fixture_checks_native_authority_against_inventory() -> Result<(), Box<dyn Error>> {
    let fixture = super::p12_harness::passing("p12-profile-native")?;
    super::p12_harness::assert_clean(&super::p12_harness::run_script(&fixture.dir, &[])?);
    super::p12_harness::mutate(
        &fixture.dir,
        "crates/velnor-actions-mise/src/catalog_qualification_java.rs",
        "pub(super) const VERSION: &str = \"25.0.4.1.1\";",
        "pub(super) const VERSION: &str = \"25.0.4.2\";",
    )?;
    super::p12_harness::assert_fail(
        &super::p12_harness::run_script(&fixture.dir, &[])?,
        "tool java",
    );
    super::p12_harness::cleanup(&fixture);
    Ok(())
}

#[test]
fn semver_selection_resolves_only_its_native_literal() -> Result<(), Box<dyn Error>> {
    check(
        r#"reset_semver()
assert profile_version('cargo-semver-checks') == '1.2.3'
assert not failures
replace(SEMVER_NATIVE, '1.2.3', '1.2.4')
assert profile_version('cargo-semver-checks') == '1.2.4'
assert not failures"#,
    )
}

#[test]
fn semver_aliases_and_module_paths_fail_closed_on_drift() -> Result<(), Box<dyn Error>> {
    check(
        r#"for path, old, new in [
    (SEMVER_CATALOG, 'qualification::CARGO_SEMVER_CHECKS_SELECTION_VERSION', '"1.2.3"'),
    (QUALIFICATION, 'semver_records::VERSION', 'gradle_records::VERSION'),
    (SEMVER_CATALOG, 'catalog_qualification.rs', 'other.rs'),
    (QUALIFICATION, 'catalog_qualification_semver.rs', 'catalog_qualification_gradle.rs'),
    (SEMVER_NATIVE, 'pub(super) const VERSION: &str = "1.2.3";', '')
]:
    reset_semver()
    replace(path, old, new)
    rejected('cargo-semver-checks')
for path in [SEMVER_CATALOG, QUALIFICATION, SEMVER_NATIVE]:
    reset_semver()
    source = pathlib.Path(root) / path
    source.write_text(source.read_text() * 2)
    rejected('cargo-semver-checks')
    reset_semver()
    source.write_text('/* ' + source.read_text() + ' */')
    rejected('cargo-semver-checks')"#,
    )
}

#[test]
fn semver_conditional_or_rebound_authority_fails_closed() -> Result<(), Box<dyn Error>> {
    check(
        r#"for path, imported in [
    (SEMVER_CATALOG, 'use other as qualification;'),
    (QUALIFICATION, 'use gradle_records as semver_records;'),
    (SEMVER_NATIVE, 'pub(super) use super::gradle_records::VERSION;')
]:
    for conditional, rebind in [(True, False), (False, True), (True, True)]:
        reset_semver()
        source = pathlib.Path(root) / path
        body = source.read_text()
        if conditional:
            body = '#[cfg(any())]\n' + body
        if rebind:
            body += imported
        source.write_text(body)
        rejected('cargo-semver-checks')"#,
    )
}

#[test]
fn complete_fixture_checks_semver_native_authority_against_inventory() -> Result<(), Box<dyn Error>>
{
    let fixture = super::p12_harness::passing("p12-semver-native")?;
    super::p12_harness::assert_clean(&super::p12_harness::run_script(&fixture.dir, &[])?);
    let path = "crates/velnor-actions-mise/src/catalog_qualification_semver.rs";
    let source = std::fs::read_to_string(fixture.dir.join(path))?;
    let declaration = source
        .lines()
        .find(|line| line.starts_with("pub(super) const VERSION:"))
        .ok_or("native semver VERSION missing")?;
    super::p12_harness::mutate(
        &fixture.dir,
        path,
        declaration,
        "pub(super) const VERSION: &str = \"0.0.0\";",
    )?;
    super::p12_harness::assert_fail(
        &super::p12_harness::run_script(&fixture.dir, &[])?,
        "tool cargo-semver-checks",
    );
    super::p12_harness::cleanup(&fixture);
    Ok(())
}
