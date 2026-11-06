// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::workspace_lock;
use std::{fs, path::Path, process::Command};

fn write(root: &Path, path: &str, contents: &str) -> Result<(), Box<dyn std::error::Error>> {
    let target = root.join(path);
    fs::create_dir_all(target.parent().ok_or("parent")?)?;
    fs::write(target, contents)?;
    Ok(())
}

fn metadata(root: &Path, manifest: &str) -> Result<String, Box<dyn std::error::Error>> {
    let output = Command::new(env!("CARGO"))
        .args([
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--offline",
            "--manifest-path",
            manifest,
        ])
        .current_dir(root)
        .output()?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned().into());
    }
    Ok(String::from_utf8(output.stdout)?)
}

#[test]
fn actual_cargo_workspace_root_controls_rendered_lock() -> Result<(), Box<dyn std::error::Error>> {
    use velnor_actions_workflow_renderer::release_tree::{
        ReleaseRenderContext, render_release_workflow,
    };
    let temp = tempfile::tempdir()?;
    let root = temp.path().canonicalize()?;
    write_workspace_fixture(&root)?;
    let known = std::collections::BTreeSet::from(["other/Cargo.toml".to_owned()]);
    let lock = |manifest: &str| -> Result<_, Box<dyn std::error::Error>> {
        Ok(workspace_lock(
            &metadata(&root, manifest)?,
            &root,
            manifest,
            &known,
            "crates-io",
            "owner/project",
        )?)
    };
    let root_lock = lock("Cargo.toml")?;
    assert_eq!(root_lock, lock("a/Cargo.toml")?);
    assert_eq!(root_lock, lock("b/Cargo.toml")?);
    assert_ne!(root_lock.group, lock("other/Cargo.toml")?.group);
    assert!(!root_lock.cancel_in_progress);
    let (mut spec, rust) = crate::release::release_steps::tests::fixture()?;
    spec.concurrency = root_lock.clone();
    let ctx = ReleaseRenderContext {
        generator_version: env!("CARGO_PKG_VERSION").into(),
        runs_on: "ubuntu-26.04".into(),
    };
    let workflow = render_release_workflow(&spec, &ctx, &rust)?;
    assert!(workflow.contains(&root_lock.group));
    assert_invalid_workspace_metadata(&root, &known)?;
    let original = metadata(&root, "a/Cargo.toml")?;
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(root.join("a"), root.join("alias"))?;
        assert_eq!(
            root_lock,
            workspace_lock(
                &original,
                &root,
                "alias/Cargo.toml",
                &known,
                "crates-io",
                "owner/project"
            )?
        );
        let outside = tempfile::tempdir()?;
        std::os::unix::fs::symlink(outside.path(), root.join("escaped"))?;
        let mut escaped: serde_json::Value = serde_json::from_str(&original)?;
        escaped["workspace_root"] = root.join("escaped").to_string_lossy().to_string().into();
        assert!(
            workspace_lock(
                &serde_json::to_string(&escaped)?,
                &root,
                "a/Cargo.toml",
                &known,
                "crates-io",
                "owner/project"
            )
            .is_err()
        );
    }
    Ok(())
}

fn write_workspace_fixture(root: &Path) -> Result<(), Box<dyn std::error::Error>> {
    write(
        root,
        "Cargo.toml",
        "[workspace]\nmembers=['a','b']\nexclude=['other']\nresolver='2'\n",
    )?;
    for (dir, name) in [("a", "probe_a"), ("b", "probe_b"), ("other", "probe_other")] {
        write(
            root,
            &format!("{dir}/Cargo.toml"),
            &format!(
                "[package]\nname='{name}'\nversion='1.0.0'\nedition='2021'\n{}",
                if dir == "other" {
                    "[workspace]\n"
                } else if dir == "a" {
                    "[dependencies]\nprobe_other={path='../other',version='1'}\n"
                } else {
                    ""
                }
            ),
        )?;
        write(root, &format!("{dir}/src/lib.rs"), "pub fn f() {}\n")?;
    }
    Ok(())
}

fn assert_invalid_workspace_metadata(
    root: &Path,
    known: &std::collections::BTreeSet<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    assert!(
        workspace_lock(
            "not json",
            root,
            "a/Cargo.toml",
            known,
            "crates-io",
            "owner/project"
        )
        .is_err()
    );
    let original = metadata(root, "a/Cargo.toml")?;
    assert!(
        workspace_lock(
            &original,
            root,
            "a/Cargo.toml",
            &std::collections::BTreeSet::new(),
            "crates-io",
            "owner/project"
        )
        .is_err()
    );
    let mut forged: serde_json::Value = serde_json::from_str(&original)?;
    forged["workspace_root"] = root
        .parent()
        .ok_or("parent")?
        .to_string_lossy()
        .to_string()
        .into();
    assert!(
        workspace_lock(
            &serde_json::to_string(&forged)?,
            root,
            "a/Cargo.toml",
            known,
            "crates-io",
            "owner/project"
        )
        .is_err()
    );
    assert!(
        workspace_lock(
            &original,
            root,
            "other/Cargo.toml",
            known,
            "crates-io",
            "owner/project"
        )
        .is_err()
    );
    assert_invalid_workspace_membership(root, known, &original)?;
    Ok(())
}

fn assert_invalid_workspace_membership(
    root: &Path,
    known: &std::collections::BTreeSet<String>,
    original: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut forged_member: serde_json::Value = serde_json::from_str(original)?;
    forged_member["workspace_root"] = root.join("a").to_string_lossy().to_string().into();
    assert!(
        workspace_lock(
            &serde_json::to_string(&forged_member)?,
            root,
            "a/Cargo.toml",
            known,
            "crates-io",
            "owner/project"
        )
        .is_err()
    );
    let mut missing: serde_json::Value = serde_json::from_str(original)?;
    missing
        .as_object_mut()
        .ok_or("metadata object")?
        .remove("workspace_root");
    assert!(
        workspace_lock(
            &serde_json::to_string(&missing)?,
            root,
            "a/Cargo.toml",
            known,
            "crates-io",
            "owner/project"
        )
        .is_err()
    );
    Ok(())
}
