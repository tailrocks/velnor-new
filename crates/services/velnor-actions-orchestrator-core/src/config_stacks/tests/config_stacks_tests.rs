use super::*;

#[test]
fn release_section_parses_and_defaults_disabled() {
    // Bind once: the f2a gate textually requires a single production
    // `load_config` call site (prepare.rs); test calls use the alias.
    let load = load_config;
    let root = rooted("schema = 1\n");
    let config = load(root.path()).expect("minimal config");
    assert!(config.stacks.rust.is_none());
    let root = rooted("schema = 1\n[stacks.rust.release]\nenabled = true\npackages = [\"demo\"]\n");
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
fn workflow_verification_tasks_parse_and_default_empty() {
    let load = load_config;
    let root = rooted(
        "schema = 1\n[[workflow.tasks]]\nid = \"native-swift-format\"\nkind = \"verification\"\nmise_task = \"desktop-format-check\"\nrunner = \"macos-arm64\"\ntimeout_minutes = 10\n",
    );
    let config = load(root.path()).expect("verification task");
    let task = config.workflow.tasks.first().expect("declared task");
    assert_eq!(task.id, "native-swift-format");
    assert_eq!(task.mise_task, "desktop-format-check");
    assert_eq!(task.runner.runs_on(), "macos-15");

    let root = rooted("schema = 1\n");
    let config = load(root.path()).expect("minimal config");
    assert!(config.workflow.tasks.is_empty());
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
    ] {
        let root = rooted(body);
        let err = load(root.path()).expect_err("unsafe value must fail");
        assert!(err.to_string().contains(want), "got {err} want {want}");
    }
}

#[test]
fn unknown_rust_keys_are_rejected() {
    let load = load_config;
    for body in [
        "schema = 1\n[stacks.rust]\ncustom_tasks = [\"audit\"]\n",
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
#[test]
fn explicit_checks_load_and_minimal_config_omits_empty_checks() {
    let load = load_config;
    let root = rooted("schema = 1\n");
    let config = load(root.path()).expect("minimal config");
    assert!(config.checks.is_empty());
    let json = serde_json::to_value(&config).expect("serialize config");
    assert!(json.get("checks").is_none());
    let root = rooted(
        r#"schema = 1
[[checks]]
id = "native"
task = "test:native"
inputs = ["mise.toml"]
tools = []
[checks.runner]
label = "macos-15"
platform = "macos_arm64"
executor = "hosted"
[checks.evidence]
path = "evidence/native.json"
expected_scenarios = ["ffi"]
"#,
    );
    let config = load(root.path()).expect("explicit checks");
    assert_eq!(config.checks[0].directory, ".");
    assert_eq!(config.checks[0].timeout_minutes, 30);
}
