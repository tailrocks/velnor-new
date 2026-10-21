use super::*;
use velnor_actions_contract::StepKind;

#[test]
fn fixed_seed_barrier_is_sequential_scrubbed_and_preserves_exact_helper_bytes() {
    let step = seed_parallel_targets_step("crates/work/target", &["velnor-clippy", "velnor-doc"])
        .expect("fixed native preparation");
    assert!(!step.parallel);
    assert_eq!(step.role, None);
    let StepKind::Shell { run, env } = step.kind else {
        panic!("shell preparation")
    };
    assert_eq!(env.get("GH_TOKEN").map(String::as_str), Some(""));
    let script = run.last().expect("script");
    assert!(script.contains("exec python3 -I -c "));
    assert!(script.contains("unset "));
    let python = format!("exec({})", canonical_json_str(SEED).expect("quoted source"));
    assert!(script.contains(&commands::quote_script_arg(&python)));
    assert!(script.ends_with("crates/work/target velnor-clippy velnor-doc"));
    commands::join_argv_for_run(&run).expect("existing command confinement");
}

#[test]
fn fixed_seed_barrier_rejects_path_lane_and_shell_injection() {
    for target in [
        "/target",
        "../target",
        "a//target",
        "a/../target",
        "target/",
        "a;id/target",
        "other",
    ] {
        assert!(
            seed_parallel_targets_step(target, &["velnor-clippy"]).is_err(),
            "{target}"
        );
    }
    for lanes in [
        vec![],
        vec!["velnor-clippy", "velnor-clippy"],
        vec!["../doc"],
        vec!["velnor-doc;id"],
    ] {
        assert!(seed_parallel_targets_step("target", &lanes).is_err());
    }
}

/// The helper needs `shutil.rmtree(dir_fd=...)`, added in Python 3.11. Select a
/// qualifying interpreter instead of trusting whatever `python3` the host has
/// first on `PATH` (macOS ships 3.9).
fn python_for_seed_helper() -> String {
    let probe = "import sys; sys.exit(0 if sys.version_info >= (3, 11) else 1)";
    let candidates = std::env::var("VELNOR_TEST_PYTHON").into_iter().chain(
        [
            "python3",
            "python3.15",
            "python3.14",
            "python3.13",
            "python3.12",
            "python3.11",
        ]
        .map(str::to_owned),
    );
    let mut tried = Vec::new();
    for candidate in candidates {
        let qualifies = std::process::Command::new(&candidate)
            .args(["-I", "-c", probe])
            .status()
            .is_ok_and(|status| status.success());
        if qualifies {
            return candidate;
        }
        tried.push(candidate);
    }
    panic!(
        "The native target seed helper needs Python 3.11 or later. Tried: {tried:?}. Set VELNOR_TEST_PYTHON."
    );
}

#[test]
fn shipped_parallel_target_filesystem_regressions_pass() {
    let scripts =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/parallel-targets");
    let output = std::process::Command::new(python_for_seed_helper())
        .args(["-B", "-m", "unittest", "-v", "test_seed"])
        .current_dir(scripts)
        .output()
        .expect("Python required for the native target-seeding helper");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
