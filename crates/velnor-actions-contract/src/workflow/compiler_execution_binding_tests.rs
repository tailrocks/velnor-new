use super::{
    CompiledRustReportRecipe, CompilerDriver, RustCompilerOperation, RustCompilerTools,
    RustReportFrame,
};
use crate::{
    CompiledSourceHelper, HelperInvocation, SourceBoundHelper, SourceBoundOperation,
    canonical_task_digest, compiled_source_sha256, matrix_id_for_task_group, matrix_key_for_id,
};
use std::collections::BTreeMap;

const TASK_ID: &str = "stack/rust/root/clippy/default";
const TOOLCHAIN: &str = "b3-0000000000000000000000000000000000000000000000000000000000000000";
const RUST_SELECTOR: &str = "rust[profile=minimal,components=clippy,rustfmt]@1.98.1";
const MBX_SELECTOR: &str = "mr-boxington@0.4.0";
const NEXTTEST_SELECTOR: &str = "aqua:nextest-rs/nextest/cargo-nextest@0.9.0";

struct Fixture {
    record: CompiledSourceHelper,
    recipe: CompiledRustReportRecipe,
    frame: RustReportFrame,
    environment: BTreeMap<String, String>,
}

fn cargo_tools() -> RustCompilerTools {
    RustCompilerTools {
        rust: RUST_SELECTOR.to_owned(),
        mbx: None,
        nextest: None,
    }
}

fn clippy_payload() -> Vec<String> {
    vec![
        "clippy".to_owned(),
        "--locked".to_owned(),
        "--offline".to_owned(),
    ]
}

fn valid_recipe() -> Result<CompiledRustReportRecipe, crate::ContractError> {
    let mut recipe = CompiledRustReportRecipe::compiled(
        CompilerDriver::Cargo,
        RustCompilerOperation::Clippy,
        cargo_tools(),
        clippy_payload(),
        TOOLCHAIN.to_owned(),
        TOOLCHAIN.to_owned(),
    )?;
    recipe.expected_task_digest =
        canonical_task_digest(TASK_ID, recipe.compiler_argv(), TOOLCHAIN, None, None)?;
    Ok(recipe)
}

fn frame() -> Result<RustReportFrame, crate::ContractError> {
    let matrix_id = matrix_id_for_task_group("rust", TASK_ID)?;
    RustReportFrame::compiled(
        env!("CARGO_PKG_VERSION"),
        TASK_ID,
        &matrix_key_for_id(&matrix_id)?,
    )
}

fn environment(
    recipe: &CompiledRustReportRecipe,
    frame: &RustReportFrame,
) -> Result<BTreeMap<String, String>, crate::ContractError> {
    let matrix_id = matrix_id_for_task_group("rust", &frame.task_id)?;
    let argv = serde_json::to_string(recipe.compiler_argv())
        .map_err(|_| crate::ContractError::identity("rust_report_wrapper", "frame_json"))?;
    Ok(BTreeMap::from([
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
        ("VELNOR_TASK_ID".to_owned(), frame.task_id.clone()),
        (
            "VELNOR_TASK_DIGEST".to_owned(),
            recipe.expected_task_digest.clone(),
        ),
        ("VELNOR_MATRIX_ID".to_owned(), matrix_id),
        ("VELNOR_MATRIX_KEY".to_owned(), frame.matrix_key.clone()),
        ("VELNOR_RUST_FRAME_ARGV_JSON".to_owned(), argv),
        (
            "VELNOR_RUST_FRAME_TOOLCHAIN".to_owned(),
            recipe.toolchain_id.clone(),
        ),
    ]))
}

fn fixture() -> Result<Fixture, crate::ContractError> {
    let recipe = valid_recipe()?;
    let frame = frame()?;
    let environment = environment(&recipe, &frame)?;
    let record = CompiledSourceHelper::rust_report_wrapper(
        recipe.clone(),
        frame.clone(),
        environment.clone(),
    )?;
    Ok(Fixture {
        record,
        recipe,
        frame,
        environment,
    })
}

fn generic_rust_report() -> Result<CompiledSourceHelper, crate::ContractError> {
    let operation = SourceBoundOperation::RustReportWrapper;
    let source = crate::generated_source(env!("CARGO_PKG_VERSION"), "exit 0\n")?;
    let digest = compiled_source_sha256(source.as_bytes());
    let path = format!("{}{digest}.sh", operation.path());
    let descriptor = SourceBoundHelper::compiled(operation, &path, &digest)?;
    let invocation = HelperInvocation::compiled(descriptor, vec!["cargo".to_owned()], Vec::new())?;
    CompiledSourceHelper::compiled(invocation, source)
}

#[test]
fn issued_record_has_compiler_authority_and_exact_binding() -> Result<(), crate::ContractError> {
    let fixture = fixture()?;
    assert_eq!(
        fixture.record.compiler_driver(),
        Some(CompilerDriver::Cargo)
    );
    fixture.record.validate_binding()?;
    Ok(())
}

#[test]
fn generic_rust_report_has_no_compiler_authority() -> Result<(), crate::ContractError> {
    let record = generic_rust_report()?;
    assert_eq!(record.compiler_driver(), None);
    assert!(record.validate_binding().is_err());
    Ok(())
}

#[test]
fn post_issued_source_mutation_is_rejected() -> Result<(), crate::ContractError> {
    let mut record = fixture()?.record;
    record.source.push_str("tampered\n");
    assert!(record.validate_binding().is_err());
    Ok(())
}

#[test]
fn post_issued_invocation_mutation_is_rejected() -> Result<(), crate::ContractError> {
    let mut record = fixture()?.record;
    record.invocation.args[0] = "mbx".to_owned();
    assert!(record.validate_binding().is_err());
    Ok(())
}

#[test]
fn post_issued_task_digest_mutation_is_rejected() -> Result<(), crate::ContractError> {
    let mut record = fixture()?.record;
    record.invocation.args[2] = format!("b3-{}", "0".repeat(64));
    assert!(record.validate_binding().is_err());
    Ok(())
}

#[test]
fn post_issued_environment_mutation_is_rejected() -> Result<(), crate::ContractError> {
    let mut record = fixture()?.record;
    record
        .environment
        .insert("MISE_NO_CONFIG".to_owned(), "0".to_owned());
    assert!(record.validate_binding().is_err());
    Ok(())
}

#[test]
fn environment_overrides_and_foreign_keys_are_rejected() -> Result<(), crate::ContractError> {
    let fixture = fixture()?;
    for (key, value) in [
        ("MISE_NO_CONFIG", "0"),
        ("RUSTUP_AUTO_INSTALL", "1"),
        ("RUSTUP_TOOLCHAIN", "9.9.9"),
        ("PATH", "/tmp/forged"),
    ] {
        let mut environment = fixture.environment.clone();
        environment.insert(key.to_owned(), value.to_owned());
        assert!(
            CompiledSourceHelper::rust_report_wrapper(
                fixture.recipe.clone(),
                fixture.frame.clone(),
                environment,
            )
            .is_err()
        );
    }
    Ok(())
}

#[test]
fn frame_environment_is_exact() -> Result<(), crate::ContractError> {
    let fixture = fixture()?;
    for (key, value) in [
        ("VELNOR_TASK_ID", "stack/rust/root/clippy/other"),
        ("VELNOR_MATRIX_KEY", "m-0000000000000000"),
        ("VELNOR_MATRIX_ID", "stack:rust|task:forged"),
        (
            "VELNOR_RUST_FRAME_TOOLCHAIN",
            "b3-1111111111111111111111111111111111111111111111111111111111111111",
        ),
    ] {
        let mut environment = fixture.environment.clone();
        environment.insert(key.to_owned(), value.to_owned());
        assert!(
            CompiledSourceHelper::rust_report_wrapper(
                fixture.recipe.clone(),
                fixture.frame.clone(),
                environment,
            )
            .is_err()
        );
    }
    Ok(())
}

#[test]
fn recipe_digest_and_toolchain_mutations_are_rejected() -> Result<(), crate::ContractError> {
    let fixture = fixture()?;
    let mut digest_recipe = fixture.recipe.clone();
    digest_recipe.expected_task_digest = format!("b3-{}", "0".repeat(64));
    assert!(
        CompiledSourceHelper::rust_report_wrapper(
            digest_recipe,
            fixture.frame.clone(),
            fixture.environment.clone(),
        )
        .is_err()
    );

    let mut toolchain_recipe = fixture.recipe.clone();
    toolchain_recipe.toolchain_id = format!("b3-{}", "1".repeat(64));
    assert!(
        CompiledSourceHelper::rust_report_wrapper(
            toolchain_recipe,
            fixture.frame,
            fixture.environment,
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn wrong_task_and_matrix_frames_are_rejected() -> Result<(), crate::ContractError> {
    let fixture = fixture()?;
    let other_task = "stack/rust/root/clippy/other";
    let other_id = matrix_id_for_task_group("rust", other_task)?;
    let other_frame = RustReportFrame::compiled(
        env!("CARGO_PKG_VERSION"),
        other_task,
        &matrix_key_for_id(&other_id)?,
    )?;
    assert!(
        CompiledSourceHelper::rust_report_wrapper(
            fixture.recipe.clone(),
            other_frame,
            fixture.environment.clone(),
        )
        .is_err()
    );

    let wrong_key = "m-0000000000000000";
    assert!(RustReportFrame::compiled(env!("CARGO_PKG_VERSION"), TASK_ID, wrong_key).is_err());
    Ok(())
}

#[test]
fn post_issued_matrix_frame_mutation_is_rejected() -> Result<(), crate::ContractError> {
    let fixture = fixture()?;
    let mut frame = fixture.frame;
    frame.matrix_key = "m-0000000000000000".to_owned();
    assert!(
        CompiledSourceHelper::rust_report_wrapper(fixture.recipe, frame, fixture.environment,)
            .is_err()
    );
    Ok(())
}

#[test]
fn selector_slots_must_match_the_explicit_driver() {
    let payload = clippy_payload();
    let cargo_with_mbx = CompiledRustReportRecipe::compiled(
        CompilerDriver::Cargo,
        RustCompilerOperation::Clippy,
        RustCompilerTools {
            rust: RUST_SELECTOR.to_owned(),
            mbx: Some(MBX_SELECTOR.to_owned()),
            nextest: None,
        },
        payload.clone(),
        TOOLCHAIN.to_owned(),
        TOOLCHAIN.to_owned(),
    );
    assert!(cargo_with_mbx.is_err());

    let mbx_without_selector = CompiledRustReportRecipe::compiled(
        CompilerDriver::Mbx,
        RustCompilerOperation::Clippy,
        cargo_tools(),
        payload,
        TOOLCHAIN.to_owned(),
        TOOLCHAIN.to_owned(),
    );
    assert!(mbx_without_selector.is_err());
}

#[test]
fn nextest_requires_its_explicit_selector() {
    let missing = CompiledRustReportRecipe::compiled(
        CompilerDriver::Cargo,
        RustCompilerOperation::Nextest,
        cargo_tools(),
        vec![
            "nextest".to_owned(),
            "run".to_owned(),
            "--locked".to_owned(),
            "--offline".to_owned(),
        ],
        TOOLCHAIN.to_owned(),
        TOOLCHAIN.to_owned(),
    );
    assert!(missing.is_err());

    let mut tools = cargo_tools();
    tools.nextest = Some(NEXTTEST_SELECTOR.to_owned());
    let accepted = CompiledRustReportRecipe::compiled(
        CompilerDriver::Cargo,
        RustCompilerOperation::Nextest,
        tools,
        vec![
            "nextest".to_owned(),
            "run".to_owned(),
            "--locked".to_owned(),
            "--offline".to_owned(),
        ],
        TOOLCHAIN.to_owned(),
        TOOLCHAIN.to_owned(),
    );
    assert!(accepted.is_ok());
}

#[test]
fn echo_help_and_version_payloads_are_rejected() {
    for payload in [
        vec!["echo", "mbx", "--locked", "--offline"],
        vec!["clippy", "--locked", "--offline", "--help"],
        vec!["clippy", "--locked", "--offline", "--version"],
    ] {
        let payload = payload.into_iter().map(str::to_owned).collect();
        assert!(
            CompiledRustReportRecipe::compiled(
                CompilerDriver::Cargo,
                RustCompilerOperation::Clippy,
                cargo_tools(),
                payload,
                TOOLCHAIN.to_owned(),
                TOOLCHAIN.to_owned(),
            )
            .is_err()
        );
    }
}

#[test]
fn actual_driver_program_position_cannot_be_forged() -> Result<(), crate::ContractError> {
    let fixture = fixture()?;
    let mut recipe = fixture.recipe.clone();
    recipe.compiler_argv[7] = "mbx".to_owned();
    recipe.expected_task_digest =
        canonical_task_digest(TASK_ID, recipe.compiler_argv(), TOOLCHAIN, None, None)?;
    let environment = environment(&recipe, &fixture.frame)?;
    assert!(
        CompiledSourceHelper::rust_report_wrapper(recipe, fixture.frame, environment,).is_err()
    );
    Ok(())
}
