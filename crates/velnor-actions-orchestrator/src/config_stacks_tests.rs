use super::*;
use crate::config::{CONFIG_REL, load_config};
use velnor_actions_contract::WorkflowTask;

/// Write `body` as `.velnor/config.toml` under a fresh temp root.
fn rooted(body: &str) -> tempfile::TempDir {
    let root = tempfile::tempdir().expect("temp root");
    let dir = root.path().join(".velnor");
    std::fs::create_dir_all(&dir).expect("velnor dir");
    std::fs::write(dir.join("config.toml"), body).expect("config write");
    root
}

#[test]
fn parallel_target_seeding_loads_only_as_explicit_rust_boolean() {
    for (setting, expected) in [
        ("", false),
        ("seed_parallel_targets = false\n", false),
        ("seed_parallel_targets = true\n", true),
    ] {
        let root = rooted(&format!("schema = 1\n[stacks.rust]\n{setting}"));
        let config = load_config(root.path()).expect("Rust stack");
        assert_eq!(
            config.stacks.rust.expect("Rust").seed_parallel_targets,
            expected
        );
    }
    let root = rooted("schema = 1\n[workflow]\nseed_parallel_targets = true\n");
    assert!(
        load_config(root.path()).is_err(),
        "not a generic workflow option"
    );
    let root = rooted("schema = 1\n[stacks.rust]\nseed_parallel_targets = \"true\"\n");
    assert!(load_config(root.path()).is_err(), "not a string option");
}

#[test]
fn all_features_configuration_is_preserved_during_stack_materialization() {
    let root = rooted(
        "schema = 1\n[[stacks.rust.configurations]]\nname = \"all\"\nall_features = true\ntarget = \"host\"\n",
    );
    let rust = load_config(root.path())
        .expect("all-features configuration")
        .stacks
        .rust
        .expect("Rust stack");
    assert_eq!(rust.configurations[0].name, "all");
    assert!(rust.configurations[0].all_features);
    assert_eq!(rust.configurations[0].features, [] as [String; 0]);
    assert!(!rust.clippy_all_features);

    let root = rooted("schema = 1\n[stacks.rust]\n");
    let rust = load_config(root.path())
        .expect("default Rust configuration")
        .stacks
        .rust
        .expect("Rust stack");
    assert!(!rust.configurations[0].all_features);
}

#[test]
fn clippy_all_features_is_an_independent_optional_stack_setting() {
    let root = rooted("schema = 1\n[stacks.rust]\nclippy_all_features = true\n");
    let rust = load_config(root.path())
        .expect("Clippy feature policy")
        .stacks
        .rust
        .expect("Rust stack");
    assert!(rust.clippy_all_features);
    assert!(!rust.configurations[0].all_features);

    let root = rooted("schema = 1\n[stacks.rust]\n");
    let rust = load_config(root.path())
        .expect("default feature policy")
        .stacks
        .rust
        .expect("Rust stack");
    assert!(!rust.clippy_all_features);

    let root = rooted("schema = 1\n[stacks.rust]\nclippy_all_features = \"true\"\n");
    assert!(load_config(root.path()).is_err(), "not a string option");
}

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
    assert!(!rust.binary_release.enabled());
    assert_eq!(rust.binary_release.manifest_path, "Cargo.toml");

    let root = rooted(&native_preview_config());
    let config = load(root.path()).expect("consumer binary release config");
    let rust = config.stacks.rust.expect("rust stack");
    assert!(rust.binary_release.enabled());
    assert_eq!(rust.binary_release.version_package, "repo-scan");
    assert_eq!(rust.binary_release.products[0].package, "repo-scan");
    assert_eq!(rust.binary_release.products[0].bins, ["repo-scan"]);
}

fn native_preview_config() -> String {
    let tool = serde_json::json!({"qualified_tools":[{
        "id":"rust","version":"1.98.1","backend":{"kind":"core","tool":"rust"},
        "options":{"kind":"rust","components":[],"targets":[]},"depends_on":[],
        "platforms":[{"platform":"linux_x64","artifacts":[{"url":"https://static.rust-lang.org/dist/rust-1.98.1-x86_64-unknown-linux-gnu.tar.xz","sha256":"a".repeat(64)}],"dependency_artifacts":[],"install_tree_sha256":"b".repeat(64),
        "executables":[{"name":"rustc","path":"bin/rustc","sha256":"c".repeat(64),"probe":{"kind":"version","expected":"rustc 1.98.1"}}]}]
    }]});
    format!(
        r#"schema = 1
[workflow]
policy = "consumer-v1"
[stacks.rust.binary_release]
version_package = "repo-scan"
version_env = "REPO_SCAN_VERSION_OVERRIDE"
[stacks.rust.binary_release.evidence]
profile = "signed-sbom-v1"
[stacks.rust.binary_release.producer]
task = "release-native"
tools = ["rust"]
inputs = ["Cargo.toml", "Cargo.lock", "mise.toml"]
[[stacks.rust.binary_release.products]]
id = "repo-scan"
package = "repo-scan"
bins = ["repo-scan"]
targets = ["aarch64-apple-darwin"]
archive = "tar.gz"
[stacks.rust.binary_release.preview]
enabled = true
release_id = 1
{}"#,
        toml::to_string(&tool).expect("qualified loader fixture")
    )
}

#[test]
fn workflow_task_union_parses_variants_and_defaults_empty() {
    let load = load_config;
    let root = rooted(
        "schema = 1\n[[workflow.tasks]]\nkind = \"build\"\nid = \"native-desktop\"\nmise_task = \"desktop-ci\"\nsource = { mise_config = \"native/mise.toml\", working_directory = \"native\" }\ntools = [\"mr-boxington\", \"rust\"]\nrunner = \"macos-26-arm64\"\ntimeout_minutes = 5\ncargo_build_jobs = 2\nnextest_test_threads = 2\n[[workflow.tasks]]\nkind = \"verification\"\nid = \"native-swift-format\"\nmise_task = \"desktop-format-check\"\nsource = { mise_config = \"native/mise.toml\", working_directory = \"native\" }\nrunner = \"macos-arm64\"\ntimeout_minutes = 5\n",
    );
    let config = load(root.path()).expect("workflow task variants");
    let Some(WorkflowTask::Build(build)) = config.workflow.tasks.first() else {
        panic!("first declared task is build")
    };
    assert_eq!(build.id, "native-desktop");
    assert_eq!(build.mise_task, "desktop-ci");
    assert_eq!(build.source.mise_config, "native/mise.toml");
    assert_eq!(build.source.working_directory, "native");
    assert_eq!(build.tools, ["mr-boxington", "rust"]);
    assert_eq!(build.runner.runs_on(), "macos-26");
    assert_eq!(build.cargo_build_jobs, 2);
    assert_eq!(build.nextest_test_threads, 2);
    let Some(WorkflowTask::Verification(verification)) = config.workflow.tasks.get(1) else {
        panic!("second declared task is verification")
    };
    assert_eq!(verification.id, "native-swift-format");
    assert_eq!(verification.mise_task, "desktop-format-check");
    assert_eq!(verification.source.mise_config, "native/mise.toml");
    assert_eq!(verification.source.working_directory, "native");
    assert_eq!(verification.runner.runs_on(), "macos-15");

    let root = rooted("schema = 1\n");
    let config = load(root.path()).expect("minimal config");
    assert_eq!(
        config.workflow.tasks,
        [] as [velnor_actions_contract::WorkflowTask; 0]
    );

    let legacy = rooted(
        "schema = 1\n[[workflow.build_tasks]]\nid = \"native-desktop\"\nmise_task = \"desktop-ci\"\ntools = [\"mr-boxington\", \"rust\"]\nrunner = \"macos-26-arm64\"\ntimeout_minutes = 5\ncargo_build_jobs = 2\nnextest_test_threads = 2\n",
    );
    assert!(
        load(legacy.path()).is_err(),
        "parallel task list is rejected"
    );
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
    assert_eq!(config.checks, [] as [velnor_actions_contract::MiseCheck; 0]);
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
    assert_eq!(config.checks[0].timeout_minutes, 5);
}

#[test]
fn crate_build_jobs_parse_validate_and_reject_unknown_keys() {
    let load = load_config;
    let root = rooted("schema = 1\n[stacks.rust.crates.platform-preview-app]\nbuild_jobs = 1\n");
    let config = load(root.path()).expect("crate cap config");
    let rust = config.stacks.rust.expect("rust stack");
    assert_eq!(rust.crates["platform-preview-app"].build_jobs, 1);
    let root = rooted("schema = 1\n[stacks.rust]\n");
    let rust = load(root.path())
        .expect("no crates")
        .stacks
        .rust
        .expect("rust stack");
    assert!(rust.crates.is_empty());
    for body in [
        "schema = 1\n[stacks.rust.crates.demo]\nbuild_jobs = 0\n",
        "schema = 1\n[stacks.rust.crates.demo]\nbuild_jobs = 3\n",
        "schema = 1\n[stacks.rust.crates.demo]\nbuild_jobs = 1\njobs = 1\n",
        "schema = 1\n[stacks.rust.crates.demo]\n",
    ] {
        let root = rooted(body);
        assert!(load(root.path()).is_err(), "{body}");
    }
    let root = rooted(
        "schema = 1\n[resources]\ncompiler_process_budget = 4\ntest_process_budget = 2\n[stacks.rust.crates.demo]\nbuild_jobs = 3\n",
    );
    assert!(load(root.path()).is_ok(), "cap within raised budget");
}

#[test]
fn bun_stack_loads_strictly_and_requires_roots() {
    let root = rooted(
        "schema = 1\n[stacks.bun]\nroots = [\"frontend\"]\nscripts = [\"test\"]\n[stacks.bun.members.\"@chainargos/ui\"]\nbrowsers = [\"chromium\"]\n",
    );
    let bun = load_config(root.path())
        .expect("Bun stack")
        .stacks
        .bun
        .expect("Bun");
    assert_eq!(bun.roots[0].as_str(), "frontend");
    assert_eq!(bun.scripts, ["test"]);
    assert_eq!(bun.members["@chainargos/ui"].browsers.len(), 1);
    for body in [
        "schema = 1\n[stacks.bun]\n",
        "schema = 1\n[stacks.bun]\nroots = []\n",
        "schema = 1\n[stacks.bun]\nroots = [\".\"]\nrun = \"bun test\"\n",
        "schema = 1\n[stacks.bun]\nroots = [\"../escape\"]\n",
    ] {
        assert!(load_config(rooted(body).path()).is_err(), "{body}");
    }
}

#[test]
fn dbt_stack_loads_strictly_and_requires_projects() {
    let root = rooted(
        "schema = 1\n[stacks.dbt]\nprojects = [\"chainargos-dbt\"]\nservices = [\"postgres\"]\nchecks = [\"parse\", \"compile\"]\n[stacks.dbt.env]\nDBT_HOST = \"127.0.0.1\"\n",
    );
    let dbt = load_config(root.path())
        .expect("dbt stack")
        .stacks
        .dbt
        .expect("dbt");
    assert_eq!(dbt.projects[0].as_str(), "chainargos-dbt");
    assert_eq!(dbt.env["DBT_HOST"], "127.0.0.1");
    for body in [
        "schema = 1\n[stacks.dbt]\n",
        "schema = 1\n[stacks.dbt]\nprojects = []\n",
        "schema = 1\n[stacks.dbt]\nprojects = [\".\"]\nrun = \"dbt run\"\n",
        "schema = 1\n[stacks.dbt]\nprojects = [\".\"]\nchecks = [\"compile\"]\n",
        "schema = 1\n[stacks.dbt]\nprojects = [\".\"]\n[stacks.dbt.env]\nHOME = \"x\"\n",
    ] {
        assert!(load_config(rooted(body).path()).is_err(), "{body}");
    }
}

#[test]
fn ansible_stack_loads_strictly_and_requires_its_inputs() {
    let sha = "a".repeat(64);
    let root = rooted(&format!(
        "schema = 1\n[stacks.ansible]\nroots = [\"ansible-configs\"]\npython_requirements = \"ansible-configs/requirements.txt\"\ninventory = \"ansible-configs/hosts.ini\"\n[[stacks.ansible.collections]]\nname = \"ansible.posix\"\nversion = \"2.2.2\"\nsha256 = \"{sha}\"\n"
    ));
    let ansible = load_config(root.path())
        .expect("ansible stack")
        .stacks
        .ansible
        .expect("ansible");
    assert_eq!(ansible.roots[0].as_str(), "ansible-configs");
    assert_eq!(ansible.collections[0].name, "ansible.posix");
    for body in [
        "schema = 1\n[stacks.ansible]\n",
        "schema = 1\n[stacks.ansible]\nroots = [\"a\"]\n",
        "schema = 1\n[stacks.ansible]\nroots = [\"a\"]\npython_requirements = \"r.txt\"\ninventory = \"../h\"\n",
        "schema = 1\n[stacks.ansible]\nroots = [\"a\"]\npython_requirements = \"r.txt\"\ninventory = \"h\"\nplaybook = \"x\"\n",
    ] {
        assert!(load_config(rooted(body).path()).is_err(), "{body}");
    }
}
