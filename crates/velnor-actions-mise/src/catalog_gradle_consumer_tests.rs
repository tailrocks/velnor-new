use super::*;
use velnor_actions_contract::{HelperInvocation, SourceBoundHelper, SourceBoundOperation};

#[test]
fn exact_roles_are_sealed_and_linux_installation_is_absent() -> Result<(), MiseError> {
    let context = GradleConsumerContext::require(DistributionHost::MacosArm64)?;
    assert_eq!(context.domain(), ToolCacheDomain::GradleBootstrap);
    for (role, version) in [
        (GradleConsumerRole::Java, JAVA_SELECTION_VERSION),
        (GradleConsumerRole::Engine, "9.5.1"),
        (GradleConsumerRole::Bootstrap, "9.4.1"),
    ] {
        let launch = context.launch(role);
        assert_eq!(launch.distribution().selection_version(), version);
        assert!(launch.executable().starts_with(context.owned_root()));
        assert!(launch.install_root().starts_with(context.owned_root()));
    }
    assert!(context.java_home().ends_with("/Contents/Home"));
    let recipe = context.execution_recipe()?;
    context.validate_recipe(&recipe)?;
    assert_eq!(recipe.installed_selectors(), context.selectors());
    assert_eq!(
        recipe.environment().get("VELNOR_GRADLE_ENGINE"),
        Some(&context.launch(GradleConsumerRole::Engine).executable())
    );
    assert_eq!(
        recipe.environment().get("VELNOR_GRADLE_BOOTSTRAP"),
        Some(&context.launch(GradleConsumerRole::Bootstrap).executable())
    );
    assert!(!recipe.environment().contains_key("GH_TOKEN"));
    assert!(
        context
            .selectors()
            .iter()
            .all(|selector| !selector.ends_with("@9.8.0"))
    );
    for host in [DistributionHost::LinuxAmd64, DistributionHost::LinuxArm64] {
        assert!(GradleConsumerContext::require(host).is_err());
        assert!(helper_for_host(host, "0.1.0").is_err());
    }
    Ok(())
}

#[test]
fn exact_factory_reconstruction_rejects_substitution() -> Result<(), MiseError> {
    let helper = helper_for_host(DistributionHost::MacosArm64, "0.1.0")?;
    assert_eq!(
        record_for_invocation(helper.invocation(), helper.environment(), "0.1.0")?,
        helper
    );
    assert_eq!(
        super::super::tool_prepare::record_for_invocation(
            helper.invocation(),
            helper.environment(),
            "0.1.0"
        )?,
        helper
    );
    let mut environment = helper.environment().clone();
    environment.insert("JAVA_HOME".to_owned(), "/tmp/foreign-java".to_owned());
    assert!(record_for_invocation(helper.invocation(), &environment, "0.1.0").is_err());
    let mut args = helper.invocation().args().to_vec();
    args[3] = args[3].replace("9.4.1", "9.8.0");
    let changed = HelperInvocation::compiled(
        helper.invocation().descriptor().clone(),
        args,
        helper.invocation().installed_selectors().to_vec(),
    )
    .map_err(|error| super::super::tool_prepare::contract(&error))?;
    assert!(record_for_invocation(&changed, helper.environment(), "0.1.0").is_err());
    let descriptor = SourceBoundHelper::compiled(
        SourceBoundOperation::MiseToolPrepare,
        SourceBoundOperation::MiseToolPrepare.path(),
        &"0".repeat(64),
    )
    .map_err(|error| super::super::tool_prepare::contract(&error))?;
    let changed = HelperInvocation::compiled(
        descriptor,
        helper.invocation().args().to_vec(),
        helper.invocation().installed_selectors().to_vec(),
    )
    .map_err(|error| super::super::tool_prepare::contract(&error))?;
    assert!(record_for_invocation(&changed, helper.environment(), "0.1.0").is_err());
    Ok(())
}

#[test]
fn recipe_scope_and_launch_cannot_change() -> Result<(), MiseError> {
    use velnor_actions_contract::workflow::native_tools::{
        CompiledNativeExecRecipe, NativeCredentialScope,
    };
    let context = GradleConsumerContext::require(DistributionHost::MacosArm64)?;
    let recipe = context.execution_recipe()?;
    let changed = CompiledNativeExecRecipe::compiled_for_scope(
        recipe.prefix().to_vec(),
        recipe.environment().clone(),
        recipe.installed_selectors().to_vec(),
        NativeCredentialScope::GithubReadOnly,
    )
    .map_err(|error| super::super::tool_prepare::contract(&error))?;
    assert!(context.validate_recipe(&changed).is_err());
    let mut environment = recipe.environment().clone();
    environment.insert(
        "VELNOR_GRADLE_ENGINE".to_owned(),
        "/tmp/managed-gradle".to_owned(),
    );
    let changed = CompiledNativeExecRecipe::compiled(
        recipe.prefix().to_vec(),
        environment,
        recipe.installed_selectors().to_vec(),
    )
    .map_err(|error| super::super::tool_prepare::contract(&error))?;
    assert!(context.validate_recipe(&changed).is_err());
    Ok(())
}
