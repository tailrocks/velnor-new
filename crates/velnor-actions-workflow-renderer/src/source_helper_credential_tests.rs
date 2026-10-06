use super::credentials::validate_record;
use std::collections::BTreeMap;
use velnor_actions_contract::workflow::native_tools::NativeCredentialScope;
use velnor_actions_contract::{
    CompiledNativeExecRecipe, CompiledSourceHelper, HelperInvocation, SourceBoundHelper,
    SourceBoundOperation,
};

#[path = "source_helper_apple_credential_tests.rs"]
mod apple;
#[path = "source_helper_observer_credential_tests.rs"]
mod observer;

const GH_TOKEN: &str = "${{ github.token }}";
const APT_PRIVATE_KEY: &str = "${{ secrets.APT_GPG_PRIVATE_KEY }}";
const APT_PASSPHRASE: &str = "${{ secrets.APT_GPG_PASSPHRASE }}";
const OCI_CONFIG: &str = "${{ runner.temp }}/velnor/oci-docker";

fn record(
    operation: SourceBoundOperation,
    scope: NativeCredentialScope,
    environment: BTreeMap<String, String>,
) -> CompiledSourceHelper {
    let source = velnor_actions_contract::generated_source("0.1.0", "exit 0\n").expect("source");
    let digest = velnor_actions_contract::compiled_source_sha256(source.as_bytes());
    let path = match operation {
        SourceBoundOperation::NativeSwiftExecution | SourceBoundOperation::NativeRustExecution => {
            format!("{}{}.sh", operation.path(), digest)
        }
        _ => operation.path().to_owned(),
    };
    let descriptor = SourceBoundHelper::compiled(operation, &path, &digest).expect("descriptor");
    let selectors = vec!["python@3.14.0".to_owned()];
    let arguments = match (operation, scope) {
        (SourceBoundOperation::OciDelivery, NativeCredentialScope::Anonymous) => {
            vec!["record", "-", "-", "-"]
        }
        (SourceBoundOperation::OciDelivery, NativeCredentialScope::GithubReadOnly) => {
            vec!["verify", "owner/repository", "ci.yml", "main"]
        }
        (SourceBoundOperation::OciDelivery, NativeCredentialScope::OciRegistryPublish) => {
            vec!["admission", "-", "-", "-"]
        }
        _ => Vec::new(),
    }
    .into_iter()
    .map(str::to_owned)
    .collect();
    let invocation =
        HelperInvocation::compiled(descriptor, arguments, selectors.clone()).expect("invocation");
    let mut recipe_environment = environment.clone();
    let recipe = CompiledNativeExecRecipe::compiled_for_scope(
        [
            vec!["/usr/bin/env".to_owned(), "-i".to_owned()],
            recipe_environment
                .iter()
                .map(|(key, _)| format!("{key}=${{{key}}}"))
                .collect(),
            vec![
                "/owned/mise".to_owned(),
                "--no-config".to_owned(),
                "--no-env".to_owned(),
                "--no-hooks".to_owned(),
                "exec".to_owned(),
            ],
            selectors,
            vec!["--".to_owned()],
        ]
        .concat(),
        std::mem::take(&mut recipe_environment),
        vec!["python@3.14.0".to_owned()],
        scope,
    )
    .expect("recipe");
    CompiledSourceHelper::compiled(invocation, source)
        .expect("compiled source")
        .with_environment(environment)
        .with_execution_recipe(recipe)
        .expect("bound recipe")
}

fn github_environment() -> BTreeMap<String, String> {
    BTreeMap::from([(String::from("GH_TOKEN"), String::from(GH_TOKEN))])
}

#[test]
fn anonymous_scope_rejects_github_token() {
    let helper = record(
        SourceBoundOperation::NativePagesAdmission,
        NativeCredentialScope::Anonymous,
        github_environment(),
    );
    assert!(validate_record(&helper).is_err());
}

#[test]
fn github_scope_rejects_source_and_tool_producers() {
    for operation in [
        SourceBoundOperation::NpmPublicSourceProducer,
        SourceBoundOperation::MiseToolPrepare,
    ] {
        let helper = record(
            operation,
            NativeCredentialScope::GithubReadOnly,
            github_environment(),
        );
        assert!(validate_record(&helper).is_err(), "{operation:?}");
    }
}

#[test]
fn github_scope_admits_only_closed_operations() {
    for operation in [
        SourceBoundOperation::AptVerify,
        SourceBoundOperation::AptTransportIncoming,
        SourceBoundOperation::NativePagesAdmission,
        SourceBoundOperation::NativePublishAdmission,
        SourceBoundOperation::NativePublishReceiptVerifier,
        SourceBoundOperation::NativeSwiftExecution,
        SourceBoundOperation::OciDelivery,
        SourceBoundOperation::ReleaseAdmissionDefaultBranch,
        SourceBoundOperation::RustReleaseForgePreflight,
        SourceBoundOperation::RustRegistryArtifactProof,
        SourceBoundOperation::RustReleaseReconcile,
    ] {
        let helper = record(
            operation,
            NativeCredentialScope::GithubReadOnly,
            github_environment(),
        );
        assert!(validate_record(&helper).is_ok(), "{operation:?}");
    }
}

#[test]
fn github_scope_rejects_missing_or_foreign_bindings() {
    let cases = [
        BTreeMap::new(),
        BTreeMap::from([(
            String::from("GH_TOKEN"),
            String::from("${{ secrets.TOKEN }}"),
        )]),
        BTreeMap::from([
            (String::from("GH_TOKEN"), String::from(GH_TOKEN)),
            (String::from("GH_HOST"), String::from("github.example")),
        ]),
        BTreeMap::from([
            (String::from("GH_TOKEN"), String::from(GH_TOKEN)),
            (
                String::from("ACTIONS_ID_TOKEN_REQUEST_URL"),
                String::from("https://example.invalid"),
            ),
        ]),
        BTreeMap::from([
            (String::from("GH_TOKEN"), String::from(GH_TOKEN)),
            (
                String::from("AWS_SECRET_ACCESS_KEY"),
                String::from("secret"),
            ),
        ]),
        BTreeMap::from([
            (String::from("GH_TOKEN"), String::from(GH_TOKEN)),
            (String::from("GITHUB_OUTPUT"), String::from("/tmp/output")),
        ]),
    ];
    for environment in cases {
        let helper = record(
            SourceBoundOperation::NativePagesAdmission,
            NativeCredentialScope::GithubReadOnly,
            environment,
        );
        assert!(validate_record(&helper).is_err());
    }
}

#[test]
fn apt_scope_requires_exact_stage_bindings() {
    let exact = BTreeMap::from([
        (
            String::from("APT_GPG_PRIVATE_KEY"),
            String::from(APT_PRIVATE_KEY),
        ),
        (
            String::from("APT_GPG_PASSPHRASE"),
            String::from(APT_PASSPHRASE),
        ),
    ]);
    let helper = record(
        SourceBoundOperation::AptStage,
        NativeCredentialScope::AptSigning,
        exact,
    );
    assert!(validate_record(&helper).is_ok());

    let wrong_operation = record(
        SourceBoundOperation::AptVerify,
        NativeCredentialScope::AptSigning,
        BTreeMap::from([
            (
                String::from("APT_GPG_PRIVATE_KEY"),
                String::from(APT_PRIVATE_KEY),
            ),
            (
                String::from("APT_GPG_PASSPHRASE"),
                String::from(APT_PASSPHRASE),
            ),
        ]),
    );
    assert!(validate_record(&wrong_operation).is_err());
}

#[test]
fn oci_scope_requires_exact_delivery_bindings() {
    let read = record(
        SourceBoundOperation::OciDelivery,
        NativeCredentialScope::GithubReadOnly,
        github_environment(),
    );
    assert!(validate_record(&read).is_ok());

    let mut read_with_config = github_environment();
    read_with_config.insert("DOCKER_CONFIG".to_owned(), OCI_CONFIG.to_owned());
    assert!(
        validate_record(&record(
            SourceBoundOperation::OciDelivery,
            NativeCredentialScope::GithubReadOnly,
            read_with_config,
        ))
        .is_err()
    );

    let helper = record(
        SourceBoundOperation::OciDelivery,
        NativeCredentialScope::OciRegistryPublish,
        BTreeMap::from([
            (String::from("GH_TOKEN"), String::from(GH_TOKEN)),
            (String::from("DOCKER_CONFIG"), String::from(OCI_CONFIG)),
        ]),
    );
    assert!(validate_record(&helper).is_ok());

    let wrong_scope = record(
        SourceBoundOperation::OciDelivery,
        NativeCredentialScope::Anonymous,
        BTreeMap::from([
            (String::from("GH_TOKEN"), String::from(GH_TOKEN)),
            (String::from("DOCKER_CONFIG"), String::from(OCI_CONFIG)),
        ]),
    );
    assert!(validate_record(&wrong_scope).is_err());
}

fn with_oci_phase(record: &CompiledSourceHelper, phase: &str) -> CompiledSourceHelper {
    let mut wire = serde_json::to_value(record.invocation()).expect("invocation");
    wire["args"][0] = serde_json::json!(phase);
    let invocation: HelperInvocation = serde_json::from_value(wire).expect("shape");
    CompiledSourceHelper::compiled(invocation, record.source().to_owned())
        .expect("record")
        .with_environment(record.environment().clone())
        .with_execution_recipe(record.execution_recipe().expect("recipe").clone())
        .expect("binding")
}

#[test]
fn oci_scopes_reject_foreign_phases() {
    let read = record(
        SourceBoundOperation::OciDelivery,
        NativeCredentialScope::GithubReadOnly,
        github_environment(),
    );
    assert!(validate_record(&with_oci_phase(&read, "admission")).is_err());

    let publish = record(
        SourceBoundOperation::OciDelivery,
        NativeCredentialScope::OciRegistryPublish,
        BTreeMap::from([
            (String::from("GH_TOKEN"), String::from(GH_TOKEN)),
            (String::from("DOCKER_CONFIG"), String::from(OCI_CONFIG)),
        ]),
    );
    assert!(validate_record(&with_oci_phase(&publish, "verify")).is_err());
}

fn publisher_environment(scope: NativeCredentialScope) -> BTreeMap<String, String> {
    scope
        .allowed_keys()
        .iter()
        .map(|key| {
            let value = match *key {
                "GH_TOKEN" => GH_TOKEN.to_owned(),
                "CARGO_REGISTRY_TOKEN" => "${{ secrets.CARGO_REGISTRY_TOKEN }}".to_owned(),
                _ => format!("${{{{ env.{key} }}}}"),
            };
            ((*key).to_owned(), value)
        })
        .collect()
}

#[test]
fn fixed_publishers_admit_only_their_exact_credential_scope() {
    use NativeCredentialScope::{
        GithubReleasePublish, RustRegistryPublishBootstrap, RustRegistryPublishOidc,
    };
    for (operation, scope) in [
        (SourceBoundOperation::RustForgePublish, GithubReleasePublish),
        (
            SourceBoundOperation::RustReleasePrepareForge,
            GithubReleasePublish,
        ),
        (
            SourceBoundOperation::RustRegistryPublish,
            RustRegistryPublishBootstrap,
        ),
        (
            SourceBoundOperation::RustRegistryPublish,
            RustRegistryPublishOidc,
        ),
    ] {
        let environment = publisher_environment(scope);
        let exact = record(operation, scope, environment.clone());
        assert!(validate_record(&exact).is_ok());
        let scrubbed =
            super::credentials::scrubbed_environment(&exact, &environment).expect("admitted scrub");
        for (key, value) in &environment {
            assert_eq!(scrubbed.get(key), Some(value));
        }
        for key in crate::toolchain_env::STEP_CREDENTIAL_DENYLIST
            .iter()
            .chain(&crate::toolchain_env::STEP_ENDPOINT_DENYLIST)
        {
            if !scope.allowed_keys().contains(key) {
                assert_eq!(scrubbed.get(*key).map(String::as_str), Some(""));
            }
        }
        for wrong in [
            SourceBoundOperation::MiseToolPrepare,
            SourceBoundOperation::RustReleaseAnonymousPackage,
            SourceBoundOperation::RustRegistryArtifactProof,
        ] {
            assert!(validate_record(&record(wrong, scope, environment.clone())).is_err());
        }
        assert!(validate_record(&record(operation, scope, BTreeMap::new())).is_err());
        let mut foreign = environment.clone();
        foreign.insert("GH_TOKEN".to_owned(), GH_TOKEN.to_owned());
        foreign.insert(
            "CARGO_REGISTRY_TOKEN".to_owned(),
            "${{ secrets.CARGO_REGISTRY_TOKEN }}".to_owned(),
        );
        assert!(validate_record(&record(operation, scope, foreign)).is_err());
        let mut wrong_binding = environment;
        wrong_binding.insert(scope.allowed_keys()[0].to_owned(), "foreign".to_owned());
        assert!(validate_record(&record(operation, scope, wrong_binding)).is_err());
        assert!(
            validate_record(&record(
                operation,
                NativeCredentialScope::Anonymous,
                BTreeMap::new()
            ))
            .is_err()
        );
    }
}

#[test]
fn registry_artifact_proof_has_no_registry_credentials() {
    let operation = SourceBoundOperation::RustRegistryArtifactProof;
    assert!(
        validate_record(&record(
            operation,
            NativeCredentialScope::GithubReadOnly,
            github_environment()
        ))
        .is_ok()
    );
    for scope in [
        NativeCredentialScope::RustRegistryPublishBootstrap,
        NativeCredentialScope::RustRegistryPublishOidc,
        NativeCredentialScope::GithubReleasePublish,
    ] {
        assert!(validate_record(&record(operation, scope, publisher_environment(scope))).is_err());
    }
}
