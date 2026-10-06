//! Isolated Gradle environment; source and compiler transports qualify separately.

use std::collections::BTreeMap;

const GRADLE_HOME: &str = "${{ runner.temp }}/velnor/native/gradle";

/// Fresh, owned user home prevents ambient properties and initialization code.
pub(crate) fn task_env() -> BTreeMap<String, String> {
    BTreeMap::from([
        ("GRADLE_USER_HOME".to_owned(), GRADLE_HOME.to_owned()),
        ("GRADLE_OPTS".to_owned(), String::new()),
        ("JAVA_OPTS".to_owned(), String::new()),
        ("JAVA_TOOL_OPTIONS".to_owned(), String::new()),
        ("JDK_JAVA_OPTIONS".to_owned(), String::new()),
        ("_JAVA_OPTIONS".to_owned(), String::new()),
    ])
}

#[cfg(test)]
mod tests {
    use super::task_env;

    #[test]
    fn environment_owns_gradle_state_and_blocks_ambient_jvm_options() {
        assert_eq!(
            task_env().get("GRADLE_USER_HOME").map(String::as_str),
            Some("${{ runner.temp }}/velnor/native/gradle")
        );
        for key in [
            "JAVA_OPTS",
            "GRADLE_OPTS",
            "JAVA_TOOL_OPTIONS",
            "JDK_JAVA_OPTIONS",
            "_JAVA_OPTIONS",
        ] {
            assert_eq!(task_env().get(key).map(String::as_str), Some(""));
        }
        assert!(!task_env().contains_key("JAVA_HOME"));
    }
}
