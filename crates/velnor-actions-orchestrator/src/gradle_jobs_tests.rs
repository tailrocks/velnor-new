//! Helper tests for Gradle job construction.

use super::*;

fn files(list: &[&str]) -> Vec<String> {
    list.iter().map(|file| (*file).to_owned()).collect()
}

#[test]
fn roots_need_settings_gradlew_and_wrapper_properties() {
    let full = [
        "backend/settings.gradle.kts",
        "backend/gradlew",
        "backend/gradle/wrapper/gradle-wrapper.properties",
        "other/settings.gradle",
        "other/gradle/wrapper/gradle-wrapper.properties",
        "settings.gradle.kts",
    ];
    assert_eq!(root_dirs(&files(&full)), ["backend"]);
    let top = [
        "settings.gradle",
        "gradlew",
        "gradle/wrapper/gradle-wrapper.properties",
    ];
    assert_eq!(root_dirs(&files(&top)), [""]);
}

#[test]
fn job_ids_are_slugged_and_scoped_by_root() {
    assert_eq!(job_id("backend", "eth-domain"), "gradle-backend-eth-domain");
    assert_eq!(job_id("", "a:b"), "gradle-a-b");
}

#[test]
fn the_run_script_is_one_continue_invocation_with_strict_verification() {
    let script = run_script("backend", ":core", 4);
    assert_eq!(script.matches("./gradlew").count(), 1);
    for expected in [
        "--continue",
        "--dependency-verification strict",
        "--max-workers=4",
        ":core:assemble :core:check",
        "cd backend",
        "VELNOR_MAVEN_ROOT",
    ] {
        assert!(script.contains(expected), "{expected}: {script}");
    }
}
