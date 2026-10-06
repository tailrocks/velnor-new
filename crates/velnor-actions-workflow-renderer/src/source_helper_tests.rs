use super::{source_helper_step, step_to_yaml, validate_registry};
use std::collections::BTreeMap;
use velnor_actions_contract::{
    CompiledNativeExecRecipe, CompiledSourceHelper, HelperInvocation, SourceBoundHelper,
    SourceBoundOperation, StepKind,
};

fn record() -> CompiledSourceHelper {
    let op = SourceBoundOperation::RustPrepareRootLinux;
    let source = velnor_actions_contract::generated_source("0.1.0", "exit 0\n").expect("source");
    let digest = velnor_actions_contract::compiled_source_sha256(source.as_bytes());
    let descriptor = SourceBoundHelper::compiled(op, op.path(), &digest).expect("descriptor");
    let invocation =
        HelperInvocation::compiled(descriptor, vec!["tools".into()], vec!["rust@1.98.1".into()])
            .expect("invocation");
    CompiledSourceHelper::compiled(invocation, source).expect("record")
}

#[test]
fn registry_admits_exact_invocation_and_environment_only() {
    let record = record();
    let step = source_helper_step("Prepare", &record, BTreeMap::new()).expect("step");
    let StepKind::SourceBoundHelper { invocation, env } = &step.kind else {
        panic!("helper")
    };
    assert!(
        step_to_yaml(
            &step,
            invocation,
            env,
            std::slice::from_ref(&record),
            "ubuntu-26.04"
        )
        .is_ok()
    );
    assert!(step_to_yaml(&step, invocation, env, &[], "ubuntu-26.04").is_err());
    let mut wire = serde_json::to_value(invocation).expect("serialize");
    wire["args"] = serde_json::json!(["planning-bootstrap"]);
    let foreign_args: HelperInvocation = serde_json::from_value(wire).expect("shape only");
    assert!(
        step_to_yaml(
            &step,
            &foreign_args,
            env,
            std::slice::from_ref(&record),
            "ubuntu-26.04"
        )
        .is_err()
    );
    let mut wire = serde_json::to_value(invocation).expect("serialize");
    wire["helper"]["source_sha256"] = serde_json::json!("cd".repeat(32));
    let foreign_digest: HelperInvocation = serde_json::from_value(wire).expect("shape only");
    assert!(
        step_to_yaml(
            &step,
            &foreign_digest,
            env,
            std::slice::from_ref(&record),
            "ubuntu-26.04"
        )
        .is_err()
    );
    let mut wire = serde_json::to_value(invocation).expect("serialize");
    wire["helper"]["operation"] = serde_json::json!("npm-public-source-producer");
    wire["helper"]["path"] =
        serde_json::json!(SourceBoundOperation::NpmPublicSourceProducer.path());
    let foreign_operation: HelperInvocation = serde_json::from_value(wire).expect("shape only");
    assert!(step_to_yaml(&step, &foreign_operation, env, &[record], "ubuntu-26.04").is_err());
}

#[test]
fn credentials_startup_and_unapproved_environment_fail() {
    let record = record();
    for key in ["GH_TOKEN", "BASH_ENV", "ENV", "LD_PRELOAD", "HOME"] {
        let env = BTreeMap::from([(key.to_owned(), "attacker".to_owned())]);
        assert!(
            source_helper_step("Prepare", &record, env).is_err(),
            "{key}"
        );
    }
}

#[test]
fn distinct_approved_environments_do_not_depend_on_registry_order() {
    let first = record();
    let env = BTreeMap::from([("HOME".to_owned(), "/owned/home".to_owned())]);
    let second = first.clone().with_environment(env.clone());
    let step = source_helper_step("Prepare", &second, env).expect("step");
    let StepKind::SourceBoundHelper { invocation, env } = &step.kind else {
        panic!("helper")
    };
    assert!(step_to_yaml(&step, invocation, env, &[first, second], "ubuntu-26.04").is_ok());
}

#[test]
fn registry_rejects_conflicting_auxiliary_sources_and_marker_versions() {
    let first = record();
    let second = CompiledSourceHelper::compiled(
        first.invocation().clone(),
        velnor_actions_contract::generated_source("0.1.0", "exit 1\n").expect("source"),
    )
    .expect("record");
    assert!(validate_registry(&[first.clone(), second], "0.1.0").is_err());
    assert!(validate_registry(&[first], "0.2.0").is_err());
}

#[test]
fn descriptors_and_sources_have_closed_paths_and_size_limits() {
    let op = SourceBoundOperation::RustPrepareRootLinux;
    assert!(SourceBoundHelper::compiled(op, "../arbitrary", &"ab".repeat(32)).is_err());
    assert!(SourceBoundHelper::compiled(op, op.path(), &"AB".repeat(32)).is_err());
    let record = record();
    let source =
        velnor_actions_contract::generated_source("0.1.0", &"x".repeat(262_144)).expect("source");
    assert!(CompiledSourceHelper::compiled(record.invocation().clone(), source).is_err());
}

#[test]
fn github_interpolation_cannot_change_source_authority() {
    let first = record();
    let source = velnor_actions_contract::generated_source("0.1.0", "echo '${{ github.token }}'\n")
        .expect("source");
    let altered =
        CompiledSourceHelper::compiled(first.invocation().clone(), source).expect("record");
    assert!(source_helper_step("Prepare", &altered, BTreeMap::new()).is_err());
    let invocation = HelperInvocation::compiled(
        first.invocation().descriptor().clone(),
        vec!["${{ github.token }}".into()],
        Vec::new(),
    )
    .expect("shape");
    let altered =
        CompiledSourceHelper::compiled(invocation, first.source().to_owned()).expect("record");
    assert!(source_helper_step("Prepare", &altered, BTreeMap::new()).is_err());
}

#[test]
fn execution_recipe_binding_rejects_edited_prefix_and_environment() {
    let selectors = vec!["rust@1.98.1".to_owned()];
    let recipe = CompiledNativeExecRecipe::compiled(
        vec![
            "env".to_owned(),
            "-i".to_owned(),
            "/owned/mise".to_owned(),
            "exec".to_owned(),
            "rust@1.98.1".to_owned(),
            "--".to_owned(),
        ],
        BTreeMap::from([("HOME".to_owned(), "/owned/home".to_owned())]),
        selectors,
    )
    .expect("recipe");
    let bound = record().with_execution_recipe(recipe).expect("bind recipe");
    assert!(bound.validate_binding().is_ok());

    let edited_environment = bound
        .clone()
        .with_environment(BTreeMap::from([("HOME".to_owned(), "/edited".to_owned())]));
    assert!(edited_environment.validate_binding().is_err());

    let mut wire = serde_json::to_value(bound.invocation()).expect("serialize invocation");
    wire["execution_prefix"] =
        serde_json::json!(["env", "-i", "/tampered/mise", "exec", "rust@1.98.1", "--"]);
    let edited_invocation: HelperInvocation =
        serde_json::from_value(wire).expect("edited invocation shape");
    let edited = CompiledSourceHelper::compiled(edited_invocation, bound.source().to_owned())
        .expect("edited source record");
    assert!(edited.validate_binding().is_err());
}

#[test]
fn github_output_capability_is_oci_only_and_runner_bound() {
    assert!(record().with_github_output().is_err());

    let forged_environment =
        compiled_record(SourceBoundOperation::OciDelivery, "exit 0\n", Vec::new())
            .with_environment(BTreeMap::from([(
                "GITHUB_OUTPUT".to_owned(),
                "/forged/output".to_owned(),
            )]));
    assert!(forged_environment.with_github_output().is_err());

    let unbound = compiled_record(SourceBoundOperation::OciDelivery, "exit 0\n", Vec::new())
        .with_github_output()
        .expect("OCI output capability");
    assert!(unbound.validate_binding().is_err());

    let recipe = CompiledNativeExecRecipe::compiled(
        vec![
            "/usr/bin/env".to_owned(),
            "-i".to_owned(),
            "/usr/bin/mise".to_owned(),
            "oci@1.0.0".to_owned(),
            "--".to_owned(),
        ],
        BTreeMap::new(),
        vec!["oci@1.0.0".to_owned()],
    )
    .expect("recipe");
    let bound = compiled_record(SourceBoundOperation::OciDelivery, "exit 0\n", Vec::new())
        .with_execution_recipe(recipe)
        .expect("recipe binding")
        .with_github_output()
        .expect("OCI output capability");
    assert!(bound.validate_binding().is_ok());
    assert!(source_helper_step("OCI delivery", &bound, BTreeMap::new()).is_ok());
    let mut edited_step_environment = BTreeMap::new();
    edited_step_environment.insert("GITHUB_OUTPUT".to_owned(), "/edited/output".to_owned());
    assert!(source_helper_step("OCI delivery", &bound, edited_step_environment).is_err());
    let edited = bound.with_environment(BTreeMap::from([(
        "GITHUB_OUTPUT".to_owned(),
        "/edited/output".to_owned(),
    )]));
    assert!(edited.validate_binding().is_err());
}

fn compiled_record(
    operation: SourceBoundOperation,
    body: &str,
    arguments: Vec<String>,
) -> CompiledSourceHelper {
    let source = velnor_actions_contract::generated_source("0.1.0", body).expect("source");
    let digest = velnor_actions_contract::compiled_source_sha256(source.as_bytes());
    let descriptor =
        SourceBoundHelper::compiled(operation, operation.path(), &digest).expect("descriptor");
    let invocation =
        HelperInvocation::compiled(descriptor, arguments, Vec::new()).expect("invocation");
    CompiledSourceHelper::compiled(invocation, source).expect("record")
}

fn assert_chunks(environment: &BTreeMap<String, String>, name: &str) {
    let count_key = format!("VELNOR_COMPILED_HELPER_{name}_COUNT");
    let count = environment
        .get(&count_key)
        .expect("chunk count")
        .parse::<usize>()
        .expect("chunk count integer");
    assert!(count > 0);
    for index in 0..count {
        let key = format!("VELNOR_COMPILED_HELPER_{name}_{index:04}");
        let chunk = environment.get(&key).expect("chunk");
        assert!(chunk.len() <= 8192);
        assert!(chunk.is_ascii());
    }
}

fn realistic_record() -> (CompiledSourceHelper, String, Vec<String>) {
    let mut lock_hcl = String::from(concat!(
        "provider \"registry.opentofu.org/hashicorp/aws\" {\n",
        "  version = \"5.0.0\"\n",
        "  hashes = [\n",
    ));
    for index in 0..180 {
        lock_hcl.push_str(&format!("    \"zh:{index:064x}\",\n"));
    }
    lock_hcl.push_str("  ]\n}\n");
    lock_hcl.push_str(&" ".repeat(16 * 1024 - lock_hcl.len()));
    assert_eq!(lock_hcl.len(), 16 * 1024);
    let lock = lock_hcl
        .bytes()
        .map(|byte| format!("\\{byte:03o}"))
        .collect::<String>();
    let integrity = format!("sha512-{}==", "A".repeat(86));
    let descriptors = (0..1024)
        .map(|index| {
            serde_json::json!({
                "name": format!("npm-package-{index}"),
                "version": "1.0.0",
                "resolved": format!("https://registry.npmjs.org/npm-package-{index}/-npm-package-{index}-1.0.0.tgz"),
                "integrity": integrity.clone(),
            })
        })
        .map(|descriptor| serde_json::to_string(&descriptor).expect("npm descriptor"))
        .collect::<Vec<_>>();
    let mut arguments = vec!["--tofu-lock-hcl-octal".to_owned(), lock.clone()];
    arguments.push("--native-npm-source".to_owned());
    arguments.extend(descriptors.iter().cloned());
    let body = "printf '%s\\n' source-bound\n";
    (
        compiled_record(SourceBoundOperation::TofuProviderExport, body, arguments),
        lock,
        descriptors,
    )
}

#[test]
fn transport_emits_bounded_reserved_chunks_without_payload_in_run() {
    let (record, lock, descriptors) = realistic_record();
    let mut environment = BTreeMap::new();
    let run =
        super::transport::encode(&record, &mut environment, "ubuntu-26.04").expect("transport");
    assert!(run.chars().count() <= 21_000);
    assert!(!run.contains(&lock));
    for descriptor in descriptors {
        assert!(!run.contains(&descriptor));
    }
    assert_eq!(
        environment.get("VELNOR_COMPILED_HELPER_SCHEMA"),
        Some(&"1".to_owned())
    );
    let environment_bytes = environment
        .iter()
        .map(|(key, value)| key.len() + value.len() + 2)
        .sum::<usize>();
    let pointers = 8 * (environment.len() + record.invocation().args().len() + 16);
    assert!(environment_bytes + pointers + 65_536 + run.len() < 1_048_576);
    assert_chunks(&environment, "SOURCE");
    assert_chunks(&environment, "ARGUMENTS");
    assert_chunks(&environment, "EXECUTION");
    assert!(super::transport::encode(&record, &mut BTreeMap::new(), "macos-26").is_ok());
}

#[test]
fn transport_accepts_full_linux_budget_and_rejects_macos_overflow() {
    let marker = velnor_actions_contract::generated_source("0.1.0", "").expect("marker");
    let body = "x".repeat(262_144 - marker.len());
    let arguments = vec!["x".repeat(65_536); 8];
    let record = compiled_record(SourceBoundOperation::RustPrepareRootLinux, &body, arguments);
    assert_eq!(record.source().len(), 262_144);
    assert_eq!(
        record
            .invocation()
            .args()
            .iter()
            .map(String::len)
            .sum::<usize>(),
        524_288
    );
    assert!(super::transport::encode(&record, &mut BTreeMap::new(), "ubuntu-26.04").is_ok());
    assert!(super::transport::encode(&record, &mut BTreeMap::new(), "ubuntu-24.04-arm").is_ok());
    let error = super::transport::encode(&record, &mut BTreeMap::new(), "macos-26")
        .expect_err("macOS environment budget");
    assert!(
        error
            .to_string()
            .contains("unsupported_environment_limit:macos-26")
    );
}

#[test]
fn transport_admits_fixed_oci_arm_runner_and_rejects_unknown_runner() {
    let record = compiled_record(SourceBoundOperation::OciDelivery, "exit 0\n", Vec::new());
    assert!(super::transport::encode(&record, &mut BTreeMap::new(), "ubuntu-24.04-arm").is_ok());
    assert!(super::transport::encode(&record, &mut BTreeMap::new(), "ubuntu-24.04-arm64").is_err());
    assert!(
        super::transport::encode(&record, &mut BTreeMap::new(), "${{ matrix.runner }}").is_err()
    );
}

#[test]
fn transport_rejects_linux_oversized_single_args_and_owner_values() {
    let argument_record = compiled_record(
        SourceBoundOperation::RustPrepareRootLinux,
        "exit 0\n",
        vec!["x".repeat(131_072)],
    );
    assert!(
        super::transport::encode(&argument_record, &mut BTreeMap::new(), "ubuntu-26.04").is_err()
    );

    let environment_record = compiled_record(
        SourceBoundOperation::RustPrepareRootLinux,
        "exit 0\n",
        Vec::new(),
    )
    .with_environment(BTreeMap::from([(
        "OWNER_PAYLOAD".to_owned(),
        "x".repeat(131_073),
    )]));
    let mut environment = environment_record.environment().clone();
    assert!(
        super::transport::encode(&environment_record, &mut environment, "ubuntu-26.04").is_err()
    );

    let oversized_json = serde_json::to_string(&vec!["x"; 40_000]).expect("JSON array");
    assert!(oversized_json.len() > 131_072);
    let json_record = compiled_record(
        SourceBoundOperation::RustPrepareRootLinux,
        "exit 0\n",
        vec![oversized_json],
    );
    assert!(super::transport::encode(&json_record, &mut BTreeMap::new(), "ubuntu-26.04").is_err());
}

#[test]
fn macos_allows_two_hundred_kilobyte_arg_when_total_budget_fits() {
    let record = compiled_record(
        SourceBoundOperation::RustPrepareDesktopMac,
        "exit 0\n",
        vec!["x".repeat(200_000)],
    );
    assert!(super::transport::encode(&record, &mut BTreeMap::new(), "macos-26").is_ok());
}
