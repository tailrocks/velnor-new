//! Closed helper identity and owner selection regressions.

use super::owner_for_task;
use velnor_actions_contract::{
    CompiledSourceHelper, HelperInvocation, HelperObligationDescriptor, SourceBoundHelper,
    SourceBoundOperation,
};

fn identity(args: Vec<String>, selectors: Vec<String>, body: &str) -> HelperObligationDescriptor {
    let operation = SourceBoundOperation::PackageUpdateFixture;
    let source = velnor_actions_contract::generated_source("0.1.0", body).expect("fixture source");
    let hash = velnor_actions_contract::compiled_source_sha256(source.as_bytes());
    let helper =
        SourceBoundHelper::compiled(operation, operation.path(), &hash).expect("fixture helper");
    let invocation =
        HelperInvocation::compiled(helper, args, selectors).expect("fixture invocation");
    let record = CompiledSourceHelper::compiled(invocation, source).expect("fixture record");
    let id = velnor_actions_contract::matrix_id_for_task_group(
        "workload",
        "stack/workload/pkg/package-update-fixtures/package_update_fixture",
    )
    .expect("fixture matrix");
    let key = velnor_actions_contract::matrix_key_for_id(&id).expect("fixture key");
    HelperObligationDescriptor::from_compiled(&record, &key).expect("fixture descriptor")
}

#[test]
fn semantic_task_digest_binds_compiled_transport_dimensions() {
    let task = "stack/workload/pkg/package-update-fixtures/package_update_fixture";
    let original = identity(vec!["profile".to_owned()], vec![], "exit 0\n");
    let digest = |helper: Option<&HelperObligationDescriptor>| {
        crate::internal::plan_obligation::task_digest(
            task,
            &["native-helper".to_owned()],
            "toolchain",
            helper,
            None,
        )
        .expect("fixture digest")
    };
    let expected = digest(Some(&original));
    assert_ne!(expected, digest(None));
    let mut environment = original.clone();
    environment
        .environment
        .insert("HOME".to_owned(), "isolated".to_owned());
    assert_ne!(expected, digest(Some(&environment)));
    for changed in [
        identity(vec!["other-profile".to_owned()], vec![], "exit 0\n"),
        identity(
            vec!["profile".to_owned()],
            vec!["ruby@3.4.1".to_owned()],
            "exit 0\n",
        ),
        identity(vec!["profile".to_owned()], vec![], "exit 1\n"),
    ] {
        assert_ne!(expected, digest(Some(&changed)));
    }
}

#[test]
fn required_phases_are_selected_without_adapter_metadata() {
    for (stack, task) in [
        (
            "workload",
            "stack/workload/tap/homebrew-tap-local/homebrew_audit",
        ),
        (
            "workload",
            "stack/workload/pkg/package-update-fixtures/package_update_fixture",
        ),
        (
            "workload",
            "stack/workload/app/native-ffi/native_xcode_project_ci",
        ),
        (
            "workload",
            "stack/workload/app/native-swift-test/native_swift_package_ci",
        ),
        ("tofu", "stack/tofu/dir-737461636b732f767063/init/default"),
        (
            "tofu",
            "stack/tofu/dir-737461636b732f767063/init/default/shard-1-of-2",
        ),
    ] {
        assert!(owner_for_task(stack, task).is_some(), "{task}");
    }
}

#[test]
fn ordinary_phases_and_cross_owner_names_do_not_gain_helper_authority() {
    for (stack, task) in [
        (
            "workload",
            "stack/workload/tap/homebrew-audit/homebrew_audit",
        ),
        ("workload", "stack/workload/pkg/test/node_ci"),
        (
            "workload",
            "stack/workload/app/native-swift-test/native_xcode_project_ci",
        ),
        (
            "workload",
            "stack/tofu/dir-737461636b732f767063/init/default",
        ),
        (
            "tofu",
            "stack/tofu/dir-737461636b732f767063/validate/default",
        ),
        ("tofu", "stack/tofu/dir-737461636b732f767063/init/unknown"),
    ] {
        assert!(owner_for_task(stack, task).is_none(), "{task}");
    }
}
