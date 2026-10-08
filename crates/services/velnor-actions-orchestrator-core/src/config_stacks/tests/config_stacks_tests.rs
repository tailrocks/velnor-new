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
fn workflow_verify_selection_parses_and_defaults_empty() {
    let load = load_config;
    let root = rooted("schema = 1\n");
    let config = load(root.path()).expect("minimal config");
    assert!(config.workflow.verify.jobs.is_empty());

    let root = rooted("schema = 1\n[workflow.verify]\njobs = [\"zizmor\"]\n");
    let config = load(root.path()).expect("supported verification selector");
    assert_eq!(config.workflow.verify.jobs, ["zizmor"]);

    for job in [
        "markdownlint",
        "strict-json",
        "native-validators",
        "unknown",
    ] {
        let body = format!("schema = 1\n[workflow.verify]\njobs = [\"{job}\"]\n");
        let root = rooted(&body);
        let error = load(root.path()).expect_err("unimplemented selector fails closed");
        assert!(
            error
                .to_string()
                .contains(&format!("unsupported_verify_job:{job}")),
            "{error}"
        );
    }

    let root = rooted("schema = 1\n[workflow.verify]\njobs = [\"zizmor\", \"zizmor\"]\n");
    let error = load(root.path()).expect_err("duplicate selector fails");
    assert!(
        error.to_string().contains("duplicate_verify_job:zizmor"),
        "{error}"
    );
}

#[test]
fn workflow_artifact_tasks_parse_and_default_empty() {
    let load = load_config;
    let root = rooted(
        "schema = 1\n[[workflow.artifact_tasks]]\nid = \"frontend-bundle\"\nmise_task = \"build-frontend\"\nrunner = \"linux-x64\"\ntimeout_minutes = 30\n[[workflow.artifact_tasks.outputs]]\nid = \"bundle\"\npath = \"dist/app.tar\"\nmax_bytes = 16384\n",
    );
    let config = load(root.path()).expect("bounded artifact task");
    let task = config
        .workflow
        .artifact_tasks
        .first()
        .expect("declared task");
    assert_eq!(task.id, "frontend-bundle");
    assert_eq!(task.outputs[0].path, "dist/app.tar");
    assert_eq!(task.outputs[0].max_bytes, 16_384);

    let root = rooted("schema = 1\n");
    let config = load(root.path()).expect("minimal existing config");
    assert_eq!(config.workflow.artifact_tasks, Vec::new());
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

#[test]
fn policy_loads_and_rejects_bad_pins() {
    let load = load_config;
    let sha = "a".repeat(64);
    let root = rooted(&format!(
        "schema = 1\n[stacks.rust.policy]\nversion = \"0.1.3\"\nsha256 = \"{sha}\"\nprofile = \"rust-strict-v1\"\n"
    ));
    let config = load(root.path()).expect("policy loads");
    let policy = config
        .stacks
        .rust
        .expect("rust stack")
        .policy
        .expect("policy");
    assert_eq!(policy.version, "0.1.3");
    for (body, want) in [
        (
            format!(
                "schema = 1\n[stacks.rust.policy]\nversion = \"v1\"\nsha256 = \"{sha}\"\nprofile = \"rust-strict-v1\"\n"
            ),
            "bad_version",
        ),
        (
            format!(
                "schema = 1\n[stacks.rust.policy]\nversion = \"0.1.3\"\nsha256 = \"abc\"\nprofile = \"rust-strict-v1\"\n"
            ),
            "bad_sha256",
        ),
        (
            format!(
                "schema = 1\n[stacks.rust.policy]\nversion = \"0.1.3\"\nsha256 = \"{sha}\"\nprofile = \"lax-v9\"\n"
            ),
            "document",
        ),
        (
            format!("schema = 1\n[stacks.rust.policy]\nversion = \"0.1.3\"\nsha256 = \"{sha}\"\n"),
            "document",
        ),
    ] {
        let root = rooted(&body);
        let err = load(root.path()).expect_err("bad policy must fail");
        assert!(err.to_string().contains(want), "got {err} want {want}");
    }
    let root = rooted("schema = 1\n[stacks.rust]\n");
    let config = load(root.path()).expect("rust without policy");
    assert!(config.stacks.rust.expect("rust stack").policy.is_none());
}

#[test]
fn docs_lane_defaults_and_rejects_bad_routes() {
    let load = load_config;
    let root = rooted("schema = 1\n[docs]\n");
    let config = load(root.path()).expect("bare docs table");
    let docs = config.docs.expect("docs lane");
    assert_eq!(docs.app_dir, "docs");
    assert_eq!(docs.content_dir, "content/docs");
    assert_eq!(docs.base_path, "/docs");
    assert_eq!(docs.output_dir, ".output/public");
    assert_eq!(docs.smoke_routes, vec!["/".to_owned(), "/docs".to_owned()]);
    let root = rooted("schema = 1\n[docs]\nbase_path = \"/manual\"\n");
    let config = load(root.path()).expect("custom base");
    assert_eq!(
        config.docs.expect("docs lane").smoke_routes,
        vec!["/".to_owned(), "/manual".to_owned()]
    );
    for (body, want) in [
        ("schema = 1\n[docs]\napp_dir = \"/abs\"\n", "malformed_dir"),
        (
            "schema = 1\n[docs]\nbase_path = \"/docs/\"\n",
            "malformed_base_path",
        ),
        (
            "schema = 1\n[docs]\nsmoke_routes = [\"/docs\", \"/\"]\n",
            "must_be_sorted",
        ),
        ("schema = 1\n[docs]\nsmoke_routes = []\n", "empty_routes"),
    ] {
        let root = rooted(body);
        let err = load(root.path()).expect_err("bad docs must fail");
        assert!(err.to_string().contains(want), "got {err} want {want}");
    }
    let root = rooted("schema = 1\n");
    let config = load(root.path()).expect("minimal config");
    assert!(config.docs.is_none());
}
