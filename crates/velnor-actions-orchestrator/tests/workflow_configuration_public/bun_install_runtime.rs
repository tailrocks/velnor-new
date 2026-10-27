// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[path = "bun_install_runtime_harness.rs"]
mod harness;

use std::error::Error;
use std::fs;
use std::path::Path;

const SHADOW_FILE: &str = "shadow-log";

#[test]
fn generated_bun_installs_select_bun_for_trusted_local_lifecycle_scripts()
-> Result<(), Box<dyn Error>> {
    let bun = harness::pinned_bun()?;
    harness::frozen_legacy_install_arm(&bun)?;
    harness::frozen_lock_mismatch_arm(&bun)?;
    let ordinary = generated_ordinary_install_arm(&bun);
    let private = generated_private_lifecycle_arm(&bun);
    let script_shadow = generated_shadow_arm(&bun, ShadowKind::PackageScript);
    ordinary?;
    private?;
    script_shadow?;
    Ok(())
}

fn generated_ordinary_install_arm(bun: &Path) -> Result<(), Box<dyn Error>> {
    let root = harness::make_repo(false)?;
    let (prepared, script) = harness::generated_install(root.path())?;
    assert!(!prepared.workflow.ir.jobs.is_empty());
    let lock = fs::read(root.path().join("frontend/bun.lock"))?;
    let output = harness::run_generated(root.path(), &script, bun, "ordinary")?;
    harness::assert_runtime_success(root.path(), bun, &output, "ordinary")?;
    assert_eq!(fs::read(root.path().join("frontend/bun.lock"))?, lock);
    Ok(())
}

fn generated_private_lifecycle_arm(bun: &Path) -> Result<(), Box<dyn Error>> {
    let root = harness::make_repo(true)?;
    let (prepared, script) = harness::generated_private_lifecycle(root.path())?;
    assert!(!prepared.workflow.ir.jobs.is_empty());
    let lock = fs::read(root.path().join("frontend/bun.lock"))?;
    let stale = root.path().join("frontend/node_modules/stale-marker");
    fs::create_dir_all(stale.parent().ok_or("node_modules parent missing")?)?;
    fs::write(&stale, b"stale")?;
    let output = harness::run_generated(root.path(), &script, bun, "private")?;
    harness::assert_runtime_success(root.path(), bun, &output, "private")?;
    assert!(!stale.exists(), "lifecycle phase kept stale node_modules");
    assert_eq!(fs::read(root.path().join("frontend/bun.lock"))?, lock);
    Ok(())
}

fn generated_shadow_arm(bun: &Path, shadow: ShadowKind) -> Result<(), Box<dyn Error>> {
    let root = harness::make_repo(false)?;
    shadow.apply(root.path())?;
    let (_, script) = harness::generated_install(root.path())?;
    let lock = fs::read(root.path().join("frontend/bun.lock"))?;
    let label = shadow.label();
    let output = harness::run_generated(root.path(), &script, bun, label)?;
    let marker = fs::read_to_string(root.path().join(SHADOW_FILE))?;
    if !marker.is_empty() {
        assert_eq!(marker, "package-script\n");
        assert_eq!(
            output.status.code(),
            Some(78),
            "shadow exit changed: {output:?}"
        );
        return Err("generated Bun install executed the root bun package script".into());
    }
    harness::assert_runtime_success(root.path(), bun, &output, label)?;
    assert_eq!(fs::read(root.path().join("frontend/bun.lock"))?, lock);
    Ok(())
}

#[derive(Clone, Copy)]
enum ShadowKind {
    PackageScript,
}

impl ShadowKind {
    fn label(self) -> &'static str {
        match self {
            Self::PackageScript => "script-shadow",
        }
    }

    fn apply(self, root: &Path) -> Result<(), Box<dyn Error>> {
        match self {
            Self::PackageScript => {
                write_root_shadow_package(root)?;
                let manifest = root.join("package.json");
                let mut package: serde_json::Value = serde_json::from_slice(&fs::read(&manifest)?)?;
                package["scripts"]["bun"] = serde_json::json!("bun ./frontend/script-shadow.cjs");
                fs::write(&manifest, serde_json::to_vec(&package)?)?;
            }
        }
        Ok(())
    }
}

fn write_root_shadow_package(root: &Path) -> Result<(), Box<dyn Error>> {
    fs::write(
        root.join("package.json"),
        r#"{"name":"shadow-root","private":true,"scripts":{}}"#,
    )?;
    Ok(())
}
