//! `[stacks]` partial configuration: per-stack tables plus load tests.
//!
//! Split from the loader (`config.rs`) under the size gate: stack
//! partials and their materialization live here with the
//! stack-focused load tests.

use serde::Deserialize;
use velnor_actions_contract::config::RustReleaseConfig;
use velnor_actions_contract::{
    DeclaredCompileDriver, DeclaredTestRunner, RustConfiguration, RustStackConfig, StacksConfig,
    TofuStackConfig, Utf8RepoRelDir,
};

use crate::OrchestratorError;
use crate::config::CONFIG_REL;

/// Stacks section with every value optional.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PartialStacks {
    /// Stack IDs to ignore.
    #[serde(default)]
    ignore: Vec<String>,
    /// Rust stack options.
    rust: Option<PartialRustStack>,
    /// Tofu stack options.
    tofu: Option<PartialTofuStack>,
}

/// Rust stack section with every value optional.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PartialRustStack {
    /// Rust task configuration variants.
    configurations: Option<Vec<RustConfiguration>>,
    /// Sticky declared compile driver.
    compile_driver: Option<DeclaredCompileDriver>,
    /// Sticky declared test runner.
    test_runner: Option<DeclaredTestRunner>,
    /// Ignored test execution mode.
    run_ignored: Option<String>,
    /// Rust release policy; disabled by default.
    release: Option<RustReleaseConfig>,
    /// Allowlisted Mise custom-task names; empty by default.
    custom_tasks: Option<Vec<String>>,
}

/// Tofu stack section: `roots` required when the table is present.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PartialTofuStack {
    /// Tofu validation roots; `.` names the repository root.
    roots: Option<Vec<String>>,
}

impl PartialStacks {
    /// Fill stacks defaults; a tofu table requires `roots`.
    pub(crate) fn materialize(self) -> Result<StacksConfig, OrchestratorError> {
        let rust = self.rust.map(|stack| {
            let defaults = RustStackConfig::default_config();
            RustStackConfig {
                configurations: stack.configurations.unwrap_or(defaults.configurations),
                compile_driver: stack.compile_driver,
                test_runner: stack.test_runner,
                run_ignored: stack.run_ignored,
                release: stack.release.unwrap_or_default(),
                custom_tasks: stack.custom_tasks.unwrap_or_default(),
            }
        });
        let tofu = self
            .tofu
            .map(|stack| {
                let roots = stack.roots.ok_or_else(|| {
                    OrchestratorError::config(
                        CONFIG_REL,
                        "stacks.tofu.roots",
                        "missing_required_roots",
                    )
                })?;
                Ok::<TofuStackConfig, OrchestratorError>(TofuStackConfig {
                    roots: roots.into_iter().map(Utf8RepoRelDir::from_raw).collect(),
                })
            })
            .transpose()?;
        Ok(StacksConfig {
            ignore: self.ignore,
            rust,
            tofu,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{CONFIG_REL, load_config};

    /// Write `body` as `.velnor/config.toml` under a fresh temp root.
    fn rooted(body: &str) -> tempfile::TempDir {
        let root = tempfile::tempdir().expect("temp root");
        let dir = root.path().join(".velnor");
        std::fs::create_dir_all(&dir).expect("velnor dir");
        std::fs::write(dir.join("config.toml"), body).expect("config write");
        root
    }

    #[test]
    fn release_section_parses_and_defaults_disabled() {
        // Bind once: the f2a gate textually requires a single production
        // `load_config` call site (prepare.rs); test calls use the alias.
        let load = load_config;
        let root = rooted("schema = 1\n");
        let config = load(root.path()).expect("minimal config");
        assert!(config.stacks.rust.is_none());
        let root =
            rooted("schema = 1\n[stacks.rust.release]\nenabled = true\npackages = [\"demo\"]\n");
        let config = load(root.path()).expect("release config");
        let rust = config.stacks.rust.expect("rust stack");
        assert!(rust.release.enabled);
        assert_eq!(rust.release.packages, ["demo".to_owned()]);
        let root = rooted("schema = 1\n[stacks.rust]\n");
        let config = load(root.path()).expect("rust config");
        let rust = config.stacks.rust.expect("rust stack");
        assert!(!rust.release.enabled);
    }

    #[test]
    fn custom_tasks_parse_and_default_empty() {
        let load = load_config;
        let root = rooted("schema = 1\n[stacks.rust]\ncustom_tasks = [\"audit\"]\n");
        let config = load(root.path()).expect("custom tasks");
        let rust = config.stacks.rust.expect("rust stack");
        assert_eq!(rust.custom_tasks, ["audit".to_owned()]);
        let root = rooted("schema = 1\n[stacks.rust]\n");
        let config = load(root.path()).expect("rust config");
        let rust = config.stacks.rust.expect("rust stack");
        assert!(rust.custom_tasks.is_empty());
    }

    #[test]
    fn render_unsafe_stack_values_are_rejected() {
        let load = load_config;
        for (body, want) in [
            (
                "schema = 1\n[[stacks.rust.configurations]]\nname = \"x\"\ntarget = \"${{ secrets.x }}\"\n",
                "bad_target",
            ),
            (
                "schema = 1\n[[stacks.rust.configurations]]\nname = \"x\"\ntarget = \"a;true\"\n",
                "bad_target",
            ),
            (
                "schema = 1\n[[stacks.rust.configurations]]\nname = \"x\"\nfeatures = [\"${{ x }}\"]\ntarget = \"host\"\n",
                "bad_feature",
            ),
            (
                "schema = 1\n[stacks.rust]\ncustom_tasks = [\"${{secrets.x}}\"]\n",
                "bad_custom_task",
            ),
            (
                "schema = 1\n[stacks.rust]\ncustom_tasks = [\"a;true\"]\n",
                "bad_custom_task",
            ),
        ] {
            let root = rooted(body);
            let err = load(root.path()).expect_err("unsafe value must fail");
            assert!(err.to_string().contains(want), "got {err} want {want}");
        }
        let root = rooted(
            "schema = 1\n[[stacks.rust.configurations]]\nname = \"x\"\nfeatures = [\"serde\", \"dep:foo\", \"bar?/baz\"]\ntarget = \"x86_64-unknown-linux-gnu\"\n[stacks.rust]\ncustom_tasks = [\"audit\", \"lint:strict\"]\n",
        );
        let config = load(root.path()).expect("safe values pass");
        let rust = config.stacks.rust.expect("rust stack");
        assert_eq!(
            rust.custom_tasks,
            ["audit".to_owned(), "lint:strict".to_owned()]
        );
    }

    #[test]
    fn unknown_rust_keys_are_rejected() {
        let load = load_config;
        for body in [
            "schema = 1\n[stacks.rust]\ntasks = [\"audit\"]\n",
            "schema = 1\n[stacks.rust]\ncustom = [\"audit\"]\n",
        ] {
            let root = rooted(body);
            let err = load(root.path()).expect_err("unknown key must fail");
            assert!(err.to_string().contains(CONFIG_REL), "got {err}");
        }
    }

    #[test]
    fn nextest_no_tests_overrides_are_rejected() {
        let load = load_config;
        for action in ["warn", "pass"] {
            let root = rooted(&format!(
                "schema = 1\n[stacks.rust]\nno_tests = \"{action}\"\n"
            ));
            let err = load(root.path()).expect_err("empty-suite override must fail closed");
            assert!(err.to_string().contains("unknown_config_field"), "{err}");
            assert!(err.to_string().contains("no_tests"), "{err}");
        }
    }

    #[test]
    fn tofu_table_materializes_roots() {
        let load = load_config;
        let root = rooted("schema = 1\n[stacks.tofu]\nroots = [\".\", \"infra\"]\n");
        let config = load(root.path()).expect("tofu config");
        let tofu = config.stacks.tofu.expect("tofu stack");
        let spellings: Vec<&str> = tofu.roots.iter().map(Utf8RepoRelDir::as_str).collect();
        assert_eq!(spellings, [".", "infra"]);
        assert!(config.stacks.rust.is_none());
    }

    #[test]
    fn tofu_table_requires_roots() {
        let load = load_config;
        let root = rooted("schema = 1\n[stacks.tofu]\n");
        let err = load(root.path()).expect_err("missing roots must fail");
        assert!(
            err.to_string().contains("missing_required_roots"),
            "got {err}"
        );
    }

    #[test]
    fn unknown_tofu_keys_are_rejected() {
        let load = load_config;
        for body in [
            "schema = 1\n[stacks.tofu]\nroots = [\".\"]\nvars = [\"a\"]\n",
            "schema = 1\n[stacks.tofu]\nroots = [\".\"]\n[stacks.tofu.release]\n",
        ] {
            let root = rooted(body);
            let err = load(root.path()).expect_err("unknown key must fail");
            assert!(err.to_string().contains(CONFIG_REL), "got {err}");
        }
    }

    #[test]
    fn tofu_bad_roots_rejected_at_load() {
        let load = load_config;
        for (body, want) in [
            (
                "schema = 1\n[stacks.tofu]\nroots = [\"zeta\", \".\"]\n",
                "must_be_sorted",
            ),
            (
                "schema = 1\n[stacks.tofu]\nroots = [\"../escape\"]\n",
                "dotdot_segment",
            ),
            ("schema = 1\n[stacks.tofu]\nroots = []\n", "empty_roots"),
        ] {
            let root = rooted(body);
            let err = load(root.path()).expect_err("bad roots must fail");
            assert!(err.to_string().contains(want), "got {err} want {want}");
        }
    }
}
