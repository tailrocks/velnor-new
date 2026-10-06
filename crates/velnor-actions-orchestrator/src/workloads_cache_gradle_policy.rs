//! Fixed Gradle policy; no repository-defined task selector or cache endpoint.

const POLICY: &str = include_str!("workloads_cache_gradle_policy.init.gradle");
const PREPARE: &str = include_str!("workloads_cache_gradle_policy_prepare.py");

pub(super) fn prepare_step(
    root: &str,
) -> Result<velnor_actions_contract::Step, crate::OrchestratorError> {
    let encoded = PREPARE
        .bytes()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let argv = vec![
        "/usr/bin/python3".to_owned(),
        "-I".to_owned(),
        "-c".to_owned(),
        format!(
            "import binascii;exec(compile(binascii.unhexlify('{encoded}'),'velnor-gradle-policy.py','exec'))"
        ),
    ];
    let mut env = super::super::gradle::task_env();
    for name in [
        "BASH_ENV",
        "ENV",
        "PYTHONPATH",
        "PYTHONHOME",
        "LD_PRELOAD",
        "LD_LIBRARY_PATH",
        "LD_AUDIT",
        "DYLD_INSERT_LIBRARIES",
        "DYLD_LIBRARY_PATH",
        "DYLD_FRAMEWORK_PATH",
        "DYLD_FALLBACK_FRAMEWORK_PATH",
        "DYLD_FALLBACK_LIBRARY_PATH",
    ] {
        env.insert(name.to_owned(), String::new());
    }
    env.insert("PATH".to_owned(), "/usr/bin:/bin".to_owned());
    env.insert("RUNNER_TEMP".to_owned(), "${{ runner.temp }}".to_owned());
    env.insert("VELNOR_GRADLE_OUTPUT_POLICY".to_owned(), POLICY.to_owned());
    env.insert("VELNOR_GRADLE_OUTPUT_ROOT".to_owned(), root.to_owned());
    Ok(velnor_actions_workflow_renderer::shell_step(
        "Prepare qualified native Gradle compiler cache",
        argv,
        env,
    )?)
}

#[cfg(test)]
mod tests {
    use super::{POLICY, PREPARE};

    #[test]
    fn prepare_rejects_duplicate_and_ancestor_links() {
        let fixture = include_str!("workloads_cache_gradle_policy_prepare_fixture.py");
        assert!(
            std::process::Command::new("/usr/bin/python3")
                .args(["-I", "-c", fixture, PREPARE])
                .status()
                .expect("closed policy fixture")
                .success()
        );
    }

    #[test]
    fn policy_is_closed_to_one_compiler_and_fresh_tests() {
        assert!(POLICY.contains(":processor-target-validation:compileJava"));
        assert!(POLICY.contains("task.outputs.doNotCacheIf"));
        assert!(POLICY.contains("task.outputs.upToDateWhen { false }"));
        assert!(POLICY.contains("settings.buildCache.remote = null"));
        assert!(POLICY.contains("settings.buildCache.local.push = false"));
        assert!(POLICY.contains("options.compilerArgs != ['-parameters']"));
        assert!(POLICY.contains("options.encoding != null"));
        assert!(POLICY.contains("classpath.size() != permitted.size()"));
        assert!(POLICY.contains("options.compilerArgumentProviders.empty"));
        assert!(POLICY.contains("options.annotationProcessorPath.files.empty"));
        assert!(POLICY.contains("sha256(file)"));
    }
}
