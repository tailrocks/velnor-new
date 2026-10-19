//! CLI gate coverage for the Cache seed internal ops.
use std::error::Error;

use crate::impl_cli_gate::assert_identical;
use crate::impl_cli_tmp::{cleanup, code, fresh_tempdir, spawn_isolated};

#[test]
fn seed_ops_need_runner_temp() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("gate-seed")?;
    let bare = spawn_isolated(&[], &[], &tmp)?;
    for op in ["resolve-seed-task-v1", "seed-skeleton-v1", "seed-trim-v1"] {
        let gated = spawn_isolated(&[], &[("VELNOR_INTERNAL_OP", op)], &tmp)?;
        assert_eq!(code(&gated), 2, "{op}");
        assert_identical(&bare, &gated);
    }
    cleanup(&tmp);
    Ok(())
}

#[test]
fn seed_skeleton_stubs_members_from_the_staged_metadata() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("gate-seed-run")?.canonicalize()?;
    let workspace = tmp.join("ws");
    let lib = workspace.join("app/src/lib.rs");
    std::fs::create_dir_all(lib.parent().ok_or("parent")?)?;
    std::fs::write(&lib, "pub fn real() {}\n")?;
    let seed = tmp.join("rt/velnor/seed");
    std::fs::create_dir_all(&seed)?;
    let metadata = serde_json::json!({
        "packages": [{"id": "path+file:///ws/app#0.1.0", "source": null,
            "targets": [{"kind": ["lib"], "src_path": lib}]}],
        "workspace_members": ["path+file:///ws/app#0.1.0"],
        "resolve": {"nodes": []}
    });
    std::fs::write(seed.join("metadata.json"), metadata.to_string())?;
    let output = spawn_isolated(
        &[],
        &[
            ("VELNOR_INTERNAL_OP", "seed-skeleton-v1"),
            ("RUNNER_TEMP", tmp.join("rt").to_str().ok_or("utf8")?),
            ("GITHUB_WORKSPACE", workspace.to_str().ok_or("utf8")?),
            ("VELNOR_SEED_ROOT", "."),
        ],
        &tmp,
    )?;
    assert_eq!(
        code(&output),
        0,
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(&lib)?,
        "//! Velnor dependency seed stub.\n"
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("stubbed 1 files"));
    cleanup(&tmp);
    Ok(())
}
