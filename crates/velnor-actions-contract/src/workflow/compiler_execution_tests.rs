#![cfg(unix)]

use super::{
    CompiledRustReportRecipe, CompilerDriver, RustCompilerOperation, RustCompilerTools,
    RustReportFrame,
};
use crate::{
    CompiledSourceHelper, canonical_task_digest, matrix_id_for_task_group, matrix_key_for_id,
};
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const TASK_ID: &str = "stack/rust/root/clippy/default";
const TOOLCHAIN: &str = "b3-0000000000000000000000000000000000000000000000000000000000000000";
const PREEXEC_OP: &str = "validate-rust-report-preexec-v1";

#[test]
fn compiled_wrapper_runs_preexec_before_clock_compiler_and_report()
-> Result<(), Box<dyn std::error::Error>> {
    let (record, recipe, frame) = fixture()?;
    let root = TempRoot::new()?;
    let helper = root.write_executable(
        format!("velnor/bin/velnor-actions-{}", frame.version),
        HELPER_STUB,
    )?;
    let _compiler = root.write_executable("velnor/mise/bin/mise", COMPILER_STUB)?;

    let output = Command::new("/bin/bash")
        .args(["-p", "-c", record.source()])
        .env_clear()
        .env("RUNNER_TEMP", root.path())
        .output()?;
    assert_eq!(
        output.status.code(),
        Some(23),
        "stderr: {:?}",
        output.stderr
    );
    assert_eq!(
        fs::read_to_string(root.path().join("ops"))?,
        format!("{PREEXEC_OP}\n")
    );
    assert_eq!(
        fs::read_to_string(root.path().join("argv"))?,
        format!("{}\n", serde_json::to_string(recipe.compiler_argv())?)
    );
    assert_eq!(
        fs::read_to_string(root.path().join("digest"))?,
        format!("{}\n", recipe.expected_task_digest)
    );
    assert_eq!(
        fs::read_to_string(root.path().join("task-id"))?,
        format!("{TASK_ID}\n")
    );
    assert_eq!(
        fs::read_to_string(root.path().join("toolchain"))?,
        format!("{TOOLCHAIN}\n")
    );
    assert!(!root.path().join("compiler-marker").exists());
    assert!(
        !root
            .path()
            .join(format!("velnor/start-{}", frame.matrix_key))
            .exists()
    );
    assert!(!fs::read_to_string(root.path().join("ops"))?.contains("write-task-report-v1"));
    assert!(helper.exists());
    Ok(())
}

fn fixture() -> Result<
    (
        CompiledSourceHelper,
        CompiledRustReportRecipe,
        RustReportFrame,
    ),
    crate::ContractError,
> {
    let mut recipe = CompiledRustReportRecipe::compiled(
        CompilerDriver::Cargo,
        RustCompilerOperation::Clippy,
        RustCompilerTools {
            rust: "rust[profile=minimal,components=clippy,rustfmt]@1.98.1".to_owned(),
            mbx: None,
            nextest: None,
        },
        vec![
            "clippy".to_owned(),
            "--locked".to_owned(),
            "--offline".to_owned(),
        ],
        TOOLCHAIN.to_owned(),
        TOOLCHAIN.to_owned(),
    )?;
    recipe.expected_task_digest =
        canonical_task_digest(TASK_ID, recipe.compiler_argv(), TOOLCHAIN, None, None)?;
    let matrix_id = matrix_id_for_task_group("rust", TASK_ID)?;
    let frame = RustReportFrame::compiled(
        env!("CARGO_PKG_VERSION"),
        TASK_ID,
        &matrix_key_for_id(&matrix_id)?,
    )?;
    let environment = fixture_environment(&recipe, &frame, matrix_id)?;
    let record =
        CompiledSourceHelper::rust_report_wrapper(recipe.clone(), frame.clone(), environment)?;
    Ok((record, recipe, frame))
}

fn fixture_environment(
    recipe: &CompiledRustReportRecipe,
    frame: &RustReportFrame,
    matrix_id: String,
) -> Result<BTreeMap<String, String>, crate::ContractError> {
    let argv_json = serde_json::to_string(recipe.compiler_argv())
        .map_err(|_| crate::ContractError::identity("rust_report_wrapper", "frame_json"))?;
    let environment = BTreeMap::from([
        ("MISE_NO_CONFIG".to_owned(), "1".to_owned()),
        ("MISE_NO_ENV".to_owned(), "1".to_owned()),
        ("MISE_NO_HOOKS".to_owned(), "1".to_owned()),
        ("MISE_LOCKFILE".to_owned(), "0".to_owned()),
        ("RUSTUP_AUTO_INSTALL".to_owned(), "0".to_owned()),
        ("MISE_AUTO_INSTALL".to_owned(), "false".to_owned()),
        ("MISE_EXEC_AUTO_INSTALL".to_owned(), "false".to_owned()),
        (
            "MISE_RUSTUP_HOME".to_owned(),
            "${{ runner.temp }}/velnor/rustup".to_owned(),
        ),
        (
            "MISE_CARGO_HOME".to_owned(),
            "${{ runner.temp }}/velnor/cargo".to_owned(),
        ),
        (
            "MISE_DATA_DIR".to_owned(),
            "${{ runner.temp }}/velnor/mise".to_owned(),
        ),
        ("RUSTUP_TOOLCHAIN".to_owned(), "1.98.1".to_owned()),
        ("VELNOR_TASK_ID".to_owned(), TASK_ID.to_owned()),
        (
            "VELNOR_TASK_DIGEST".to_owned(),
            recipe.expected_task_digest.clone(),
        ),
        ("VELNOR_MATRIX_ID".to_owned(), matrix_id),
        ("VELNOR_MATRIX_KEY".to_owned(), frame.matrix_key.clone()),
        ("VELNOR_RUST_FRAME_ARGV_JSON".to_owned(), argv_json),
        (
            "VELNOR_RUST_FRAME_TOOLCHAIN".to_owned(),
            TOOLCHAIN.to_owned(),
        ),
    ]);
    Ok(environment)
}

struct TempRoot {
    path: PathBuf,
}

impl TempRoot {
    fn new() -> io::Result<Self> {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        for attempt in 0..100u32 {
            let path = std::env::temp_dir().join(format!(
                "velnor-compiler-execution-{}-{stamp}-{attempt}",
                std::process::id()
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Ok(Self { path }),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "temp root collision",
        ))
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn write_executable(&self, relative: impl AsRef<Path>, source: &str) -> io::Result<PathBuf> {
        use std::os::unix::fs::PermissionsExt;

        let path = self.path.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, source)?;
        let mut permissions = fs::metadata(&path)?.permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&path, permissions)?;
        Ok(path)
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.path) {
            eprintln!(
                "failed to remove test temp root {}: {error}",
                self.path.display()
            );
        }
    }
}

const HELPER_STUB: &str = r#"#!/bin/sh
printf '%s\n' "${VELNOR_INTERNAL_OP-}" >> "$RUNNER_TEMP/ops"
printf '%s\n' "${VELNOR_RUST_FRAME_ARGV_JSON-}" >> "$RUNNER_TEMP/argv"
printf '%s\n' "${VELNOR_TASK_DIGEST-}" >> "$RUNNER_TEMP/digest"
printf '%s\n' "${VELNOR_TASK_ID-}" >> "$RUNNER_TEMP/task-id"
printf '%s\n' "${VELNOR_RUST_FRAME_TOOLCHAIN-}" >> "$RUNNER_TEMP/toolchain"
exit 23
"#;

const COMPILER_STUB: &str = r#"#!/bin/sh
printf '%s\n' compiler > "$RUNNER_TEMP/compiler-marker"
exit 99
"#;
