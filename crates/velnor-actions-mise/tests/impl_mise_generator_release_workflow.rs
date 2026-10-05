use velnor_actions_contract::{GeneratorReleaseTarget, Step, StepKind};

use velnor_actions_mise::{
    MISE_VERSION, MR_BOXINGTON_VERSION, RUST_VERSION, ToolCatalog, ToolHomes,
    generator_release_mise_binary_sha256, rust_exec_step, setup_rust_steps,
};

const MISE_USES: &str = "jdx/mise-action@9149ea85001c7435d5a66bb127d6a1b6227cb0a5";

#[test]
fn setup_lowers_target_matched_action_checksum_and_rust_tool_install() {
    let catalog = ToolCatalog::pinned();
    let homes = ToolHomes::runner_temp();
    let expected = [
        (
            GeneratorReleaseTarget::LinuxX86_64,
            "d24fe0bf7e613824ad99f7b8dac3f2b381a37b9f75f84dd250855217095a8de4",
        ),
        (
            GeneratorReleaseTarget::MacosArm64,
            "484c135bd4329975d608d3f77e26c2ece5d2f5590f18ca71f44440294f8cfa6f",
        ),
    ];
    for (target, digest) in expected {
        let steps = setup_rust_steps(
            MISE_USES,
            target,
            generator_release_mise_binary_sha256(target),
            &homes,
            &catalog,
        );
        assert!(steps.is_ok(), "target setup steps must be valid");
        if let Ok(steps) = steps {
            assert!(assert_setup_steps(&steps, target, digest));
        }
    }
}

fn assert_setup_steps(
    steps: &[Step],
    target: GeneratorReleaseTarget,
    expected_digest: &str,
) -> bool {
    assert_eq!(steps.len(), 2);
    let StepKind::Action { uses, with, .. } = &steps[0].kind else {
        return false;
    };
    assert_eq!(uses, MISE_USES);
    assert_eq!(with.get("version").map(String::as_str), Some(MISE_VERSION));
    assert_eq!(with.get("install").map(String::as_str), Some("false"));
    assert_eq!(with.get("env").map(String::as_str), Some("false"));
    assert_eq!(
        with.get("sha256").map(String::as_str),
        Some(expected_digest)
    );
    assert_eq!(
        generator_release_mise_binary_sha256(target),
        expected_digest
    );
    let StepKind::Shell { run, env } = &steps[1].kind else {
        return false;
    };
    let expected_run = ["mise", "--no-config", "--no-env", "--no-hooks", "install"]
        .map(str::to_owned)
        .into_iter()
        .chain([
            format!("rust@{RUST_VERSION}"),
            format!("mr-boxington@{MR_BOXINGTON_VERSION}"),
        ])
        .collect::<Vec<_>>();
    assert_eq!(run, &expected_run);
    assert_eq!(
        env.get("MISE_RUSTUP_HOME").map(String::as_str),
        Some("${{ runner.temp }}/velnor/rustup")
    );
    true
}

#[test]
fn rust_exec_lowers_pinned_cargo_args_and_owned_environment() {
    let catalog = ToolCatalog::pinned();
    let homes = ToolHomes::runner_temp();
    let args = [
        "build",
        "--locked",
        "--release",
        "--target-dir",
        "target",
        "--target",
        "x86_64-unknown-linux-gnu",
    ]
    .map(std::ffi::OsString::from);
    let step = rust_exec_step(
        "Build generator",
        std::ffi::OsStr::new("cargo"),
        &args,
        &homes,
        &catalog,
    );
    assert!(step.is_ok(), "Cargo execution step must be valid");
    if let Ok(step) = step {
        assert!(matches!(step.kind, StepKind::Shell { .. }));
        if let StepKind::Shell { run, env } = step.kind {
            assert!(run.windows(2).any(|pair| pair == ["exec", "rust@1.98.1"]));
            assert!(run.windows(2).any(|pair| pair == ["--", "cargo"]));
            assert!(
                run.windows(2)
                    .any(|pair| pair == ["--target", "x86_64-unknown-linux-gnu"])
            );
            assert!(
                run.windows(2)
                    .any(|pair| pair == ["--target-dir", "target"])
            );
            assert_eq!(
                env.get("RUSTUP_TOOLCHAIN").map(String::as_str),
                Some("1.98.1")
            );
        }
    }
}

#[test]
fn setup_rejects_wrong_mise_action_pin_and_target_checksum() {
    let catalog = ToolCatalog::pinned();
    let homes = ToolHomes::runner_temp();
    let target = GeneratorReleaseTarget::LinuxX86_64;
    let digest = generator_release_mise_binary_sha256(target);
    assert!(setup_rust_steps("jdx/mise-action@v5", target, digest, &homes, &catalog).is_err());
    assert!(setup_rust_steps(MISE_USES, target, "", &homes, &catalog).is_err());
    assert!(setup_rust_steps(MISE_USES, target, &"0".repeat(64), &homes, &catalog).is_err());
    assert!(
        rust_exec_step(
            "Install Rust",
            std::ffi::OsStr::new("cargo"),
            &[
                std::ffi::OsString::from("install"),
                std::ffi::OsString::from("rustup"),
            ],
            &homes,
            &catalog,
        )
        .is_err()
    );
}
