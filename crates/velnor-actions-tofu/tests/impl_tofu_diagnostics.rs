//! Lock, version, and init-stderr diagnostic cases.
use velnor_actions_tofu::diagnostics::{
    LOCKFILE_MISSING, LOCKFILE_STALE, PROVIDER_DEPENDENCY_CHANGES,
    REQUIRED_VERSION_EXCLUDES_TOOLCHAIN, RequiredVersionClaim, lockfile_findings_for_root,
    remediation_for_init_stderr, required_versions_for_root, version_compat_findings,
};
use velnor_actions_tofu::file_cache::FileCache;
use velnor_actions_tofu::lockfile::LOCKFILE_UNPINNED_HASHES;

use crate::support::{Outcome, TempDir, fixture_dir};

/// Seed `main.tf` plus an optional lock under one root.
fn seed_root(dir: &TempDir, unit: &str, config: &str, lock: Option<&str>) -> Outcome {
    let prefix = if unit == "." {
        String::new()
    } else {
        format!("{unit}/")
    };
    dir.write(&format!("{prefix}main.tf"), config)?;
    if let Some(bytes) = lock {
        dir.write(&format!("{prefix}.terraform.lock.hcl"), bytes)?;
    }
    Ok(())
}

#[test]
fn missing_lock_on_provider_root_is_a_finding() -> Outcome {
    let dir = TempDir::create("tofu-diag-missing")?;
    seed_root(&dir, ".", "resource \"x\" \"y\" {}\n", None)?;
    let findings = lockfile_findings_for_root(dir.path(), ".", &mut FileCache::new());
    assert_eq!(findings.len(), 1);
    let finding = &findings[0];
    assert_eq!(finding.code, LOCKFILE_MISSING);
    assert_eq!(finding.code, "tofu_lockfile_missing");
    assert_eq!(finding.path, ".terraform.lock.hcl");
    assert!(finding.validate().is_ok());
    assert!(
        finding
            .action
            .as_deref()
            .unwrap_or_default()
            .contains("manually"),
        "manual remediation: {finding:?}"
    );
    Ok(())
}

#[test]
fn absent_fixture_root_stays_silent() {
    let root = fixture_dir("tofu-lockfile").join("absent");
    assert!(lockfile_findings_for_root(&root, ".", &mut FileCache::new()).is_empty());
}

#[test]
fn present_empty_fixture_root_stays_silent() {
    let root = fixture_dir("tofu-lockfile").join("present-empty");
    assert!(lockfile_findings_for_root(&root, ".", &mut FileCache::new()).is_empty());
}

#[test]
fn stale_fixture_root_names_the_provider() {
    let root = fixture_dir("tofu-lockfile").join("stale");
    let findings = lockfile_findings_for_root(&root, ".", &mut FileCache::new());
    assert_eq!(findings.len(), 1);
    let finding = &findings[0];
    assert_eq!(finding.code, LOCKFILE_STALE);
    assert_eq!(finding.code, "tofu_lockfile_stale");
    assert!(finding.validate().is_ok());
    assert!(
        finding
            .observed
            .as_deref()
            .unwrap_or_default()
            .contains("example.com/a/b"),
        "names the stale entry: {finding:?}"
    );
}

#[test]
fn weakened_fixture_root_is_corrupt() {
    let root = fixture_dir("tofu-lockfile").join("weakened");
    let findings = lockfile_findings_for_root(&root, ".", &mut FileCache::new());
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].code, "tofu_lockfile_corrupt");
    assert!(findings[0].validate().is_ok());
}

#[test]
fn provider_root_with_pins_stays_silent() -> Outcome {
    let dir = TempDir::create("tofu-diag-pinned")?;
    seed_root(
        &dir,
        "stacks/a",
        "resource \"x\" \"y\" {}\n",
        Some(
            "provider \"example.com/a/b\" {\nversion = \"1.0.0\"\n\
             hashes = [\"h1:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=\"]\n}\n",
        ),
    )?;
    assert!(lockfile_findings_for_root(dir.path(), "stacks/a", &mut FileCache::new()).is_empty());
    Ok(())
}

#[test]
fn provider_root_with_empty_lock_is_missing() -> Outcome {
    let dir = TempDir::create("tofu-diag-empty")?;
    seed_root(&dir, ".", "data \"x\" \"y\" {}\n", Some(""))?;
    let findings = lockfile_findings_for_root(dir.path(), ".", &mut FileCache::new());
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].code, LOCKFILE_MISSING);
    Ok(())
}

#[test]
fn module_calls_abstain_from_stale_claims() -> Outcome {
    let dir = TempDir::create("tofu-diag-module")?;
    seed_root(
        &dir,
        ".",
        "module \"m\" {\n  source = \"./mods/m\"\n}\n",
        Some(
            "provider \"example.com/a/b\" {\n\
             hashes = [\"h1:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=\"]\n}\n",
        ),
    )?;
    assert!(lockfile_findings_for_root(dir.path(), ".", &mut FileCache::new()).is_empty());
    Ok(())
}

#[test]
fn terraform_block_alone_abstains_both_ways() -> Outcome {
    let dir = TempDir::create("tofu-diag-terraform")?;
    seed_root(
        &dir,
        ".",
        "terraform {\n  required_version = \">= 1.6\"\n}\n",
        None,
    )?;
    assert!(lockfile_findings_for_root(dir.path(), ".", &mut FileCache::new()).is_empty());
    Ok(())
}

#[test]
fn malformed_configs_abstain_from_shape_claims_only() -> Outcome {
    let dir = TempDir::create("tofu-diag-malformed")?;
    seed_root(
        &dir,
        ".",
        "resource \"x\" {\n  broken ==\n",
        Some("provider \"a/b/c\" {}\n"),
    )?;
    let findings = lockfile_findings_for_root(dir.path(), ".", &mut FileCache::new());
    assert_eq!(findings.len(), 1, "lock-level claims need no configs");
    assert_eq!(findings[0].code, LOCKFILE_UNPINNED_HASHES);
    assert_eq!(findings[0].code, "tofu_lockfile_unpinned_hashes");
    assert!(findings[0].validate().is_ok());
    Ok(())
}

#[test]
fn unreadable_lock_stays_silent() -> Outcome {
    let dir = TempDir::create("tofu-diag-unreadable")?;
    seed_root(&dir, ".", "resource \"x\" \"y\" {}\n", None)?;
    std::fs::create_dir(dir.path().join(".terraform.lock.hcl"))?;
    assert!(lockfile_findings_for_root(dir.path(), ".", &mut FileCache::new()).is_empty());
    Ok(())
}

#[test]
fn version_compat_flags_constraints_excluding_the_toolchain() {
    let claims = vec![
        RequiredVersionClaim {
            path: "a/main.tf".to_owned(),
            constraint: "= 1.5.7".to_owned(),
        },
        RequiredVersionClaim {
            path: "b/main.tf".to_owned(),
            constraint: ">= 1.7.0".to_owned(),
        },
    ];
    let findings = version_compat_findings(&claims, "1.13.1");
    assert_eq!(findings.len(), 1);
    let finding = &findings[0];
    assert_eq!(finding.code, REQUIRED_VERSION_EXCLUDES_TOOLCHAIN);
    assert_eq!(finding.code, "tofu_required_version_excludes_toolchain");
    assert_eq!(finding.path, "a/main.tf");
    assert!(finding.validate().is_ok());
    assert!(
        finding
            .observed
            .as_deref()
            .unwrap_or_default()
            .contains("1.13.1"),
        "names the toolchain: {finding:?}"
    );
}

#[test]
fn version_compat_abstains_without_a_toolchain_claim() {
    let claims = vec![RequiredVersionClaim {
        path: "main.tf".to_owned(),
        constraint: "= 1.5.7".to_owned(),
    }];
    for toolchain in ["latest", "", "1.13.1-beta1"] {
        assert!(
            version_compat_findings(&claims, toolchain).is_empty(),
            "{toolchain:?}"
        );
    }
    let unparseable = vec![RequiredVersionClaim {
        path: "main.tf".to_owned(),
        constraint: "banana".to_owned(),
    }];
    assert!(version_compat_findings(&unparseable, "1.13.1").is_empty());
    assert!(version_compat_findings(&[], "1.13.1").is_empty());
}

#[test]
fn init_stderr_maps_to_manual_remediation() {
    let stderr = "Error: Provider dependency changes detected:\n-run tofu providers lock\n";
    let finding = remediation_for_init_stderr(".terraform.lock.hcl", stderr, 1).expect("mapping");
    assert_eq!(finding.code, PROVIDER_DEPENDENCY_CHANGES);
    assert_eq!(finding.code, "tofu_provider_dependency_changes");
    assert_eq!(finding.path, ".terraform.lock.hcl");
    assert!(finding.validate().is_ok());
    assert!(
        finding
            .action
            .as_deref()
            .unwrap_or_default()
            .contains("manually"),
        "manual remediation: {finding:?}"
    );
    assert!(remediation_for_init_stderr(".terraform.lock.hcl", stderr, 0).is_none());
    assert!(remediation_for_init_stderr(".terraform.lock.hcl", stderr, 2).is_none());
    assert!(remediation_for_init_stderr(".terraform.lock.hcl", "Success!\n", 1).is_none());
}

#[test]
fn required_versions_collect_with_paths() -> Outcome {
    let dir = TempDir::create("tofu-diag-claims")?;
    dir.write(
        "a/main.tf",
        "terraform {\n  required_version = \">= 1.7\"\n}\n",
    )?;
    dir.write(
        "a/extra.tofu",
        "terraform {\n  required_version = \"< 2.0\"\n}\n",
    )?;
    dir.write("a/README.md", "required_version = \"= 0.0\"\n")?;
    let mut claims = required_versions_for_root(dir.path(), "a", &mut FileCache::new());
    claims.sort_by(|left, right| left.path.cmp(&right.path));
    assert_eq!(claims.len(), 2);
    assert_eq!(claims[0].path, "a/extra.tofu");
    assert_eq!(claims[0].constraint, "< 2.0");
    assert_eq!(claims[1].path, "a/main.tf");
    assert_eq!(claims[1].constraint, ">= 1.7");
    Ok(())
}

#[test]
fn diagnostics_module_never_writes() {
    let src = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/diagnostics.rs"),
    )
    .expect("read");
    for token in [
        "fs::write",
        "File::create",
        "OpenOptions",
        "Command::new",
        "std::process",
    ] {
        assert!(!src.contains(token), "write token {token}");
    }
}
