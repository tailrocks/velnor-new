//! Closed Gradle compiler producer.
//!
//! The producer materializes a fresh project from one reviewed Java source
//! file.  It never enters the consumer Gradle root, so consumer plugins,
//! `mavenLocal`, and validation tasks cannot write the exported cache.

use std::collections::BTreeMap;

use velnor_actions_contract::{Step, StepId};

use crate::OrchestratorError;

const ROOT: &str = "backend";
const GRADLE_VERSION: &str = "9.5.1";
const JAVA_RUNTIME: &str = "25.0.4.1+1-jvmci-25.3-b22";
const HOME: &str = "${{ runner.temp }}/velnor/native/gradle-producer/gradle-home";
const EMPTY_HOME: &str = "${{ runner.temp }}/velnor/native/gradle-producer/empty-home";
const PROJECT: &str = "${{ runner.temp }}/velnor/native/gradle-producer/project";
const STATE: &str = "${{ runner.temp }}/velnor/native/gradle-producer/state";
const WORKING: &str = "${{ runner.temp }}/velnor/native/gradle-producer/native-cache-working";
const OUTPUT: &str = "${{ runner.temp }}/velnor/native/gradle/velnor-compile-export-v1";
const POLICY: &str = "${{ runner.temp }}/velnor/native/gradle-producer/producer-policy.init.gradle";
const PREPARE: &str = include_str!("workloads_cache_gradle_producer_prepare.py");
const EXPORT: &str = include_str!("workloads_cache_gradle_producer_export.py");
const IMPORT: &str = include_str!("workloads_cache_gradle_import.py");
const NATIVE_ENTRY: &str = include_str!("workloads_cache_gradle_native_entry.py");
const POLICY_SOURCE: &str = include_str!("workloads_cache_gradle_producer.init.gradle");

/// Provider-qualified executable roots.  Callers must obtain these from the
/// pinned Mise/helper proof; raw selectors never reach the producer argv.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct QualifiedTools {
    gradle: String,
    java_home: String,
}

impl QualifiedTools {
    /// Bind exact provider-reported paths after the helper has qualified them.
    pub(crate) fn from_helper(gradle: &str, java_home: &str) -> Result<Self, OrchestratorError> {
        validate_tool_path(gradle, "/bin/gradle")?;
        validate_tool_path(java_home, "")?;
        if !java_home.starts_with("${{ runner.temp }}/") {
            return Err(contract("gradle_producer_java_home_not_expression"));
        }
        Ok(Self {
            gradle: gradle.to_owned(),
            java_home: java_home.to_owned(),
        })
    }
}

/// Safe archive path owned by this producer.  The consumer payload must use
/// this path after the producer has exported it.
pub(crate) fn payload_paths() -> Vec<String> {
    vec![OUTPUT.to_owned()]
}

/// Closed compiler argv.  The root check prevents a caller from rebinding the
/// producer to a repository task or a different source tree.
pub(crate) fn producer_argv(
    root: &str,
    tools: &QualifiedTools,
) -> Result<Vec<String>, OrchestratorError> {
    ensure_root(root)?;
    Ok(vec![
        tools.gradle.clone(),
        "--no-daemon".to_owned(),
        "--offline".to_owned(),
        "--build-cache".to_owned(),
        "--no-configuration-cache".to_owned(),
        "--console=plain".to_owned(),
        "--gradle-user-home".to_owned(),
        HOME.to_owned(),
        "--project-cache-dir".to_owned(),
        STATE.to_owned(),
        "--init-script".to_owned(),
        POLICY.to_owned(),
        "--project-dir".to_owned(),
        PROJECT.to_owned(),
        ":processor-target-validation:compileJava".to_owned(),
    ])
}

/// Fixed prepare argv: no repository script is interpreted by this step.
pub(crate) fn producer_prepare_argv(root: &str) -> Result<Vec<String>, OrchestratorError> {
    ensure_root(root)?;
    let artifacts =
        serde_json::to_string(super::gradle_artifacts::artifact_descriptors()).map_err(contract)?;
    Ok(python_argv(PREPARE, &[POLICY_SOURCE, &artifacts]))
}

/// Fixed export argv: only native cache key names enter the archive directory.
pub(crate) fn producer_export_argv(root: &str) -> Result<Vec<String>, OrchestratorError> {
    ensure_root(root)?;
    let source = format!("{NATIVE_ENTRY}\n{EXPORT}");
    Ok(python_argv(&source, &[]))
}

/// Fixed consumer import argv; it accepts no repository or task selector.
pub(crate) fn consumer_import_argv() -> Vec<String> {
    let source = format!("{NATIVE_ENTRY}\n{IMPORT}");
    python_argv(&source, &[])
}

/// Copy the sealed producer archive into the consumer's private local cache.
pub(crate) fn consumer_import_step() -> Result<Step, OrchestratorError> {
    shell_step(
        "Import sealed Gradle native cache",
        consumer_import_argv(),
        consumer_import_env(),
        "velnor-gradle-compile-import",
    )
}

/// Prepare, compile, and export the pure producer in order.
pub(crate) fn producer_steps(
    root: &str,
    tools: &QualifiedTools,
) -> Result<Vec<Step>, OrchestratorError> {
    let prepare = shell_step(
        "Prepare closed Gradle compiler",
        producer_prepare_argv(root)?,
        producer_env(tools),
        "velnor-gradle-producer-prepare",
    )?;
    let compile = shell_step(
        "Compile reviewed Gradle native target",
        producer_argv(root, tools)?,
        producer_env(tools),
        "velnor-gradle-producer-compile",
    )?;
    let export = shell_step(
        "Export safe Gradle native cache",
        producer_export_argv(root)?,
        producer_env(tools),
        "velnor-gradle-producer-export",
    )?;
    Ok(vec![prepare, compile, export])
}

fn python_argv(source: &str, arguments: &[&str]) -> Vec<String> {
    let encoded = source
        .bytes()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let mut argv = vec![
        "/usr/bin/python3".to_owned(),
        "-I".to_owned(),
        "-c".to_owned(),
        format!(
            "import binascii;exec(compile(binascii.unhexlify('{encoded}'),'velnor-gradle-producer.py','exec'))"
        ),
    ];
    for argument in arguments {
        let encoded = argument
            .bytes()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        argv.push(encoded);
    }
    argv
}

fn producer_env(tools: &QualifiedTools) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("GRADLE_USER_HOME".to_owned(), HOME.to_owned()),
        ("HOME".to_owned(), EMPTY_HOME.to_owned()),
        ("JAVA_HOME".to_owned(), tools.java_home.clone()),
        (
            "VELNOR_GRADLE_PRODUCER_JAVA_RUNTIME".to_owned(),
            JAVA_RUNTIME.to_owned(),
        ),
        (
            "VELNOR_GRADLE_PRODUCER_GRADLE_VERSION".to_owned(),
            GRADLE_VERSION.to_owned(),
        ),
        ("VELNOR_GRADLE_PRODUCER_ROOT".to_owned(), ROOT.to_owned()),
        (
            "VELNOR_GRADLE_PRODUCER_PROJECT".to_owned(),
            PROJECT.to_owned(),
        ),
        (
            "VELNOR_GRADLE_PRODUCER_WORKING".to_owned(),
            WORKING.to_owned(),
        ),
        (
            "VELNOR_GRADLE_PRODUCER_OUTPUT".to_owned(),
            OUTPUT.to_owned(),
        ),
        ("VELNOR_GRADLE_PRODUCER_STATE".to_owned(), STATE.to_owned()),
        (
            "VELNOR_GRADLE_PRODUCER_POLICY".to_owned(),
            POLICY.to_owned(),
        ),
        ("GRADLE_OPTS".to_owned(), String::new()),
        ("JAVA_OPTS".to_owned(), String::new()),
        ("MAVEN_OPTS".to_owned(), String::new()),
        ("CLASSPATH".to_owned(), String::new()),
        ("JAVA_TOOL_OPTIONS".to_owned(), String::new()),
        ("JDK_JAVA_OPTIONS".to_owned(), String::new()),
        ("_JAVA_OPTIONS".to_owned(), String::new()),
        ("HTTP_PROXY".to_owned(), String::new()),
        ("HTTPS_PROXY".to_owned(), String::new()),
        ("ALL_PROXY".to_owned(), String::new()),
        ("NO_PROXY".to_owned(), String::new()),
        ("BASH_ENV".to_owned(), String::new()),
        ("ENV".to_owned(), String::new()),
        ("PYTHONPATH".to_owned(), String::new()),
        ("PYTHONHOME".to_owned(), String::new()),
        ("LD_PRELOAD".to_owned(), String::new()),
        ("LD_LIBRARY_PATH".to_owned(), String::new()),
        ("LD_AUDIT".to_owned(), String::new()),
        ("DYLD_INSERT_LIBRARIES".to_owned(), String::new()),
        ("DYLD_LIBRARY_PATH".to_owned(), String::new()),
        ("DYLD_FRAMEWORK_PATH".to_owned(), String::new()),
        ("DYLD_FALLBACK_FRAMEWORK_PATH".to_owned(), String::new()),
        ("DYLD_FALLBACK_LIBRARY_PATH".to_owned(), String::new()),
        ("PATH".to_owned(), "/usr/bin:/bin".to_owned()),
    ])
}

fn consumer_import_env() -> BTreeMap<String, String> {
    BTreeMap::from([
        ("RUNNER_TEMP".to_owned(), "${{ runner.temp }}".to_owned()),
        (
            "GRADLE_USER_HOME".to_owned(),
            "${{ runner.temp }}/velnor/native/gradle".to_owned(),
        ),
        (
            "VELNOR_GRADLE_PRODUCER_OUTPUT".to_owned(),
            OUTPUT.to_owned(),
        ),
        (
            "VELNOR_GRADLE_CONSUMER_CACHE".to_owned(),
            "${{ runner.temp }}/velnor/native/gradle/velnor-compile-cache-v1".to_owned(),
        ),
        ("PATH".to_owned(), "/usr/bin:/bin".to_owned()),
        ("BASH_ENV".to_owned(), String::new()),
        ("ENV".to_owned(), String::new()),
        ("PYTHONPATH".to_owned(), String::new()),
        ("PYTHONHOME".to_owned(), String::new()),
        ("LD_PRELOAD".to_owned(), String::new()),
        ("LD_LIBRARY_PATH".to_owned(), String::new()),
        ("LD_AUDIT".to_owned(), String::new()),
        ("DYLD_INSERT_LIBRARIES".to_owned(), String::new()),
        ("DYLD_LIBRARY_PATH".to_owned(), String::new()),
        ("DYLD_FRAMEWORK_PATH".to_owned(), String::new()),
        ("DYLD_FALLBACK_FRAMEWORK_PATH".to_owned(), String::new()),
        ("DYLD_FALLBACK_LIBRARY_PATH".to_owned(), String::new()),
    ])
}

fn shell_step(
    name: &str,
    argv: Vec<String>,
    env: BTreeMap<String, String>,
    id: &str,
) -> Result<Step, OrchestratorError> {
    let mut step = velnor_actions_workflow_renderer::shell_step(name, argv, env)?;
    step.id = Some(StepId::new(id).map_err(contract)?);
    Ok(step)
}

fn ensure_root(root: &str) -> Result<(), OrchestratorError> {
    if root == ROOT {
        Ok(())
    } else {
        Err(contract("gradle_producer_root_not_reviewed"))
    }
}

fn validate_tool_path(path: &str, suffix: &str) -> Result<(), OrchestratorError> {
    const PREFIX: &str = "${{ runner.temp }}/";
    let tail = path.strip_prefix(PREFIX).unwrap_or("");
    if tail.is_empty()
        || !path.starts_with(PREFIX)
        || !path.ends_with(suffix)
        || path.contains("..")
        || path.chars().any(char::is_control)
        || tail
            .chars()
            .any(|character| matches!(character, '$' | '`' | ';' | '|' | '&' | '<' | '>'))
    {
        return Err(contract("gradle_producer_tool_path_not_qualified"));
    }
    Ok(())
}

fn contract(error: impl std::fmt::Display) -> OrchestratorError {
    OrchestratorError::Contract {
        problem: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        GRADLE_VERSION, JAVA_RUNTIME, QualifiedTools, producer_argv, producer_export_argv,
        producer_prepare_argv,
    };

    #[test]
    fn compiler_argv_is_closed_to_one_task_and_owned_roots() {
        let tools = QualifiedTools::from_helper(
            "${{ runner.temp }}/tool-owner/gradle-output/bin/gradle",
            "${{ runner.temp }}/tool-owner/java-output",
        )
        .expect("qualified tools");
        let argv = producer_argv("backend", &tools).expect("reviewed root");
        assert_eq!(
            argv[0],
            "${{ runner.temp }}/tool-owner/gradle-output/bin/gradle"
        );
        assert!(argv.contains(&":processor-target-validation:compileJava".to_owned()));
        assert!(argv.contains(&"--offline".to_owned()));
        assert!(
            !argv
                .iter()
                .any(|arg| arg.contains("gradlew") || arg.contains("mavenLocal"))
        );
        assert!(producer_argv("other", &tools).is_err());
    }

    #[test]
    fn helper_argv_embeds_source_without_a_runtime_script_path() {
        let prepare = producer_prepare_argv("backend").expect("prepare");
        let export = producer_export_argv("backend").expect("export");
        let import = consumer_import_argv();
        assert_eq!(prepare.len(), 6);
        assert_eq!(export.len(), 4);
        assert_eq!(import.len(), 4);
        for argv in [prepare, export, import] {
            assert_eq!(argv[0], "/usr/bin/python3");
            assert_eq!(argv[1], "-I");
            assert!(argv[3].starts_with("import binascii;exec(compile("));
            assert!(!argv[3].contains('\n'));
        }
    }

    #[test]
    fn pins_are_explicit() {
        assert_eq!(GRADLE_VERSION, "9.5.1");
        assert_eq!(JAVA_RUNTIME, "25.0.4.1+1-jvmci-25.3-b22");
    }

    #[test]
    fn provider_paths_are_required_and_pinned() {
        let java = "${{ runner.temp }}/mise/java/tool-owner-output";
        assert!(QualifiedTools::from_helper("/usr/bin/gradle", java).is_err());
        assert!(
            QualifiedTools::from_helper(
                "${{ runner.temp }}/mise/gradle/tool-owner-output/bin/gradle",
                "$RUNNER_TEMP/mise/java/tool-owner-output",
            )
            .is_err()
        );
    }
}
