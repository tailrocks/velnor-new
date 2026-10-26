// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

fn check_projection_restore() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let checkout = temp.path().join("checkout");
    fs::create_dir_all(&checkout)?;
    let declared = declared_entry("src/lib.rs", b"fixture-bytes", 0o644);
    declared.validate()?;
    let mut retimed = declared.clone();
    retimed.mtime = Some(RealMtime {
        seconds: 1_700_000_000,
        nanoseconds: 1,
    });
    if !declared.semantic_matches(&retimed) {
        return Err(invalid("restored mtimes leaked into coverage identity"));
    }
    write_checkout_file(&checkout, "src/lib.rs", b"other bytes", 0o644)?;
    if authenticate_entry(&checkout, &declared)? != CheckoutState::Changed {
        return Err(invalid("content mismatch missed"));
    }
    write_checkout_file(&checkout, "src/lib.rs", b"fixture-bytes", 0o600)?;
    if authenticate_entry(&checkout, &declared)? != CheckoutState::Changed {
        return Err(invalid("mode mismatch missed"));
    }
    write_checkout_file(&checkout, "src/lib.rs", b"fixture-bytes", 0o644)?;
    if authenticate_entry(&checkout, &declared)? != CheckoutState::Unchanged {
        return Err(invalid("identical restored file rejected"));
    }
    if decide(Some(&declared), Some(&declared), CheckoutState::Changed)
        != RestoreDecision::KeepCurrent
    {
        return Err(invalid("changed checkout file not kept current"));
    }
    if decide(Some(&declared), Some(&declared), CheckoutState::Missing)
        != RestoreDecision::KeepCurrent
    {
        return Err(invalid("deleted input resurrected from cache"));
    }
    if decide(Some(&declared), None, CheckoutState::Unchanged) != RestoreDecision::Discard {
        return Err(invalid("cached-only path kept"));
    }
    let git_entry = ProjectionEntry {
        path: ".git/objects/pack".to_owned(),
        ..declared_entry("src/lib.rs", b"pack", 0o644)
    };
    if !is_git_path(&git_entry.path) {
        return Err(invalid("git path not detected"));
    }
    if decide(Some(&git_entry), Some(&git_entry), CheckoutState::Unchanged)
        != RestoreDecision::KeepCurrent
    {
        return Err(invalid("checkout git metadata scheduled for restore"));
    }
    Ok(())
}

fn check_rejects_tampered_projection() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let checkout = temp.path().join("checkout");
    let staged = temp.path().join("staged");
    fs::create_dir_all(&checkout)?;
    fs::create_dir_all(&staged)?;
    let declared = declared_entry("src/lib.rs", b"declared", 0o644);
    write_checkout_file(&checkout, "src/lib.rs", b"declared", 0o644)?;
    let mut tampered = declared.clone();
    tampered.content_sha256 = sha256_hex(b"tampered");
    must_fail(
        &reconcile(&tampered, &declared).map(|_| ()),
        "archive mismatch admitted",
    )?;
    let mut timeless = declared.clone();
    timeless.mtime = None;
    must_fail(
        &reconcile(&timeless, &declared).map(|_| ()),
        "missing mtime admitted",
    )?;
    let staged_file = write_checkout_file(&staged, "src/lib.rs", b"tampered", 0o644)?;
    let destination = staged_path(&checkout, "src/lib.rs")?;
    must_fail(
        &install_staged_file(&staged_file, &destination, &declared).map(|_| ()),
        "tampered staged file installed",
    )?;
    if fs::read(&destination)? != b"declared" {
        return Err(invalid("failed install wrote destination"));
    }
    Ok(())
}

#[test]
fn c03() -> Result<(), Box<dyn Error>> {
    require_nonempty_workload(&workload())?;
    require_complete_observation(&observation("application-only change"))?;
    check_app_only_preserves_coverage()?;
    check_projection_restore()?;
    check_rejects_tampered_projection()?;
    Ok(())
}

fn check_closure_partition() -> Result<(), Box<dyn Error>> {
    let closure = app_closure()?;
    for member in [APP_ID, LIB_ID, SERDE_ID] {
        if !closure.contains(member) {
            return Err(invalid("affected closure lost a member"));
        }
    }
    if closure.len() != 3 {
        return Err(invalid("affected closure grew beyond the fixture"));
    }
    require_nonzero("affected closure size", closure.len() as u64)?;
    let workspaces = fixture_workspaces("0.1.0", "0.2.0");
    if workspaces.len() != 1 {
        return Err(invalid("workspace fixture grew beyond the case"));
    }
    for member in [APP_ID, LIB_ID] {
        if !workspaces[0].members.contains(&member.to_owned()) {
            return Err(invalid("workspace fixture lost a member"));
        }
    }
    Ok(())
}

fn check_single_dep_update() -> Result<(), Box<dyn Error>> {
    let base: DependencyCoverage = base_coverage()?;
    let lock_bump = coverage_variant("0.1.0", "0.2.0", None, LINUX_TARGET, None, '1')?;
    if base.dependency_digest == lock_bump.dependency_digest {
        return Err(invalid("dependency update kept the old dependency digest"));
    }
    if base.coverage_digest == lock_bump.coverage_digest {
        return Err(invalid("dependency update kept the old coverage digest"));
    }
    let app_only = coverage_variant("0.1.1", "0.2.0", None, LINUX_TARGET, None, 'f')?;
    require_same_coverage(&base, &app_only)?;
    let changed = changed_closure_ledger()?;
    require_nonzero(
        "changed-closure compilations",
        changed.dependency_compilations(),
    )?;
    require_zero(
        "changed-closure application compilations",
        changed.application_compilations(),
    )?;
    let closure = resolve_dependency_closure("c04-task", &base)?;
    closure.verify_complete()?;
    let mut keys: Vec<&str> = closure.inputs.keys().map(String::as_str).collect();
    keys.sort_unstable();
    if keys
        != vec![
            "coverage",
            "dependencies",
            "native_inputs",
            "workspace_deps",
        ]
    {
        return Err(invalid("dependency closure lost an input class"));
    }
    Ok(())
}

fn check_rejects_unknown_closure() -> Result<(), Box<dyn Error>> {
    must_fail(
        &dependency_closure_ids(&fixture_graph(), "").map(|_| ()),
        "empty package admitted",
    )?;
    must_fail(
        &dependency_closure_ids(&fixture_graph(), "missing-crate").map(|_| ()),
        "unknown package admitted",
    )?;
    let base = base_coverage()?;
    must_fail(
        &resolve_dependency_closure("", &base).map(|_| ()),
        "empty task admitted",
    )?;
    let mut malformed = base.clone();
    "not-a-digest".clone_into(&mut malformed.coverage_digest);
    must_fail(
        &resolve_dependency_closure("c04-task", &malformed).map(|_| ()),
        "malformed coverage admitted",
    )?;
    Ok(())
}

#[test]
fn c04() -> Result<(), Box<dyn Error>> {
    require_nonempty_workload(&workload())?;
    require_complete_observation(&observation("single dependency update"))?;
    check_closure_partition()?;
    check_single_dep_update()?;
    check_rejects_unknown_closure()?;
    Ok(())
}

fn check_feature_profile_changes() -> Result<(), Box<dyn Error>> {
    let base = base_coverage()?;
    let features = coverage_variant("0.1.0", "0.2.0", Some("tokio"), LINUX_TARGET, None, 'f')?;
    if base.dependency_digest == features.dependency_digest {
        return Err(invalid("feature change kept the old dependency digest"));
    }
    if base.coverage_digest == features.coverage_digest {
        return Err(invalid("feature change kept the old coverage digest"));
    }
    if base.workspace_digest != features.workspace_digest {
        return Err(invalid("feature change moved the workspace digest"));
    }
    if base.native_digest != features.native_digest {
        return Err(invalid("feature change moved the native digest"));
    }
    let targets = coverage_variant("0.1.0", "0.2.0", None, ARM_TARGET, None, 'f')?;
    if base.dependency_digest == targets.dependency_digest {
        return Err(invalid("target change kept the old dependency digest"));
    }
    if base.coverage_digest == targets.coverage_digest {
        return Err(invalid("target change kept the old coverage digest"));
    }
    let native = coverage_variant("0.1.0", "0.2.0", None, LINUX_TARGET, Some("libgit2"), 'f')?;
    if base.native_digest == native.native_digest {
        return Err(invalid("native change kept the old native digest"));
    }
    if base.coverage_digest == native.coverage_digest {
        return Err(invalid("native change kept the old coverage digest"));
    }
    if base.dependency_digest != native.dependency_digest {
        return Err(invalid("native change moved the dependency digest"));
    }
    Ok(())
}

fn check_runner_profile_separation() -> Result<(), Box<dyn Error>> {
    if NextestProfile::Ci.as_str() == NextestProfile::Default.as_str() {
        return Err(invalid("nextest profiles lost their distinction"));
    }
    if NextestProfile::parse("ci")? != NextestProfile::Ci {
        return Err(invalid("nextest profile lost its round trip"));
    }
    if NextestProfile::parse("default")? != NextestProfile::Default {
        return Err(invalid("default profile lost its round trip"));
    }
    must_fail(
        &NextestProfile::parse("nightly").map(|_| ()),
        "unknown nextest profile admitted",
    )?;
    if CompileDriver::Cargo.as_str() == NextestProfile::Default.as_str() {
        return Err(invalid("compile driver merged with the test profile"));
    }
    if CompileDriver::parse("cargo")? != CompileDriver::Cargo {
        return Err(invalid("compile driver lost its round trip"));
    }
    must_fail(
        &CompileDriver::parse("turbo").map(|_| ()),
        "unknown compile driver admitted",
    )?;
    if TestRunner::CargoNextest == TestRunner::CargoTest {
        return Err(invalid("test runners lost their distinction"));
    }
    if TestRunner::parse("cargo_nextest")? != TestRunner::CargoNextest {
        return Err(invalid("test runner lost its round trip"));
    }
    must_fail(
        &TestRunner::parse("jest").map(|_| ()),
        "unknown test runner admitted",
    )?;
    Ok(())
}

#[test]
fn c05() -> Result<(), Box<dyn Error>> {
    require_nonempty_workload(&workload())?;
    require_complete_observation(&observation("feature and profile change"))?;
    check_feature_profile_changes()?;
    check_runner_profile_separation()?;
    Ok(())
}
