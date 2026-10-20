//! The fixed Gradle init text and user-home properties.

use velnor_actions_gradle::script::{INIT_SCRIPT_NAME, init_script, user_home_properties};

#[test]
fn init_script_adds_every_bundle_repository_for_the_chainargos_group_only() {
    let init = init_script();
    assert!(init.contains("VELNOR_MAVEN_ROOT"));
    assert!(init.contains("pluginManagement.repositories"));
    assert!(init.contains("dependencyResolutionManagement.repositories"));
    assert!(init.contains("com\\\\.chainargos"));
    assert_eq!(INIT_SCRIPT_NAME, "velnor.init.gradle.kts");
}

#[test]
fn user_home_properties_forbid_toolchain_downloads() {
    let props = user_home_properties();
    assert!(props.contains("auto-download=false"));
    assert!(props.contains("auto-detect=false"));
    assert!(props.contains("fromEnv=JAVA_HOME"));
}
