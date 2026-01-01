//! OW3 remediation: source hygiene cases (split from `impl_orch_gen`).

use std::fs;
use std::path::PathBuf;

use crate::impl_common::TestResult;

#[test]
fn orch_gen_no_direct_process_spawn_in_source() -> TestResult {
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    for entry in fs::read_dir(&src)? {
        files.push(entry?.path());
    }
    files.sort();
    assert!(!files.is_empty(), "orchestrator src present");
    for path in files {
        if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
            continue;
        }
        let text = fs::read_to_string(&path)?;
        for banned in ["process::Command", "Command::new", ".spawn("] {
            assert!(
                !text.contains(banned),
                "{} contains {banned}",
                path.display()
            );
        }
    }
    Ok(())
}

#[test]
fn orch_gen_plan_module_has_no_second_discovery() -> TestResult {
    let text = fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/plan.rs"))?;
    for banned in ["discover(", "build_workflow("] {
        assert!(!text.contains(banned), "plan.rs must not call {banned}");
    }
    Ok(())
}
